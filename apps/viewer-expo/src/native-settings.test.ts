import { beforeEach, expect, it, vi } from "vitest";
import type { ActiveStream } from "./catalog-model-types";
import type { SessionRequestContext } from "./session";
import type { ReservedStream } from "./reserved-stream";

vi.mock("react-native-tcp-socket", () => ({
  default: { createConnection: vi.fn() },
}));
vi.mock("./session", () => ({
  captureRequestContext: vi.fn(),
  isRequestContextCurrent: vi.fn(),
}));
vi.mock("./stream-lifetime-operation", () => ({
  sameStreamLifetime: (left: ActiveStream, right: ActiveStream) =>
    left.session === right.session &&
    left.port === right.port &&
    left.startedAt === right.startedAt &&
    left.reservation === right.reservation,
}));
import { captureRequestContext, isRequestContextCurrent } from "./session";
import { NativeSettingsController } from "./native-settings";

const target = { width: 1920, height: 1080, fps: 60 };
function stream(overrides: Partial<ActiveStream> = {}): ActiveStream {
  return {
    session: 8, port: 5001, startedAt: 10, sourceIndex: 0, sourceName: "Main",
    ...target, sourceTarget: target, activeTarget: target, fallbackTarget: null,
    qualityState: "native", captureBackend: "screenCaptureKit",
    contentMode: "interactive", encoderExperiment: "auto", mediaTransport: "udp",
    viewerIps: ["192.168.0.2"], mediaKey: "fixture", ...overrides,
  };
}
let selected: SessionRequestContext;
beforeEach(() => {
  selected = {
    client: { request: vi.fn(), close: vi.fn(), whenClosed: vi.fn(), hostKey: null },
    target: { host: "host-a", port: 7777 }, selectionGeneration: 1,
    identity: null, credential: null,
  };
  vi.mocked(captureRequestContext).mockImplementation(() => selected);
  vi.mocked(isRequestContextCurrent).mockImplementation(context => context === selected);
});

it("a failed native setting exposes local failure and retries its original desired value", async () => {
  const controller = new NativeSettingsController();
  const active = stream();
  let desired = false;
  const captured = desired;
  const values: boolean[] = [];
  const apply = vi.fn().mockImplementation(() => {
    values.push(captured);
    return values.length === 1 ? Promise.reject(new Error("Device audio update failed")) : Promise.resolve();
  });
  const commit = vi.fn();
  await controller.apply("audio", { host: "host-a:7777", getSnapshot: () => [active], tasks: [{ active, apply, commit }] });
  expect(controller.getSnapshot().failures).toEqual([{ key: "audio", error: "Device audio update failed" }]);
  expect(commit).not.toHaveBeenCalled();
  desired = true;
  await controller.retry("audio");
  expect(values).toEqual([false, false]);
  expect(desired).toBe(true);
  expect(commit).toHaveBeenCalledTimes(1);
  expect(controller.getSnapshot().failures).toEqual([]);
});

it("retry only revisits failed original targets, preserving completed windows", async () => {
  const controller = new NativeSettingsController();
  const first = stream();
  const second = stream({ session: 9, port: 5003 });
  const applyFirst = vi.fn().mockResolvedValue(undefined);
  const applySecond = vi.fn().mockRejectedValueOnce(new Error("retry me")).mockResolvedValue(undefined);
  await controller.apply("smooth", { host: "host-a:7777", getSnapshot: () => [first, second], tasks: [{ active: first, apply: applyFirst }, { active: second, apply: applySecond }] });
  await controller.retry("smooth");
  expect(applyFirst).toHaveBeenCalledTimes(1);
  expect(applySecond).toHaveBeenCalledTimes(2);
});

it("a failed retry cannot apply to a newly selected Host, even at the same address", async () => {
  const controller = new NativeSettingsController();
  const active = stream();
  const apply = vi.fn().mockRejectedValue(new Error("failed"));
  await controller.apply("cursor", { host: "host-a:7777", getSnapshot: () => [active], tasks: [{ active, apply }] });
  selected = { ...selected, selectionGeneration: 2 };
  await controller.retry("cursor");
  expect(apply).toHaveBeenCalledTimes(1);
  expect(controller.getSnapshot().failures).toEqual([]);
});

it("retry never sends the desired value to a replacement native window", async () => {
  const controller = new NativeSettingsController();
  const active = stream();
  let snapshot = [active];
  const apply = vi.fn().mockRejectedValue(new Error("failed"));
  await controller.apply("balanced", { host: "host-a:7777", getSnapshot: () => snapshot, tasks: [{ active, apply }] });
  snapshot = [stream({ startedAt: 20 })];
  await controller.retry("balanced");
  expect(apply).toHaveBeenCalledTimes(1);
  expect(controller.getSnapshot().failures).toEqual([]);
});

