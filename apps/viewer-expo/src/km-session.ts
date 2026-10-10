import { KmError, kmStartFailure, type KmIssue, type KmStartStage } from "./km-failure";
export { KmError, kmReasonIssue, type KmIssue, type KmFailure, type KmStartStage } from "./km-failure";

/** Commands and low-frequency status only. Native threads own every input event. */
export interface KmConsent { macToAndroid: boolean; androidToMac: boolean }
export interface KmAndroidCapabilities { cursorInjection: boolean; mouseObservation: boolean; mouseCapture: boolean; keyboardCapture: boolean }
export interface KmNativeStatus {
  state: "off" | "cleanupFailed" | "preparing" | "armed" | "macToAndroid" | "androidToMac";
  reason?: string | null;
  shizukuRunning: boolean; shizukuGranted: boolean; accessibilityEnabled: boolean;
  displayWidth: number; displayHeight: number;
  capabilities: KmAndroidCapabilities;
}
export interface KmHostStatus {
  owner: "local" | "macToAndroid" | "androidToMac"; armed: boolean; epoch: number;
  capabilities: { macCapture: boolean; macInjection: boolean; android: KmAndroidCapabilities };
  blockedReason?: string | null;
}
export interface KmStartOptions extends KmConsent { arrangementSide: "left" | "right" }
export interface KmNativeConfig extends KmStartOptions { host: string; port: number; sessionId: string; sessionKey: string; epoch: number }
export interface KmNative {
  getStatus(): Promise<KmNativeStatus>; prepare(): Promise<KmNativeStatus>;
  start(config: KmNativeConfig): Promise<KmNativeStatus>; stop(): Promise<void>;
}
export interface KmPeer {
  host: string; request<T>(command: string, args?: unknown): Promise<T>; close(): void;
  isCurrent?(): boolean;
}
export function kmStartBlocker(status: KmNativeStatus, consent: KmConsent): KmIssue | null {
  if (!consent.macToAndroid || !consent.androidToMac) return "consentRequired";
  if (!status.shizukuRunning) return "shizukuUnavailable";
  if (!status.shizukuGranted) return "shizukuPermission";
  if (!status.accessibilityEnabled) return "accessibilityPermission";
  if (!Object.values(status.capabilities).every(Boolean)) return "unsupported";
  if (![status.displayWidth, status.displayHeight].every(n => Number.isInteger(n) && n > 0 && n <= 32768)) return "displayUnavailable";
  return null;
}
function cancelled(): Error { const error = new Error("Input sharing start cancelled"); error.name = "AbortError"; return error; }
interface KmStartResult { sessionId: string; epoch: number; port: number; sessionKey: string; status: KmHostStatus }

/** App-wide lifecycle; leaving a panel cancels only its unfinished start. */
export class KmSession {
  private pending: { cancellation: AbortController; promise: Promise<KmNativeStatus> } | null = null;
  private active: KmPeer | null = null;
  private stopping: Promise<void> | null = null;
  private options: KmStartOptions = { arrangementSide: "right", macToAndroid: false, androidToMac: false };
  constructor(private readonly dependencies: { native: KmNative; openPeer(signal: AbortSignal): Promise<KmPeer> }) {}
  getPeerHost(): string | null { return this.active?.host ?? null; }
  getOptions(): KmStartOptions { return { ...this.options }; }
  setOptions(options: KmStartOptions): void { this.options = { ...options }; }
  cancelPending(): void { this.pending?.cancellation.abort(); }
  start(options: KmStartOptions): Promise<KmNativeStatus> {
    if (this.pending || this.active || this.stopping) return Promise.reject(new KmError("busy"));
    if (!options.macToAndroid || !options.androidToMac) return Promise.reject(new KmError("consentRequired"));
    this.setOptions(options);
    const cancellation = new AbortController();
    const promise = this.startPrepared(options, cancellation.signal);
    this.pending = { cancellation, promise };
    const retire = () => { if (this.pending?.promise === promise) this.pending = null; };
    void promise.then(retire, retire);
    return promise;
  }
  private async startPrepared(options: KmStartOptions, signal: AbortSignal): Promise<KmNativeStatus> {
    let peer: KmPeer | null = null;
    let preparing = false;
    let negotiating = false;
    let stage: KmStartStage = "connectionSetup";
    const check = () => { if (signal.aborted || peer?.isCurrent?.() === false) throw cancelled(); };
    try {
      peer = await this.dependencies.openPeer(signal); check();
      stage = "androidPreparation";
      preparing = true;
      const status = await this.dependencies.native.prepare(); check();
      const blocker = kmStartBlocker(status, options);
      if (blocker) throw new KmError(blocker);
      stage = "hostApproval";
      negotiating = true;
      const result = await peer.request<KmStartResult>("startKm", {
        arrangementSide: options.arrangementSide, tabletWidth: status.displayWidth, tabletHeight: status.displayHeight,
        viewerConsent: { macToAndroid: options.macToAndroid, androidToMac: options.androidToMac }, androidCapabilities: status.capabilities,
      });
      check();
      stage = "nativeHandshake";
      // The session key exists only in this stack frame and the native transport.
      const started = await this.dependencies.native.start({ ...options, host: peer.host, port: result.port,
        sessionId: result.sessionId, sessionKey: result.sessionKey, epoch: result.epoch });
      check();
      if (!["armed", "macToAndroid", "androidToMac"].includes(started.state)) throw kmStartFailure(started.reason, stage);
      this.active = peer;
      return started;
    } catch (cause) {
      const failure = kmStartFailure(cause, stage);
      let cleanupFailed = false;
      if (preparing) await this.dependencies.native.stop().catch(() => { cleanupFailed = true; });
      // Even a rejected/ambiguous request may have created a remote session.
      if (negotiating) await peer?.request("stopKm").catch(() => { cleanupFailed = true; });
      peer?.close();
      if (cleanupFailed) throw new KmError("stopFailed");
      check();
      if (cause instanceof Error && cause.name === "AbortError") throw cause;
      throw failure;
    }
  }
  stop(): Promise<void> {
    if (this.stopping) return this.stopping;
    const promise = this.stopSession();
    this.stopping = promise;
    const retire = () => { if (this.stopping === promise) this.stopping = null; };
    void promise.then(retire, retire);
    return promise;
  }
  private async stopSession(): Promise<void> {
    const pending = this.pending;
    this.cancelPending();
    if (pending) await pending.promise.catch(() => undefined);
    const peer = this.active;
    this.active = null;
    let failed = false;
    await this.dependencies.native.stop().catch(() => { failed = true; });
    if (peer) {
      await peer.request("stopKm").catch(() => { failed = true; });
      peer.close();
    }
    if (failed) throw new KmError("stopFailed");
  }
  /** Native stop (permission loss/orientation/revoke) retires only its control lease. */
  async reconcile(status: KmNativeStatus): Promise<void> {
    if (status.state === "cleanupFailed") {
      const peer = this.active;
      this.active = null;
      if (peer) {
        try { await peer.request("stopKm"); } finally { peer.close(); }
      }
      throw new KmError("stopFailed");
    }
    if (status.state === "off" && this.active) {
      await this.stop();
    }
  }
}
