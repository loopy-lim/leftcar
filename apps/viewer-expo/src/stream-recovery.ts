import type { ActiveStream, RestoredStream } from "./catalog-model-types";
import type { StreamSessionStore } from "./stream-session-store";
import type { StreamTerminationEvent, StreamTerminationSubscription } from "./stream-termination-policy";
import { claimStreamRestore, releaseStreamRestore, selectRecoverableStream } from "./stream-termination-policy";
import { sameStreamLifetime, isStreamOperationCancelled } from "./stream-lifetime";
import { AmbiguousControlError, ControlRequestError } from "./control-error";
import { LocalizedError } from "./localized-error";

export interface StreamRecoveryDependencies {
  subscribe(listener: (event: StreamTerminationEvent) => void): StreamTerminationSubscription;
  restore(active: ActiveStream): Promise<RestoredStream>;
  stop(session: number): Promise<void>;
  formatError?(error: unknown): string;
  /** Capture before a restore or retry waits; new selections retire it. */
  captureOwner?(): () => boolean;
}

const RESTORE_BACKOFF_BASE_MS = 2_000;
const RESTORE_BACKOFF_MAX_MS = 30_000;
const MAX_RESTORE_ATTEMPTS = 5;

export function restoreBackoffDelayMs(repeatIndex: number): number {
  if (repeatIndex <= 0) return 0;
  return Math.min(RESTORE_BACKOFF_BASE_MS * 2 ** (repeatIndex - 1), RESTORE_BACKOFF_MAX_MS);
}

interface RecoveryEpisode {
  active: ActiveStream;
  ownsSelection: () => boolean;
  attempts: number;
  generation?: string;
  retired: boolean;
}
interface PendingRestore {
  timer: ReturnType<typeof setTimeout>;
  episode: RecoveryEpisode;
  failedRetry: boolean;
}

/** A native window owns a bounded recovery episode across its own recreates.
 * Neither a delayed native notification nor elapsed JS time proves renderer
 * progress. Only an explicitly replaced lifetime starts a fresh budget. */
export class StreamRecoveryController {
  private subscription?: StreamTerminationSubscription;
  private readonly episodes = new Map<number, RecoveryEpisode>();
  /** A fresh native event can replace a failed retry timer when RN is paused;
   * a successful recreate still waits out its backoff. Both spend one budget. */
  private readonly pendingRestores = new Map<number, PendingRestore>();

  constructor(private readonly store: StreamSessionStore, private dependencies: StreamRecoveryDependencies) {
    store.subscribe(this.syncSubscription);
    this.syncSubscription();
  }

  configure(dependencies: StreamRecoveryDependencies): void { this.dependencies = dependencies; }

  recover(active: ActiveStream): void {
    if (!this.store.getSnapshot().some(stream => sameStreamLifetime(stream, active))) return;
    this.onEvent({ port: active.port, reason: 1, generation: active.reservation?.generation });
  }

  private isCurrent(active: ActiveStream, ownsSelection: () => boolean): boolean {
    return ownsSelection() && this.store.getSnapshot().some(stream => sameStreamLifetime(stream, active));
  }

  private clearPending(port: number): void {
    const pending = this.pendingRestores.get(port);
    if (pending) clearTimeout(pending.timer);
    this.pendingRestores.delete(port);
  }

  private syncSubscription = (): void => {
    for (const [port, episode] of this.episodes) {
      if (!this.isCurrent(episode.active, episode.ownsSelection) ||
          (episode.generation !== episode.active.reservation?.generation &&
            !this.store.recoveryInFlight.has(episode.active.session))) {
        this.clearPending(port);
        this.episodes.delete(port);
      }
    }
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
    if (matches.length !== 1) return;
    const active = matches[0]!;
    if (event.generation && active.reservation?.generation &&
        event.generation !== active.reservation.generation) return;
    if (event.reason === 0) {
      // Retire logical ownership before teardown. Late restores may clean up
      // their own replacement but cannot publish another native window.
      this.store.update(streams => streams.filter(stream => stream !== active));
      const cleanup = active.reservation?.close() ?? this.dependencies.stop(active.session);
      void cleanup.catch(this.reportError);
      return;
    }
    const candidate = selectRecoverableStream(this.store.getSnapshot(), event, this.store.recoveryInFlight);
    if (!candidate) return;
    let episode = this.episodes.get(event.port);
    if (episode && (!this.isCurrent(episode.active, episode.ownsSelection) ||
          episode.generation !== episode.active.reservation?.generation)) {
      this.clearPending(event.port);
      this.episodes.delete(event.port);
      episode = undefined;
    }
    if (!episode) {
      const ownsSelection = this.dependencies.captureOwner?.() ?? (() => true);
      if (!this.isCurrent(candidate, ownsSelection)) return;
      episode = { active: candidate, ownsSelection, attempts: 0,
        generation: candidate.reservation?.generation, retired: false };
      this.episodes.set(event.port, episode);
    }
    if (episode.retired) return;
    const pending = this.pendingRestores.get(event.port);
    if (pending) {
      if (!pending.failedRetry) return;
      this.clearPending(event.port);
      this.beginRestore(episode, event);
      return;
    }
    const delay = restoreBackoffDelayMs(episode.attempts);
    if (delay === 0) this.beginRestore(episode, event);
    else this.scheduleRestore(episode, event, delay, false);
  };

