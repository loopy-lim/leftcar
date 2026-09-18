import type { ActiveStream, RestoredStream } from "./catalog-model-types";
import type { StreamSessionStore } from "./stream-session-store";
import type { StreamTerminationEvent, StreamTerminationSubscription } from "./stream-termination-policy";
import { claimStreamRestore, releaseStreamRestore, selectRecoverableStream } from "./stream-termination-policy";
import { replaceRestartedStreamState } from "./launch-stream";

export interface StreamRecoveryDependencies {
  subscribe(listener: (event: StreamTerminationEvent) => void): StreamTerminationSubscription;
  restore(active: ActiveStream): Promise<RestoredStream>;
  stop(session: number): Promise<void>;
  formatError?(error: unknown): string;
}

/** Backoff between consecutive native-termination restores. Every recreate
 * restarts the host bitrate ladder at the profile ideal, which re-collapses
 * a weak link (XR-class Wi-Fi) and terminates again — the measured 8-session
 * death spiral. The first recovery stays immediate; repeats wait 2s, 4s, 8s…
 * capped at 30s. One quiet minute proves the stream lived long enough that
 * the next failure is a fresh incident, not the same collapse. */
const RESTORE_BACKOFF_RESET_MS = 60_000;
const RESTORE_BACKOFF_BASE_MS = 2_000;
const RESTORE_BACKOFF_MAX_MS = 30_000;

export function restoreBackoffDelayMs(repeatIndex: number): number {
  if (repeatIndex <= 0) return 0;
  return Math.min(RESTORE_BACKOFF_BASE_MS * 2 ** (repeatIndex - 1), RESTORE_BACKOFF_MAX_MS);
}

/** A native window, rather than the catalog component, owns recovery. */
export class StreamRecoveryController {
  private subscription?: StreamTerminationSubscription;
  private restoreRepeats = 0;
  private lastRestoreStartedAt = 0;
  /** failedRetry=true: 복원 실패 뒤 자동 재시도 타이머 — 새 네이티브 이벤트가
   * 오면 대체해 즉시 시도한다. false: 성공 뒤 재생성 폭주 방지 백오프 —
   * 새 이벤트는 이 타이머에 합쳐진다(기존 동작). */
  private readonly pendingRestores = new Map<number, { timer: ReturnType<typeof setTimeout>, failedRetry: boolean }>();
  constructor(private readonly store: StreamSessionStore, private dependencies: StreamRecoveryDependencies) {
    store.subscribe(this.syncSubscription);
    this.syncSubscription();
  }

  configure(dependencies: StreamRecoveryDependencies): void { this.dependencies = dependencies; }

  private syncSubscription = (): void => {
    if (this.store.getSnapshot().length > 0 && !this.subscription) {
      this.subscription = this.dependencies.subscribe(this.onEvent);
    } else if (this.store.getSnapshot().length === 0 && this.subscription) {
      this.subscription.remove();
      this.subscription = undefined;
    }
  };

