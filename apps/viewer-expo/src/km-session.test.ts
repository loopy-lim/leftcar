import { describe, expect, it } from "vitest";
import { KmSession, kmStartBlocker, type KmNative, type KmNativeStatus, type KmPeer } from "./km-session";

const prepared: KmNativeStatus = { state: "preparing", shizukuRunning: true, shizukuGranted: true, accessibilityEnabled: true,
  displayWidth: 2560, displayHeight: 1600,
  capabilities: { cursorInjection: true, mouseObservation: true, mouseCapture: true, keyboardCapture: true } };
const consent = { macToAndroid: true, androidToMac: true };
function deferred<T>() { let resolve!: (v: T) => void; let reject!: (cause: unknown) => void; const promise = new Promise<T>((r, fail) => { resolve = r; reject = fail; }); return { promise, resolve, reject }; }
function rig(overrides: Partial<KmNative> = {}, negotiate?: Promise<unknown>, openFailure?: unknown) {
  const effects: { kind: string; data?: unknown }[] = [];
  const native: KmNative = {
    getStatus: async () => prepared,
    prepare: async () => { effects.push({ kind: "prepare" }); return prepared; },
    start: async config => { effects.push({ kind: "nativeStart", data: config }); return { ...prepared, state: "armed" }; },
    stop: async () => { effects.push({ kind: "nativeStop" }); }, ...overrides,
  };
  const peer: KmPeer = { host: "192.168.1.8", close: () => { effects.push({ kind: "close" }); },
    request: async <T>(command: string, args?: unknown) => {
      effects.push({ kind: command, data: args });
      if (command === "startKm") return (await (negotiate ?? Promise.resolve({ sessionId: "km-1", epoch: 1, port: 8888, sessionKey: "memory-only-secret", status: {} }))) as T;
      return {} as T;
    } };
  const session = new KmSession({ native, openPeer: async () => { effects.push({ kind: "wifi" }); if (openFailure) throw openFailure; return peer; } });
  return { session, effects, peer };
}
describe("input-only KM lifecycle", () => {
  it.each([
    ["cancel", "prepare"], ["cancel", "startKm"], ["cancel", "nativeStart"],
    ["stale", "prepare"], ["stale", "startKm"], ["stale", "nativeStart"],
  ] as const)("retires %s startup after %s rejects, but only after cleanup", async (retirement, failing) => {
    const entered = deferred<void>(); const dependency = deferred<KmNativeStatus>(); const cleaned = deferred<void>();
    const { session, effects, peer } = rig({
      ...(failing === "prepare" ? { prepare: async () => { entered.resolve(); return dependency.promise; } } : {}),
      ...(failing === "nativeStart" ? { start: async () => { entered.resolve(); return dependency.promise; } } : {}),
      stop: async () => { effects.push({ kind: "nativeStop" }); await cleaned.promise; },
    });
    let current = true; peer.isCurrent = () => current;
    if (failing === "startKm") {
      const request = peer.request;
      peer.request = async <T>(command: string, args?: unknown) => {
        if (command === "startKm") { entered.resolve(); return await dependency.promise as T; }
        return request<T>(command, args);
      };
    }
    const starting = session.start({ arrangementSide: "right", ...consent });
    let settled = false; void starting.then(() => { settled = true; }, () => { settled = true; });
    await entered.promise;
    if (retirement === "cancel") session.cancelPending(); else current = false;
    dependency.reject(new Error("Shizuku 입력 서비스 연결 시간이 초과되었습니다"));
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(settled).toBe(false); expect(effects.at(-1)?.kind).toBe("nativeStop");
    cleaned.resolve();
    await expect(starting).rejects.toMatchObject({ name: "AbortError" });
    expect(effects.at(-1)?.kind).toBe("close"); expect(session.getPeerHost()).toBeNull();
    if (failing !== "prepare") expect(effects.slice(-3).map(e => e.kind)).toEqual(["nativeStop", "stopKm", "close"]);
  });
  it("retires a cancelled connection setup that rejects before returning a peer", async () => {
    const entered = deferred<void>(); const opened = deferred<KmPeer>();
    const effects: { kind: string }[] = [];
    const native: KmNative = { getStatus: async () => prepared, prepare: async () => prepared, start: async () => ({ ...prepared, state: "armed" }), stop: async () => { effects.push({ kind: "nativeStop" }); } };
    const pending = new KmSession({ native, openPeer: async () => { entered.resolve(); return opened.promise; } });
    const starting = pending.start({ arrangementSide: "right", ...consent });
    await entered.promise; pending.cancelPending(); opened.reject(new Error("network fixture"));
    await expect(starting).rejects.toMatchObject({ name: "AbortError" });
    expect(effects).toEqual([]); expect(pending.getPeerHost()).toBeNull();
  });
  it.each(["cancel", "stale"] as const)("keeps failed cleanup dominant over a %s rejecting startup", async retirement => {
    const entered = deferred<void>(); const preparation = deferred<KmNativeStatus>();
    const { session, peer, effects } = rig({ prepare: async () => { entered.resolve(); return preparation.promise; }, stop: async () => { effects.push({ kind: "nativeStop" }); throw new Error("cleanup fixture"); } });
    let current = true; peer.isCurrent = () => current;
    const starting = session.start({ arrangementSide: "right", ...consent }); await entered.promise;
    if (retirement === "cancel") session.cancelPending(); else current = false;
    preparation.reject(new Error("Shizuku 입력 서비스 연결 시간이 초과되었습니다"));
    await expect(starting).rejects.toMatchObject({ issue: "stopFailed" });
    expect(effects.slice(-2).map(e => e.kind)).toEqual(["nativeStop", "close"]);
  });

  it.each([
    ["connectionSetup", "openPeer"], ["androidPreparation", "prepare"],
    ["hostApproval", "startKm"], ["nativeHandshake", "nativeStart"],
  ] as const)("preserves %s attribution after cleanup without retaining an unknown cause", async (stage, failing) => {
    const secret = "sessionKey=secret-fixture token=private-fixture keyInput=user-fixture";
    const cause = Object.assign(new Error(secret), { code: "unknown", reason: secret, sessionKey: secret });
    const { session, effects } = rig({
      ...(failing === "prepare" ? { prepare: async () => { throw cause; } } : {}),
      ...(failing === "nativeStart" ? { start: async () => { throw cause; } } : {}),
    }, failing === "startKm" ? Promise.reject(cause) : undefined, failing === "openPeer" ? cause : undefined);
    const error = await session.start({ arrangementSide: "right", ...consent }).catch(error => error);
    expect(error).toMatchObject({ issue: "startFailed", stage });
    expect(JSON.stringify(error)).not.toContain(secret);
    expect(error.message).not.toContain(secret);
    expect(session.getPeerHost()).toBeNull();
    if (failing !== "openPeer") expect(effects.at(-1)?.kind).toBe("close");
  });
  it.each([
    [new Error("KeyBridge physical key ownership unavailable"), "keyBridgeUnsupported"],
    [{ code: "km_prepare_failed", message: "KeyBridge 입력 공유 연동 API가 필요합니다. KeyBridge를 활성화한 채 지원 업데이트를 적용하세요" }, "keyBridgeUnsupported"],
    [{ code: "km_prepare_failed", message: "Shizuku 입력 서비스 연결 시간이 초과되었습니다" }, "helperUnavailable"],
    [{ code: "km_prepare_failed", message: "Leftcar 접근성 권한이 필요합니다" }, "accessibilityPermission"],
  ] as const)("keeps a recognized Android preparation reason when stop clears native status: %s", async (cause, issue) => {
    let status = { ...prepared, state: "off" as const, reason: "original reason" };
    const { session } = rig({ prepare: async () => { throw cause; }, getStatus: async () => status,
      stop: async () => { status = { ...status, reason: "" }; } });
    await expect(session.start({ arrangementSide: "right", ...consent })).rejects.toMatchObject({ issue, stage: "androidPreparation" });
    expect(status.reason).toBe("");
  });
  it("preserves a reason returned by native start rather than a rejected promise", async () => {
    const { session } = rig({ start: async () => ({ ...prepared, state: "off", reason: "입력 공유 상태 확인 시간이 초과되었습니다" }) });
    await expect(session.start({ arrangementSide: "right", ...consent })).rejects.toMatchObject({ issue: "nativeTimeout", stage: "nativeHandshake" });
  });
  it("normalizes a structured Host rejection without exposing the rejected payload", async () => {
    const { session } = rig({}, Promise.reject({ message: "Approve both input-sharing directions for this peer on the Mac", sessionKey: "secret-fixture" }));
    const error = await session.start({ arrangementSide: "right", ...consent }).catch(error => error);
    expect(error).toMatchObject({ issue: "hostConsentRequired", stage: "hostApproval" });
    expect(JSON.stringify(error)).not.toContain("secret-fixture");
  });
  it("cleans a native failure even if a diagnostic property getter throws", async () => {
    const cause = { get message() { throw new Error("secret-fixture"); } };
    const { session, effects } = rig({ prepare: async () => { throw cause; } });
    await expect(session.start({ arrangementSide: "right", ...consent })).rejects.toMatchObject({ issue: "startFailed", stage: "androidPreparation" });
    expect(effects.slice(-2).map(e => e.kind)).toEqual(["nativeStop", "close"]);
  });
  it("lets failed cleanup dominate a recognized startup cause", async () => {
    const { session } = rig({ prepare: async () => { throw new Error("KeyBridge physical key ownership unavailable"); }, stop: async () => { throw new Error("cleanup failed"); } });
    await expect(session.start({ arrangementSide: "right", ...consent })).rejects.toMatchObject({ issue: "stopFailed" });
  });
  it("prepares before negotiation and starts native input with physical pixels, without video commands", async () => {
    const { session, effects } = rig();
    await session.start({ arrangementSide: "left", ...consent });
    expect(effects.map(e => e.kind)).toEqual(["wifi", "prepare", "startKm", "nativeStart"]);
    expect(effects.find(e => e.kind === "startKm")?.data).toEqual({ arrangementSide: "left", tabletWidth: 2560, tabletHeight: 1600, viewerConsent: consent, androidCapabilities: prepared.capabilities });
    expect(effects.find(e => e.kind === "nativeStart")?.data).toEqual({ host: "192.168.1.8", port: 8888, sessionId: "km-1", epoch: 1, sessionKey: "memory-only-secret", arrangementSide: "left", ...consent });
    await session.stop();
    expect(effects.slice(-3).map(e => e.kind)).toEqual(["nativeStop", "stopKm", "close"]);
  });
  it("keeps completed native input alive when its UI cancels pending work on unmount", async () => {
    const { session, effects } = rig(); await session.start({ arrangementSide: "right", ...consent });
    session.cancelPending();
    expect(session.getOptions()).toEqual({ arrangementSide: "right", ...consent });
    expect(effects.map(e => e.kind)).toEqual(["wifi", "prepare", "startKm", "nativeStart"]);
  });
  it("cleans late negotiation after UI departure before any native start", async () => {
    const pending = deferred<unknown>(); const { session, effects } = rig({}, pending.promise);
    const starting = session.start({ arrangementSide: "right", ...consent });
    await new Promise(resolve => setTimeout(resolve, 0)); session.cancelPending();
    pending.resolve({ sessionId: "late", epoch: 1, port: 8888, sessionKey: "secret", status: {} });
    await expect(starting).rejects.toMatchObject({ name: "AbortError" });
    expect(effects.map(e => e.kind)).toEqual(["wifi", "prepare", "startKm", "nativeStop", "stopKm", "close"]);
  });
  it("cleans prepared helper and negotiated host session when native start fails", async () => {
    const { session, effects } = rig({ start: async () => { throw new Error("native failed secret"); } });
    await expect(session.start({ arrangementSide: "right", ...consent })).rejects.toMatchObject({ issue: "startFailed" });
    expect(effects.slice(-3).map(e => e.kind)).toEqual(["nativeStop", "stopKm", "close"]);
  });
  it("cleans prepared helper when host rejects negotiation", async () => {
    const { session, effects } = rig({}, Promise.reject(new Error("Host consent denied")));
    await expect(session.start({ arrangementSide: "right", ...consent })).rejects.toMatchObject({ issue: "startFailed" });
    expect(effects.slice(-3).map(e => e.kind)).toEqual(["nativeStop", "stopKm", "close"]);
  });
  it("blocks unsupported helper before host negotiation", async () => {
    const { session, effects } = rig({ prepare: async () => ({ ...prepared, capabilities: { ...prepared.capabilities, mouseCapture: false } }) });
    await expect(session.start({ arrangementSide: "right", ...consent })).rejects.toMatchObject({ issue: "unsupported" });
    expect(effects.map(e => e.kind)).toEqual(["wifi", "nativeStop", "close"]);
  });
  it("cleans cancellation after native start completes before publishing an active session", async () => {
    const nativeStarting = deferred<KmNativeStatus>();
    const { session, effects } = rig({ start: async () => { effects.push({ kind: "nativeStarting" }); return nativeStarting.promise; } });
    const starting = session.start({ arrangementSide: "right", ...consent });
    await new Promise(resolve => setTimeout(resolve, 0)); session.cancelPending();
    nativeStarting.resolve({ ...prepared, state: "armed" });
    await expect(starting).rejects.toMatchObject({ name: "AbortError" });
    expect(effects.slice(-3).map(e => e.kind)).toEqual(["nativeStop", "stopKm", "close"]);
  });
  it("fences a new start until an explicit stop finishes native cleanup", async () => {
    const stopped = deferred<void>();
    const { session } = rig({ stop: async () => stopped.promise });
    await session.start({ arrangementSide: "right", ...consent });
    const stopping = session.stop();
    await expect(session.start({ arrangementSide: "left", ...consent })).rejects.toMatchObject({ issue: "busy" });
    stopped.resolve(); await stopping;
  });
  it("surfaces failed native cleanup, closes the host, and retries native cleanup explicitly", async () => {
    let fail = true;
    const { session, effects } = rig({ stop: async () => { effects.push({ kind: "nativeStop" }); if (fail) throw new Error("km_stop_failed"); } });
    await session.start({ arrangementSide: "right", ...consent });
    await expect(session.stop()).rejects.toMatchObject({ issue: "stopFailed" });
    expect(effects.slice(-3).map(e => e.kind)).toEqual(["nativeStop", "stopKm", "close"]);
    fail = false;
    await session.stop();
    expect(effects.at(-1)?.kind).toBe("nativeStop");
  });
  it("retires the host on unsolicited failed cleanup without auto-clearing native recovery", async () => {
    const { session, effects } = rig();
    await session.start({ arrangementSide: "right", ...consent });
    await expect(session.reconcile({ ...prepared, state: "cleanupFailed" })).rejects.toMatchObject({ issue: "stopFailed" });
    expect(effects.slice(-2).map(e => e.kind)).toEqual(["stopKm", "close"]);
    expect(effects.some(e => e.kind === "nativeStop")).toBe(false);
    expect(session.getPeerHost()).toBeNull();
  });
  it("requires explicit approval of both directions and verified permission/geometry", () => {
    expect(kmStartBlocker(prepared, { ...consent, androidToMac: false })).toBe("consentRequired");
    expect(kmStartBlocker({ ...prepared, shizukuGranted: false }, consent)).toBe("shizukuPermission");
    expect(kmStartBlocker({ ...prepared, accessibilityEnabled: false }, consent)).toBe("accessibilityPermission");
    expect(kmStartBlocker({ ...prepared, displayWidth: 0 }, consent)).toBe("displayUnavailable");
    expect(kmStartBlocker(prepared, consent)).toBeNull();
  });
});
