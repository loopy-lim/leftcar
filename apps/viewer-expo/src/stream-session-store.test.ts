import { expect, it } from "vitest";
import { StreamSessionStore, streamSessionStore } from "./stream-session-store";
import { DecoderReservations } from "./decoder-budget";
import { ReservedStream } from "./reserved-stream";
import type { ActiveStream } from "./catalog-model-types";
import type { StreamLauncher, StreamControlRequest } from "./launch-stream";

it("closing the catalog releases pending work but keeps published native windows and restores their controls", async () => {
  const target = { width: 1920, height: 1080, fps: 60 };
  const pool = new DecoderReservations({ maxInstances: 2 });
  const launcher: StreamLauncher = {
    async prepareStream() {}, async openStream() { return "src-5001"; }, async cancelPreparedStream() {},
  };
  const request: StreamControlRequest = async <T>() => ({} as T);
  const live = new ReservedStream(5001, { split: false, target }, launcher, request, pool);
  const pending = new ReservedStream(5002, { split: false, target }, launcher, request, pool);
  const stream: ActiveStream = { session: 9, port: 5001, reservation: live, sourceIndex: 0,
    sourceName: "Test display", ...target, sourceTarget: target, activeTarget: target,
    fallbackTarget: target, qualityState: "native", captureBackend: "screenCaptureKit",
    contentMode: "interactive", encoderExperiment: "auto", viewerIps: ["192.168.0.2"],
    mediaTransport: "udp", mediaKey: "fixture", startedAt: 1 };
  const store = new StreamSessionStore();
  store.pending.add(live);
  store.pending.add(pending);
  store.update(() => [stream]);
  const unsubscribe = store.subscribe(() => {});
  unsubscribe();
  store.detachCatalog();
  await Promise.resolve();
  expect(live.isOpen).toBe(true);
  expect(pending.isOpen).toBe(false);
  expect(store.getSnapshot()).toEqual([stream]);
  await live.close();
  store.update(() => []);
});

it("catalog remounts reuse the same host state without mixing different hosts", () => {
  expect(streamSessionStore("host-a")).toBe(streamSessionStore("host-a"));
  expect(streamSessionStore("host-a")).not.toBe(streamSessionStore("host-b"));
});