  private beginRestore(episode: RecoveryEpisode, event: StreamTerminationEvent): void {
    if (episode.retired || !this.isCurrent(episode.active, episode.ownsSelection) ||
        episode.generation !== episode.active.reservation?.generation) return;
    if (episode.attempts >= MAX_RESTORE_ATTEMPTS) {
      this.retire(episode, new LocalizedError("errRecoveryExhausted"));
      return;
    }
    const candidate = selectRecoverableStream(this.store.getSnapshot(), event, this.store.recoveryInFlight);
    if (!candidate || !sameStreamLifetime(candidate, episode.active) ||
        !claimStreamRestore(this.store.recoveryInFlight, candidate.session)) return;
    episode.attempts++;
    void this.restore(candidate, event, episode);
  }

  private retire(episode: RecoveryEpisode, error?: unknown): void {
    episode.retired = true;
    this.clearPending(episode.active.port);
    if (error !== undefined) this.reportError(error);
  }

  private reportError = (error: unknown): void => {
    if (isStreamOperationCancelled(error)) return;
    console.warn("[leftcar] window recovery/cleanup failed", error instanceof Error ? error.message : String(error));
    this.store.setRecoveryError(this.dependencies.formatError?.(error) ??
      (error instanceof LocalizedError ? error.format() : error instanceof Error ? error.message : String(error)));
  };

  private async restore(active: ActiveStream, event: StreamTerminationEvent, episode: RecoveryEpisode): Promise<void> {
    const dependencies = this.dependencies;
    try {
      const restarted = await dependencies.restore(active);
      if (!this.isCurrent(active, episode.ownsSelection)) {
        // A recycled session may already belong to a successor. Stop only
        // an unowned replacement through the initiating Host's request.
        if (!this.store.getSnapshot().some(stream => stream.session === restarted.session)) {
          void dependencies.stop(restarted.session).catch(error => {
            if (episode.ownsSelection()) this.reportError(error);
          });
        }
        return;
      }
      // Keep this episode's budget through its own recreate. A successful ACK
      // is not evidence that the new renderer delivered a healthy frame.
      const current = this.store.getSnapshot().find(stream => sameStreamLifetime(stream, active))!;
      const restored = { ...current, ...restarted, startedAt: Date.now() };
      episode.active = restored;
      episode.generation = restored.reservation?.generation;
      this.store.update(previous => previous.map(stream => sameStreamLifetime(stream, active) ? restored : stream));
      this.store.setRecoveryError(null);
    } catch (error) {
      if (!this.isCurrent(active, episode.ownsSelection)) return;
      // Failed owned work may leave its reservation generation unknown.
      // Keep retirement and retry budget attached to that resulting state.
      episode.generation = active.reservation?.generation;
      if (isStreamOperationCancelled(error)) { this.retire(episode); return; }
      this.reportError(error);
      if (error instanceof AmbiguousControlError ||
          (error instanceof ControlRequestError && error.kind !== "transport" && error.kind !== "timeout")) {
        this.retire(episode);
        return;
      }
      if (episode.attempts >= MAX_RESTORE_ATTEMPTS) this.retire(episode, new LocalizedError("errRecoveryExhausted"));
      else this.scheduleRestore(episode, event, restoreBackoffDelayMs(episode.attempts), true);
    } finally {
      releaseStreamRestore(this.store.recoveryInFlight, active.session);
    }
  }

  private scheduleRestore(episode: RecoveryEpisode, event: StreamTerminationEvent, delay: number, failedRetry: boolean): void {
    const active = episode.active;
    const generation = active.reservation?.generation;
    if (episode.retired || !this.isCurrent(active, episode.ownsSelection) || this.pendingRestores.has(active.port)) return;
    const pending: PendingRestore = { episode, failedRetry, timer: setTimeout(() => {
      if (this.pendingRestores.get(active.port) !== pending) return;
      this.pendingRestores.delete(active.port);
      if (this.isCurrent(active, episode.ownsSelection) && sameStreamLifetime(active, episode.active) &&
          active.reservation?.generation === generation) {
        this.beginRestore(episode, event);
      }
    }, delay) };
    this.pendingRestores.set(active.port, pending);
  }
}
