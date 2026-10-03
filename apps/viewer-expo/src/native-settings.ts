import type { ActiveStream } from "./catalog-model-types";
import type { SessionRequestContext } from "./session";
import { captureRequestContext, isRequestContextCurrent } from "./session";
import { sameStreamLifetime } from "./stream-lifetime-operation";
import { formatErrorMessage } from "./control";
import { LocalizedError } from "./localized-error";

export type NativeSettingsKey =
  | "cursor"
  | "audio"
  | "opus"
  | "balanced"
  | "smooth"
  | "udp";

export interface NativeSettingsTask {
  active: ActiveStream;
  apply: (isCurrent: () => boolean) => Promise<void>;
  commit?: () => void;
}

interface SettingsApplication {
  host: string;
  getSnapshot: () => readonly ActiveStream[];
  tasks: readonly NativeSettingsTask[];
  onApplied?: () => void;
}

interface CapturedSettingsTask extends NativeSettingsTask {
  generation: string | undefined;
}

interface SettingsIntent extends SettingsApplication {
  key: NativeSettingsKey;
  origin: SessionRequestContext | null;
  lifetime: number;
  tasks: readonly CapturedSettingsTask[];
  pending: boolean;
  error: string | null;
}

interface SettingsSnapshot {
  pending: readonly NativeSettingsKey[];
  failures: readonly { key: NativeSettingsKey; error: string }[];
}

/** A retry keeps the initiating desired value, Host selection and window set.
 * Preference persistence is independent: native ACKs confirm only live windows. */
export class NativeSettingsController {
  private readonly intents = new Map<NativeSettingsKey, SettingsIntent>();
  private readonly listeners = new Set<() => void>();
  private active = true;
  private lifetime = 0;
  private snapshot: SettingsSnapshot = { pending: [], failures: [] };

  getSnapshot = (): SettingsSnapshot => this.snapshot;
  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  dispose(): void {
    this.active = false;
    this.lifetime += 1;
    this.listeners.clear();
    this.intents.clear();
  }

  activate(): void {
    if (this.active) return;
    this.active = true;
    this.publish();
  }

  apply(key: NativeSettingsKey, application: SettingsApplication): Promise<void> {
    const intent: SettingsIntent = {
      ...application,
      key,
      origin: captureRequestContext(),
      lifetime: this.lifetime,
      tasks: application.tasks.map(task => ({ ...task, generation: task.active.reservation?.generation })),
      pending: false,
      error: null,
    };
    this.intents.set(key, intent);
    return this.run(key, intent);
  }

  retry = (key: NativeSettingsKey): Promise<void> => {
    const intent = this.intents.get(key);
    return intent && !intent.pending ? this.run(key, intent) : Promise.resolve();
  };

  private sameSelection(intent: SettingsIntent): boolean {
    const selected = captureRequestContext();
    return (
      this.active &&
      intent.lifetime === this.lifetime &&
      !!intent.origin &&
      !!selected &&
      isRequestContextCurrent(selected) &&
      selected.selectionGeneration === intent.origin.selectionGeneration &&
      `${selected.target.host}:${selected.target.port}` === intent.host
    );
  }

  private currentTask(intent: SettingsIntent, task: CapturedSettingsTask): boolean {
    return (
      this.sameSelection(intent) &&
      // UDP owns a bounded restart of this logical lifetime. Ordinary setting
      // intents must never cross a native generation replacement during it.
      (intent.key === "udp" || task.active.reservation?.generation === task.generation) &&
      intent.getSnapshot().some(current => sameStreamLifetime(current, task.active))
    );
  }

  private publish(): void {
    if (!this.active) return;
    this.snapshot = {
      pending: [...this.intents].filter(([, intent]) => intent.pending).map(([key]) => key),
      failures: [...this.intents].flatMap(([key, intent]) =>
        intent.error ? [{ key, error: intent.error }] : [],
      ),
    };
    for (const listener of this.listeners) listener();
  }

  private run(key: NativeSettingsKey, intent: SettingsIntent): Promise<void> {
    const tasks = intent.tasks.filter(task => this.currentTask(intent, task));
    if (tasks.length === 0) {
      this.intents.delete(key);
      this.publish();
      return Promise.resolve();
    }
    intent.pending = true;
    intent.error = null;
    this.publish();
    const failed: CapturedSettingsTask[] = [];
    const errors: string[] = [];
    const runAt = (index: number): Promise<void> => {
      const task = tasks[index];
      if (!task || this.intents.get(key) !== intent || !this.sameSelection(intent))
        return Promise.resolve();
      const isCurrent = () => this.currentTask(intent, task);
      // The native call is issued only after checking the original window again.
      return Promise.resolve().then(() => {
        if (!isCurrent()) return;
        return task.apply(isCurrent).then(() => {
          // A newer issued intent alone must not suppress an older successful
          // ACK. The caller retains its per-window latest-success ordering.
          if (isCurrent()) task.commit?.();
        });
      }).catch(cause => {
        if (isCurrent()) {
          failed.push(task);
          errors.push(cause instanceof Error && !(cause instanceof LocalizedError)
            ? cause.message : formatErrorMessage(cause));
        }
      }).then(() => runAt(index + 1));
    };
    return runAt(0).then(() => {
      if (this.intents.get(key) !== intent) return;
      if (!this.sameSelection(intent)) {
        this.intents.delete(key);
        this.publish();
        return;
      }
      intent.pending = false;
      intent.tasks = failed;
      intent.error = errors[0] ?? null;
      if (failed.length === 0) {
        this.intents.delete(key);
        intent.onApplied?.();
      }
      this.publish();
    });
  }
}
