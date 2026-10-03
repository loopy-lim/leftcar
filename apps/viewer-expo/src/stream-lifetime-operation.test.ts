import { beforeEach, expect, it, vi } from "vitest";
import type { ActiveStream } from "./catalog-model-types";
import type { SessionRequestContext } from "./session";

vi.mock("react-native-tcp-socket", () => ({
  default: { createConnection: vi.fn() },
}));
vi.mock("./session", () => ({
  bindRequestContext: vi.fn(),
  captureRequestContext: vi.fn(),
  reconnectHost: vi.fn(),
  isRequestContextCurrent: vi.fn(),
}));

import { captureRequestContext, isRequestContextCurrent } from "./session";
import { runStreamLifetimeOperation } from "./stream-lifetime-operation";

const target = { width: 1920, height: 1080, fps: 60 };
function stream(overrides: Partial<ActiveStream> = {}): ActiveStream {
  return {
    session: 8,
    port: 5001,
    startedAt: 10,
    sourceIndex: 0,
    sourceName: "Main",
    ...target,
    sourceTarget: target,
    activeTarget: target,
    fallbackTarget: null,
    qualityState: "native",
    captureBackend: "screenCaptureKit",
    contentMode: "interactive",
    encoderExperiment: "auto",
    mediaTransport: "udp",
    viewerIps: ["192.168.0.2"],
    mediaKey: "fixture",
    ...overrides,
  };
}
function context(host: string, generation: number): SessionRequestContext {
  const client = {
    async request<T>() {
      return { accepted: true } as T;
    },
    close: vi.fn(),
    whenClosed: vi.fn(),
    hostKey: null,
  };
  vi.spyOn(client, "request");
  return {
    client,
    target: { host, port: 7777 },
    selectionGeneration: generation,
    identity: null,
    credential: null,
  };
}
let selected: SessionRequestContext;
beforeEach(() => {
  selected = context("host-a", 1);
  vi.mocked(captureRequestContext).mockImplementation(() => selected);
  vi.mocked(isRequestContextCurrent).mockImplementation(
    (origin) => origin === selected,
  );
});

it("an old Host's Stop callback never sends its session to the newly selected Host", async () => {
  const active = stream();
  selected = context("host-b", 2);
  const commit = vi.fn();
  await expect(
    runStreamLifetimeOperation({
      host: "host-a:7777",
      active,
      getSnapshot: () => [active],
      work: (request) => request("stopStream", { session: active.session }),
      commit,
    }),
  ).rejects.toThrow("selection changed");
  expect(selected.client.request).not.toHaveBeenCalled();
  expect(commit).not.toHaveBeenCalled();
});

it("a Stop reply cannot remove a new window reusing the same Host session id", async () => {
  const active = stream();
  let snapshot = [active];
  let finish!: () => void;
  const stopped = new Promise<void>((resolve) => {
    finish = resolve;
  });
  vi.mocked(selected.client.request).mockImplementation(async <T>() => {
    await stopped;
    return {} as T;
  });
  const commit = vi.fn(() => {
    snapshot = [];
  });
  const pending = runStreamLifetimeOperation({
    host: "host-a:7777",
    active,
    getSnapshot: () => snapshot,
    work: (request) => request("stopStream", { session: active.session }),
    commit,
  });
  const replacement = stream({ port: 5003, startedAt: 20 });
  snapshot = [replacement];
  finish();
  expect(await pending).toBe(false);
  expect(snapshot).toEqual([replacement]);
  expect(commit).not.toHaveBeenCalled();
});

it("a resize pins its Host selection before awaiting display metrics", async () => {
  const active = stream();
  let metricsReady!: () => void;
  const metrics = new Promise<void>((resolve) => {
    metricsReady = resolve;
  });
  const commit = vi.fn();
  const pending = runStreamLifetimeOperation({
    host: "host-a:7777",
    active,
    getSnapshot: () => [active],
    work: async (request) => {
      await metrics;
      return request("reconfigureStream", { session: active.session });
    },
    commit,
  });
  const next = context("host-a", 2);
  selected = next;
  metricsReady();
  await expect(pending).rejects.toThrow("selection changed");
  expect(next.client.request).not.toHaveBeenCalled();
  expect(commit).not.toHaveBeenCalled();
});

it("a resize reply preserves a replacement and commits against the current settings of its own window", async () => {
  const active = stream();
  let snapshot = [active];
  let finish!: () => void;
  const resized = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const commit = vi.fn((_result: unknown, current: ActiveStream) => {
    snapshot = [{ ...current, width: 2560 }];
  });
  const pending = runStreamLifetimeOperation({
    host: "host-a:7777",
    active,
    getSnapshot: () => snapshot,
    work: async () => {
      await resized;
      return {};
    },
    commit,
  });
  snapshot = [{ ...active, localAudio: true }];
  finish();
  expect(await pending).toBe(true);
  expect(snapshot[0]).toMatchObject({ localAudio: true, width: 2560 });
});

it("a retired window cannot send a delayed resize command after its native preparation", async () => {
  const active = stream();
  let snapshot = [active];
  let prepared!: () => void;
  const preparation = new Promise<void>((resolve) => {
    prepared = resolve;
  });
  const commit = vi.fn();
  const pending = runStreamLifetimeOperation({
    host: "host-a:7777",
    active,
    getSnapshot: () => snapshot,
    work: async (request) => {
      await preparation;
      return request("reconfigureStream", { session: active.session });
    },
    commit,
  });
  snapshot = [stream({ startedAt: 99 })];
  prepared();
  await expect(pending).rejects.toThrow("window changed");
  expect(selected.client.request).not.toHaveBeenCalled();
  expect(commit).not.toHaveBeenCalled();
});

it("a Host switch before an acknowledged resize leaves its replacement untouched", async () => {
  const active = stream();
  let finish!: () => void;
  const reply = new Promise<void>((resolve) => {
    finish = resolve;
  });
  vi.mocked(selected.client.request).mockImplementation(async <T>() => {
    await reply;
    return {} as T;
  });
  const commit = vi.fn();
  const pending = runStreamLifetimeOperation({
    host: "host-a:7777",
    active,
    getSnapshot: () => [active],
    work: (request) =>
      request("reconfigureStream", { session: active.session }),
    commit,
  });
  selected = context("host-b", 2);
  finish();
  expect(await pending).toBe(false);
  expect(commit).not.toHaveBeenCalled();
  expect(selected.client.request).not.toHaveBeenCalled();
});
