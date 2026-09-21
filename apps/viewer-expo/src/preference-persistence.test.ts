import { describe, expect, it, vi } from "vitest";
import {
  PreferencePersistenceController,
  type PreferencePersistenceState,
} from "./preference-persistence";

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

function controller(options: {
  load: () => Promise<string>;
  save?: (value: string) => Promise<void>;
  storageKey?: string;
  storageOwner?: object;
}) {
  return new PreferencePersistenceController<string>({
    initialValue: "default",
    load: options.load,
    save: options.save ?? (async () => undefined),
    storageKey: options.storageKey ?? "test.preference",
    storageOwner: options.storageOwner ?? {},
  });
}

describe("PreferencePersistenceController", () => {
  it("keeps defaults in memory after a failed hydration and recovers on retry without writing", async () => {
    const save = vi.fn(async () => undefined);
    const load = vi.fn<() => Promise<string>>()
      .mockRejectedValueOnce(new Error("secure read failed"))
      .mockResolvedValueOnce("stored");
    const subject = controller({ load, save });

    await subject.load();

    expect(subject.state).toEqual({
      value: "default",
      status: "load-error",
    });
    expect(save).not.toHaveBeenCalled();

    await subject.retry();

    expect(subject.state).toEqual({ value: "stored", status: "ready" });
    expect(save).not.toHaveBeenCalled();
  });

  it("uses a missing store's default without writing it back", async () => {
    const save = vi.fn(async () => undefined);
    const subject = controller({ load: async () => "default", save });

    await subject.load();

    expect(subject.state).toEqual({ value: "default", status: "ready" });
    expect(save).not.toHaveBeenCalled();
  });

  it("preserves an edit made during hydration and saves it only after the read succeeds", async () => {
    const hydration = deferred<string>();
    const save = vi.fn(async () => undefined);
    const subject = controller({ load: () => hydration.promise, save });
    const loading = subject.load();

    subject.setValue("edited");
    expect(save).not.toHaveBeenCalled();
    hydration.resolve("stored");
    await loading;
    await vi.waitFor(() => expect(subject.state.status).toBe("ready"));

    expect(subject.state.value).toBe("edited");
    expect(save).toHaveBeenCalledWith("edited");
  });

  it("preserves an edit across failed hydration and a successful retry", async () => {
    const save = vi.fn(async () => undefined);
    const load = vi.fn<() => Promise<string>>()
      .mockRejectedValueOnce(new Error("secure read failed"))
      .mockResolvedValueOnce("stored");
    const subject = controller({ load, save });

    await subject.load();
    subject.setValue("edited");
    await subject.retry();
    await vi.waitFor(() => expect(subject.state.status).toBe("ready"));

    expect(subject.state.value).toBe("edited");
    expect(save).toHaveBeenCalledWith("edited");
  });

  it("reports a rejected write and saves the current value on explicit retry", async () => {
    let stored = "stored";
    const save = vi.fn(async (value: string) => {
      if (save.mock.calls.length === 1) throw new Error("secure write failed");
      stored = value;
    });
    const subject = controller({ load: async () => stored, save });
    await subject.load();

    subject.setValue("edited");
    await vi.waitFor(() => expect(subject.state.status).toBe("save-error"));
    expect(stored).toBe("stored");

    await subject.retry();
    await vi.waitFor(() => expect(subject.state.status).toBe("ready"));

    expect(stored).toBe("edited");
    expect(save).toHaveBeenCalledTimes(2);
  });

  it("serializes rapid edits so an older asynchronous write cannot win", async () => {
    const first = deferred<void>();
    const second = deferred<void>();
    const writes: string[] = [];
    const save = vi.fn((value: string) => {
      writes.push(value);
      return writes.length === 1 ? first.promise : second.promise;
    });
    const subject = controller({ load: async () => "stored", save });
    await subject.load();

    subject.setValue("older");
    subject.setValue("newer");
    await vi.waitFor(() => expect(writes).toEqual(["older"]));

    first.resolve();
    await vi.waitFor(() => expect(writes).toEqual(["older", "newer"]));
    second.resolve();
    await vi.waitFor(() => expect(subject.state.status).toBe("ready"));

    expect(subject.state.value).toBe("newer");
  });

  it("ignores hydration completion after disposal", async () => {
    const hydration = deferred<string>();
    const save = vi.fn(async () => undefined);
    const states: PreferencePersistenceState<string>[] = [];
    const subject = controller({ load: () => hydration.promise, save });
    subject.subscribe((state) => states.push(state));
    const loading = subject.load();
    const notificationsBeforeDispose = states.length;

    subject.dispose();
    hydration.resolve("stored");
    await loading;

    expect(states).toHaveLength(notificationsBeforeDispose);
    expect(save).not.toHaveBeenCalled();
  });

  it("orders a remounted controller behind a disposed lifecycle's in-flight write", async () => {
    const storageOwner = {};
    let stored = "stored";
    const staleWriteGate = deferred<void>();
    const staleWriteFinished = deferred<void>();
    const first = controller({
      storageOwner,
      load: async () => stored,
      save: async (value) => {
        await staleWriteGate.promise;
        stored = value;
        staleWriteFinished.resolve();
      },
    });
    await first.load();
    first.setValue("older");
    await vi.waitFor(() => expect(first.state.status).toBe("saving"));
    first.dispose();

    const remounted = controller({
      storageOwner,
      load: async () => stored,
      save: async (value) => {
        stored = value;
      },
    });
    const remountedLoad = remounted.load();
    remounted.setValue("newer");
    await Promise.resolve();
    await Promise.resolve();

    staleWriteGate.resolve();
    await staleWriteFinished.promise;
    await remountedLoad;
    await vi.waitFor(() => expect(remounted.state.status).toBe("ready"));

    const reloaded = controller({
      storageOwner,
      load: async () => stored,
    });
    await reloaded.load();
    expect(reloaded.state.value).toBe("newer");
  });

  it.each([false, true])("drains the outgoing latest edit before remount hydration (successor edits: %s)", async (successorEdits) => {
    const storageOwner = {};
    let stored = "stored";
    const firstWriteStarted = deferred<void>();
    const firstWriteGate = deferred<void>();
    const writes: string[] = [];
    const states: PreferencePersistenceState<string>[] = [];
    const first = controller({
      storageOwner,
      load: async () => stored,
      save: async (value) => {
        writes.push(value);
        if (writes.length === 1) {
          firstWriteStarted.resolve();
          await firstWriteGate.promise;
        }
        stored = value;
      },
    });
    first.subscribe((state) => states.push(state));
    await first.load();
    first.setValue("older");
    await firstWriteStarted.promise;
    first.setValue("outgoing-latest");
    first.dispose();
    const notificationsBeforeCompletion = states.length;
    first.setValue("ignored-after-dispose");

    const observedReads: string[] = [];
    const remounted = controller({
      storageOwner,
      load: async () => {
        observedReads.push(stored);
        return stored;
      },
      save: async (value) => {
        writes.push(value);
        stored = value;
      },
    });
    const remountedLoad = remounted.load();
    if (successorEdits) remounted.setValue("successor-latest");
    firstWriteGate.resolve();
    await remountedLoad;
    await vi.waitFor(() => expect(remounted.state.status).toBe("ready"));

    expect(observedReads).toEqual(["outgoing-latest"]);
    expect(writes).toEqual(successorEdits
      ? ["older", "outgoing-latest", "successor-latest"]
      : ["older", "outgoing-latest"]);
    expect(states).toHaveLength(notificationsBeforeCompletion);
    const reloaded = controller({ storageOwner, load: async () => stored });
    await reloaded.load();
    expect(reloaded.state.value).toBe(successorEdits ? "successor-latest" : "outgoing-latest");
  });

  it("does not persist a disposed edit after failed hydration", async () => {
    const storageOwner = {};
    const hydration = deferred<string>();
    const save = vi.fn(async () => undefined);
    const states: PreferencePersistenceState<string>[] = [];
    const first = controller({ storageOwner, load: () => hydration.promise, save });
    first.subscribe((state) => states.push(state));
    const loading = first.load();
    first.setValue("unhydrated-edit");
    first.dispose();
    const notificationsBeforeCompletion = states.length;
    hydration.reject(new Error("secure read failed"));
    await loading;
    await first.retry();
    const reloaded = controller({ storageOwner, load: async () => "stored" });
    await reloaded.load();

    expect(reloaded.state.value).toBe("stored");
    expect(save).not.toHaveBeenCalled();
    expect(states).toHaveLength(notificationsBeforeCompletion);
  });

  it.each(["older", "outgoing-latest"])("stops a disposed drain when saving %s fails", async (failedValue) => {
    const storageOwner = {};
    let stored = "stored";
    const firstWriteStarted = deferred<void>();
    const firstWriteGate = deferred<void>();
    const writes: string[] = [];
    const states: PreferencePersistenceState<string>[] = [];
    const first = controller({
      storageOwner,
      load: async () => stored,
      save: async (value) => {
        writes.push(value);
        if (writes.length === 1) {
          firstWriteStarted.resolve();
          await firstWriteGate.promise;
        }
        if (value === failedValue) throw new Error("secure write failed");
        stored = value;
      },
    });
    first.subscribe((state) => states.push(state));
    await first.load();
    first.setValue("older");
    await firstWriteStarted.promise;
    first.setValue("outgoing-latest");
    first.dispose();
    const notificationsBeforeCompletion = states.length;
    const reloaded = controller({ storageOwner, load: async () => stored });
    const reloading = reloaded.load();
    firstWriteGate.resolve();
    await reloading;
    await first.retry();

    expect(writes).toEqual(failedValue === "older" ? ["older"] : ["older", "outgoing-latest"]);
    expect(reloaded.state.value).toBe(failedValue === "older" ? "stored" : "older");
    expect(states).toHaveLength(notificationsBeforeCompletion);
  });

  it("pauses coalesced edits after a failed save until deliberate retry", async () => {
    let stored = "stored";
    const firstWriteStarted = deferred<void>();
    const firstWriteGate = deferred<void>();
    const writes: string[] = [];
    const subject = controller({
      load: async () => stored,
      save: async (value) => {
        writes.push(value);
        if (writes.length === 1) {
          firstWriteStarted.resolve();
          await firstWriteGate.promise;
          throw new Error("secure write failed");
        }
        stored = value;
      },
    });
    await subject.load();
    subject.setValue("older");
    await firstWriteStarted.promise;
    subject.setValue("coalesced");
    firstWriteGate.resolve();
    await vi.waitFor(() => expect(subject.state.status).toBe("save-error"));
    subject.setValue("latest-after-error");
    await Promise.resolve();
    expect(writes).toEqual(["older"]);
    expect(stored).toBe("stored");
    await subject.retry();

    expect(writes).toEqual(["older", "latest-after-error"]);
    expect(stored).toBe("latest-after-error");
    expect(subject.state.status).toBe("ready");
  });

  it("does not block a different key on the same storage owner", async () => {
    const storageOwner = {};
    const blockedWrite = deferred<void>();
    const first = controller({
      storageOwner,
      storageKey: "viewer",
      load: async () => "viewer-stored",
      save: () => blockedWrite.promise,
    });
    await first.load();
    first.setValue("viewer-edited");
    await vi.waitFor(() => expect(first.state.status).toBe("saving"));

    const clipboard = controller({
      storageOwner,
      storageKey: "clipboard",
      load: async () => "clipboard-stored",
    });
    await clipboard.load();

    expect(clipboard.state).toEqual({ value: "clipboard-stored", status: "ready" });
    blockedWrite.resolve();
  });

  it("reloads the latest successfully saved value in a new lifecycle", async () => {
    let stored = "stored";
    const save = async (value: string) => {
      stored = value;
    };
    const first = controller({ load: async () => stored, save });
    await first.load();
    first.setValue("saved");
    await vi.waitFor(() => expect(first.state.status).toBe("ready"));

    const reloaded = controller({ load: async () => stored, save });
    await reloaded.load();

    expect(reloaded.state).toEqual({ value: "saved", status: "ready" });
  });
});
