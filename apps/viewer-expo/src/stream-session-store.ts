import type { ActiveStream } from "./catalog-model-types";
import type { ReservedStream } from "./reserved-stream";
import { StreamRecoveryController, type StreamRecoveryDependencies } from "./stream-recovery";

/** Native windows outlive the catalog surface. Keep their ownership in the
 * application runtime so closing/reopening MainActivity cannot abandon them. */
export class StreamSessionStore {
  readonly recoveryInFlight = new Set<number>();
  private recovery?: StreamRecoveryController;
  private recoveryError: string | null = null;
  readonly pending = new Set<ReservedStream>();
  readonly streamsRef: { current: ActiveStream[] } = { current: [] };
  private readonly listeners = new Set<() => void>();

  getSnapshot = (): ActiveStream[] => this.streamsRef.current;
  getRecoveryError = (): string | null => this.recoveryError;

  setRecoveryError(error: string | null): void {
    this.recoveryError = error;
    for (const listener of this.listeners) listener();
  }

  configureRecovery(dependencies: StreamRecoveryDependencies): void {
    if (this.recovery) this.recovery.configure(dependencies);
    else this.recovery = new StreamRecoveryController(this, dependencies);
  }

  recover(active: ActiveStream): void {
    this.recovery?.recover(active);
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  update = (transition: (previous: ActiveStream[]) => ActiveStream[]): void => {
    this.streamsRef.current = transition(this.streamsRef.current);
    for (const stream of this.streamsRef.current) {
      if (stream.reservation) this.pending.delete(stream.reservation);
    }
    for (const listener of this.listeners) listener();
  };

  detachCatalog(): void {
    // Unpublished asynchronous launches still need cancellation. Published
    // windows are owned by ActiveStream and close only through explicit Stop
    // or native/host terminal events.
    for (const reservation of this.pending) reservation.abandon();
    this.pending.clear();
  }
}

const stores = new Map<string, StreamSessionStore>();
export function streamSessionStore(host: string): StreamSessionStore {
  let store = stores.get(host);
  if (!store) {
    store = new StreamSessionStore();
    stores.set(host, store);
  }
  return store;
}
