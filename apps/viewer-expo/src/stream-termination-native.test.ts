import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

const boundary = vi.hoisted(() => ({
  events: [] as string[],
  listener: undefined as ((event: unknown) => void) | undefined,
  moduleAvailable: true,
}));
vi.mock("react-native", () => ({
  DeviceEventEmitter: {
    addListener: (_name: string, listener: (event: unknown) => void) => {
      boundary.events.push("subscribe");
      boundary.listener = listener;
      return { remove: () => { boundary.events.push("unsubscribe"); boundary.listener = undefined; } };
    },
  },
  NativeModules: {
    get StreamLauncher() {
      return boundary.moduleAvailable ? {
        addListener(name: string) {
          boundary.events.push(`native-add:${name}`);
          boundary.listener?.({ port: 5007, reason: 5, generation: "pending-owner" });
        },
        removeListeners(count: number) { boundary.events.push(`native-remove:${count}`); },
      } : undefined;
    },
  },
}));

beforeEach(() => {
  boundary.events.length = 0;
  boundary.moduleAvailable = true;
  boundary.listener = undefined;
});

let nativeTermination: typeof import("./stream-termination.native");

beforeAll(async () => {
  nativeTermination = await import("./stream-termination.native");
});

describe("native stream termination entrypoint", () => {
  it("exports the viewer-close classifier selected by Android bundling", () => {
    expect(typeof nativeTermination.classifyHostTermination).toBe("function");
    expect(nativeTermination.classifyHostTermination("viewer closed stream")).toBe(
      "viewerClosed",
    );
  });
});


it("registers JS before native pending termination flush and removes both subscriptions once", () => {
  const listener = vi.fn();
  const subscription = nativeTermination.subscribeStreamTermination(listener);
  expect(boundary.events).toEqual(["subscribe", "native-add:leftcarStreamTerminated"]);
  expect(listener).toHaveBeenCalledExactlyOnceWith({ port: 5007, reason: 5, generation: "pending-owner" });
  subscription.remove();
  subscription.remove();
  expect(boundary.events).toEqual(["subscribe", "native-add:leftcarStreamTerminated", "unsubscribe", "native-remove:1"]);
});

it("keeps JS delivery and cleanup usable when the native module is absent", () => {
  boundary.moduleAvailable = false;
  const listener = vi.fn();
  const subscription = nativeTermination.subscribeStreamTermination(listener);
  boundary.listener?.({ port: 5007, reason: 1 });
  expect(listener).toHaveBeenCalledOnce();
  subscription.remove();
  expect(boundary.events).toEqual(["subscribe", "unsubscribe"]);
});