it("pending status is explicit and an unmounted settings owner cannot publish a late failure", async () => {
  const controller = new NativeSettingsController();
  const active = stream();
  let fail!: (error: Error) => void;
  const commit = vi.fn();
  const pending = controller.apply("opus", { host: "host-a:7777", getSnapshot: () => [active], tasks: [{ active, commit, apply: () => new Promise((_, reject) => { fail = reject; }) }] });
  await vi.waitFor(() => expect(fail).toBeDefined());
  expect(controller.getSnapshot().pending).toEqual(["opus"]);
  const listener = vi.fn();
  controller.subscribe(listener);
  controller.dispose();
  fail(new Error("late failure"));
  await pending;
  expect(listener).not.toHaveBeenCalled();
  expect(commit).not.toHaveBeenCalled();
});

it("a previous successful native ACK still commits when a newer desired setting fails", async () => {
  const controller = new NativeSettingsController();
  const active = stream();
  let finish!: () => void;
  const firstCommit = vi.fn();
  const first = controller.apply("audio", { host: "host-a:7777", getSnapshot: () => [active], tasks: [{ active, commit: firstCommit, apply: () => new Promise<void>(resolve => { finish = resolve; }) }] });
  await vi.waitFor(() => expect(finish).toBeDefined());
  await controller.apply("audio", { host: "host-a:7777", getSnapshot: () => [active], tasks: [{ active, apply: () => Promise.reject(new Error("new value failed")) }] });
  finish();
  await first;
  expect(firstCommit).toHaveBeenCalledTimes(1);
  expect(controller.getSnapshot().failures).toEqual([{ key: "audio", error: "new value failed" }]);
});

it("a same-address selection change during pending work retires progress without a late ACK", async () => {
  const controller = new NativeSettingsController();
  const active = stream();
  let finish!: () => void;
  const commit = vi.fn();
  const pending = controller.apply("audio", { host: "host-a:7777", getSnapshot: () => [active], tasks: [{ active, commit, apply: () => new Promise<void>(resolve => { finish = resolve; }) }] });
  await vi.waitFor(() => expect(finish).toBeDefined());
  selected = { ...selected, selectionGeneration: 2 };
  finish();
  await pending;
  expect(commit).not.toHaveBeenCalled();
  expect(controller.getSnapshot()).toEqual({ pending: [], failures: [] });
});

it("effect cleanup and reactivation admits new work but never revives old owner ACKs", async () => {
  const controller = new NativeSettingsController();
  const active = stream();
  let finish!: () => void;
  const oldCommit = vi.fn();
  const pending = controller.apply("audio", { host: "host-a:7777", getSnapshot: () => [active], tasks: [{ active, commit: oldCommit, apply: () => new Promise<void>(resolve => { finish = resolve; }) }] });
  await vi.waitFor(() => expect(finish).toBeDefined());
  controller.dispose();
  controller.activate();
  const newCommit = vi.fn();
  await controller.apply("audio", { host: "host-a:7777", getSnapshot: () => [active], tasks: [{ active, commit: newCommit, apply: () => Promise.resolve() }] });
  finish();
  await pending;
  expect(oldCommit).not.toHaveBeenCalled();
  expect(newCommit).toHaveBeenCalledTimes(1);
  expect(controller.getSnapshot()).toEqual({ pending: [], failures: [] });
});

it("a retry cannot reach a replaced native generation while its logical stream is still published", async () => {
  const controller = new NativeSettingsController();
  const reservation = { generation: "41" };
  const active = stream({ reservation: reservation as ReservedStream });
  const apply = vi.fn().mockRejectedValue(new Error("failed"));
  await controller.apply("audio", { host: "host-a:7777", getSnapshot: () => [active], tasks: [{ active, apply }] });
  reservation.generation = "42";
  await controller.retry("audio");
  expect(apply).toHaveBeenCalledTimes(1);
  expect(controller.getSnapshot()).toEqual({ pending: [], failures: [] });
});

it("a UDP reconnect can retry its owned logical window after its own native generation changes", async () => {
  const controller = new NativeSettingsController();
  const reservation = { generation: "41" };
  const active = stream({ reservation: reservation as ReservedStream });
  const apply = vi.fn().mockImplementationOnce(() => {
    reservation.generation = "42";
    return Promise.reject(new Error("reconnect failed"));
  }).mockResolvedValue(undefined);
  await controller.apply("udp", { host: "host-a:7777", getSnapshot: () => [active], tasks: [{ active, apply }] });
  expect(controller.getSnapshot().failures).toHaveLength(1);
  await controller.retry("udp");
  expect(apply).toHaveBeenCalledTimes(2);
});