  private onEvent = (event: StreamTerminationEvent): void => {
    if (!event || !Number.isInteger(event.port) || event.port < 1 || event.port > 65535) return;
    const matches = this.store.getSnapshot().filter(stream => stream.port === event.port);
    if (event.reason === 0) console.info(`[leftcar] close event port=${event.port} matches=${matches.length} generation=${event.generation} owned=${matches[0]?.reservation?.generation}`);
    if (matches.length !== 1) return;
    const active = matches[0]!;
    if (event.generation && active.reservation?.generation &&
        event.generation !== active.reservation.generation) return;
    if (event.reason === 0) {
      console.info(`[leftcar] native window closed session=${active.session} port=${active.port}`);
      // Retire logical ownership before awaiting native/Host teardown. A late
      // restore must clean up its replacement, never publish another window.
      this.store.update(streams => streams.filter(stream => stream !== active));
      const cleanup = active.reservation?.close() ?? this.dependencies.stop(active.session);
      void cleanup.then(() => console.info(`[leftcar] window cleanup complete session=${active.session}`))
        .catch(this.reportError);
      return;
    }
    const candidate = selectRecoverableStream(this.store.getSnapshot(), event, this.store.recoveryInFlight);
    if (!candidate) return;
    if (Date.now() - this.lastRestoreStartedAt >= RESTORE_BACKOFF_RESET_MS) this.restoreRepeats = 0;
    const delay = restoreBackoffDelayMs(this.restoreRepeats);
    // 실패 재시도 타이머가 대기 중이면 새 네이티브 이벤트가 이를 대체해 즉시
    // 시도한다 — RN 타이머는 네이티브 창 포그라운드에서 멈추므로 재발행
    // 이벤트(네이티브 10s 주기)가 유일한 신뢰 가능한 재시도 시계다.
    const pendingRetry = this.pendingRestores.get(event.port);
    if (pendingRetry !== undefined) {
      if (!pendingRetry.failedRetry) return;
      clearTimeout(pendingRetry.timer);
      this.pendingRestores.delete(event.port);
      if (!claimStreamRestore(this.store.recoveryInFlight, candidate.session)) return;
      this.beginRestore(candidate, event);
      return;
    }
    if (delay === 0) {
      if (!claimStreamRestore(this.store.recoveryInFlight, candidate.session)) return;
      this.beginRestore(candidate, event);
      return;
    }
    const timer = setTimeout(() => {
      this.pendingRestores.delete(event.port);
      this.restoreIfStillActive(event);
    }, delay);
    this.pendingRestores.set(event.port, { timer, failedRetry: false });
  };

  private restoreIfStillActive(event: StreamTerminationEvent): void {
    if (this.store.getSnapshot().filter(stream => stream.port === event.port).length !== 1) return;
    const candidate = selectRecoverableStream(this.store.getSnapshot(), event, this.store.recoveryInFlight);
    if (!candidate || !claimStreamRestore(this.store.recoveryInFlight, candidate.session)) return;
    this.beginRestore(candidate, event);
  }

  private beginRestore(active: ActiveStream, event: StreamTerminationEvent): void {
    this.restoreRepeats += 1;
    this.lastRestoreStartedAt = Date.now();
    void this.restore(active, event);
  }

  private reportError = (error: unknown): void => {
    console.warn("[leftcar] window recovery/cleanup failed", error instanceof Error ? error.message : String(error));
    this.store.setRecoveryError(this.dependencies.formatError?.(error) ??
      (error instanceof Error ? error.message : String(error)));
  };

  private async restore(active: ActiveStream, event: StreamTerminationEvent): Promise<void> {
    const dependencies = this.dependencies;
    try {
      const restarted = await dependencies.restore(active);
      this.store.update(previous => replaceRestartedStreamState(
        previous, active.session, restarted, Date.now(),
        session => { void dependencies.stop(session).catch(this.reportError); },
      ));
      this.store.setRecoveryError(null);
    } catch (error) {
      this.reportError(error);
      // 실패한 복원은 여기서 끝내지 않는다 — 호스트 다운 중에는 첫 시도가
      // 반드시 실패하고, 종료 이벤트는 다시 오지 않는다(단발 계측). 같은
      // 창을 백오프(2s..30s 캡)로 계속 시도해 호스트가 돌아오면 복원한다.
      // 창이 닫히면 restoreIfStillActive가 조용히 재시도를 멈춘다.
      this.scheduleRestoreRetry(active, event);
    } finally {
      releaseStreamRestore(this.store.recoveryInFlight, active.session);
    }
  }

  private scheduleRestoreRetry(active: ActiveStream, event: StreamTerminationEvent): void {
    if (this.pendingRestores.has(active.port)) return;
    const delay = restoreBackoffDelayMs(this.restoreRepeats);
    console.info(`[leftcar] window restore retry scheduled port=${active.port} delay=${delay}`);
    const timer = setTimeout(() => {
      this.pendingRestores.delete(active.port);
      if (Date.now() - this.lastRestoreStartedAt >= RESTORE_BACKOFF_RESET_MS) this.restoreRepeats = 0;
      console.info(`[leftcar] window restore retry firing port=${active.port}`);
      this.restoreIfStillActive(event);
    }, delay);
    this.pendingRestores.set(active.port, { timer, failedRetry: true });
  }
}
