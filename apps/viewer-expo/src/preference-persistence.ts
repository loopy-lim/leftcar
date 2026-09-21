export type PreferencePersistenceStatus =
  | "loading"
  | "ready"
  | "saving"
  | "load-error"
  | "save-error";

export interface PreferencePersistenceState<T> {
  value: T;
  status: PreferencePersistenceStatus;
}

interface PreferencePersistenceOptions<T> {
  initialValue: T;
  load(): Promise<T>;
  save(value: T): Promise<void>;
  storageKey: string;
  storageOwner: object;
}

type StateListener<T> = (state: PreferencePersistenceState<T>) => void;

class StorageOperationQueue {
  private tail: Promise<void> = Promise.resolve();

  run<T>(operation: () => Promise<T>): Promise<T> {
    const result = this.tail.then(operation);
    this.tail = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }
}

const storageQueues = new WeakMap<object, Map<string, StorageOperationQueue>>();

function storageOperationQueue(owner: object, key: string): StorageOperationQueue {
  let queuesByKey = storageQueues.get(owner);
  if (!queuesByKey) {
    queuesByKey = new Map();
    storageQueues.set(owner, queuesByKey);
  }
  let queue = queuesByKey.get(key);
  if (!queue) {
    queue = new StorageOperationQueue();
    queuesByKey.set(key, queue);
  }
  return queue;
}

/**
 * Coordinates one persisted preference through hydration, optimistic edits,
 * retries, and ordered writes. Storage calls stay injectable so the same
 * lifecycle is shared by viewer options and clipboard sharing.
 */
export class PreferencePersistenceController<T> {
  private currentState: PreferencePersistenceState<T>;
  private readonly listeners = new Set<StateListener<T>>();
  private revision = 0;
  private hydrated = false;
  private loading = false;
  private writing = false;
  private writePaused = false;
  private disposed = false;
  private readonly storageOperations: StorageOperationQueue;

  constructor(private readonly options: PreferencePersistenceOptions<T>) {
    this.currentState = { value: options.initialValue, status: "loading" };
    this.storageOperations = storageOperationQueue(
      options.storageOwner,
      options.storageKey,
    );
  }

  get state(): PreferencePersistenceState<T> {
    return this.currentState;
  }

  subscribe(listener: StateListener<T>): () => void {
    if (this.disposed) return () => undefined;
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  async load(): Promise<void> {
    if (this.disposed || this.loading) return;
    this.loading = true;
    this.publish({ ...this.currentState, status: "loading" });
    try {
      const loaded = await this.storageOperations.run(() => this.options.load());
      if (this.disposed) return;
      this.loading = false;
      this.hydrated = true;
      this.writePaused = false;
      if (this.revision === 0) {
        this.publish({ value: loaded, status: "ready" });
      } else {
        this.publish({ ...this.currentState, status: "ready" });
        void this.flush();
      }
    } catch {
      if (!this.disposed) {
        this.loading = false;
        this.hydrated = false;
        this.publish({ ...this.currentState, status: "load-error" });
      }
    }
  }

  setValue(value: T): void {
    if (this.disposed) return;
    this.revision += 1;
    this.publish({ ...this.currentState, value });
    if (this.hydrated && !this.writePaused) void this.flush();
  }

  async retry(): Promise<void> {
    if (this.disposed) return;
    if (this.currentState.status === "load-error") {
      await this.load();
      return;
    }
    if (this.currentState.status === "save-error") {
      this.writePaused = false;
      await this.flush();
    }
  }

  dispose(): void {
    this.disposed = true;
    this.listeners.clear();
  }

  private async flush(): Promise<void> {
    if (
      this.disposed ||
      !this.hydrated ||
      this.loading ||
      this.writing ||
      this.writePaused
    ) return;

    let revision = this.revision;
    let value = this.currentState.value;
    this.writing = true;
    this.publish({ value, status: "saving" });
    // Keep the queue slot until every accepted revision is saved. Disposal
    // silences the UI, but successor hydration must wait for this entire drain.
    await this.storageOperations.run(async () => {
      try {
        for (;;) {
          await this.options.save(value);
          if (this.revision === revision) break;
          revision = this.revision;
          value = this.currentState.value;
        }
        this.writing = false;
        this.publish({ ...this.currentState, status: "ready" });
      } catch {
        this.writing = false;
        this.writePaused = true;
        this.publish({ ...this.currentState, status: "save-error" });
      }
    });
  }

  private publish(state: PreferencePersistenceState<T>): void {
    if (this.disposed) return;
    this.currentState = state;
    for (const listener of this.listeners) listener(state);
  }
}
