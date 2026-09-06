import { describe, expect, it } from "vitest";
import type { ActiveStream } from "./catalog-model-types";
import {
  claimStreamRestore,
  classifyHostTermination,
  releaseStreamRestore,
  selectRecoverableStream,
  subscribeStreamTermination,
} from "./stream-termination";

function active(session: number, port: number): ActiveStream {
  return {
    session,
    port,
    sourceIndex: session,
    sourceName: `Display ${session}`,
    width: 3840,
    height: 2160,
    fps: 60,
    sourceTarget: { width: 3840, height: 2160, fps: 60 },
    activeTarget: { width: 3840, height: 2160, fps: 60 },
    fallbackTarget: { width: 2560, height: 1440, fps: 60 },
    qualityState: "native",
    captureBackend: "screenCaptureKit",
    contentMode: "video",
    encoderExperiment: "auto",
    mediaTransport: "udp",
    viewerIps: ["192.168.0.42"],
    startedAt: 1,
  };
}

describe("stream termination recovery selection", () => {
  const streams = [active(11, 5001), active(22, 5002)];

  it("ignores malformed payloads and non-local termination reasons", () => {
    for (const event of [
      null,
      {},
      { port: "5001", reason: 4 },
      { port: 5001.5, reason: 4 },
      { port: Number.POSITIVE_INFINITY, reason: 4 },
      { port: 5001, reason: 1 },
      { port: 5001, reason: 3 },
      { port: 5001, reason: 6 },
      { port: 5001, reason: "4" },
    ]) {
      expect(
        selectRecoverableStream(streams, event, new Set()),
      ).toBeNull();
    }
  });

  it("ignores a valid local termination when no active stream has its port", () => {
    expect(
      selectRecoverableStream(
        streams,
        { port: 5999, reason: 4 },
        new Set(),
      ),
    ).toBeNull();
  });

  it("selects the single active stream whose port terminated locally", () => {
    expect(
      selectRecoverableStream(
        streams,
        { port: 5002, reason: 5 },
        new Set(),
      ),
    ).toEqual(streams[1]);
  });

  it("rejects a duplicate termination while that session is reconnecting", () => {
    expect(
      selectRecoverableStream(
        streams,
        { port: 5001, reason: 4 },
        new Set([11]),
      ),
    ).toBeNull();
  });

  it("allows independent recovery for another port while one session is in flight", () => {
    expect(
      selectRecoverableStream(
        streams,
        { port: 5002, reason: 4 },
        new Set([11]),
      ),
    ).toEqual(streams[1]);
  });
});

describe("host termination classification", () => {
  it("classifies the exact viewer close as a silent local completion", () => {
    expect(classifyHostTermination("viewer closed stream")).toBe("viewerClosed");
    expect(classifyHostTermination(" viewer closed stream ")).toBe("viewerClosed");
  });

  it("does not hide unexpected errors that merely mention a viewer", () => {
    expect(classifyHostTermination("viewer closed stream unexpectedly: decoder failed")).toBeNull();
  });
});

describe("controller recovery policy", () => {
  const streams = [active(11, 5001), active(22, 5002)];

  it("allows an explicit retry after a failed native stream recovery", () => {
    const inFlight = new Set([streams[0].session]);

    releaseStreamRestore(inFlight, streams[0].session);

    expect(selectRecoverableStream(streams, { port: 5001, reason: 4 }, inFlight)).toEqual(streams[0]);
  });

  it("blocks a transport restore while a native restore owns the same session", () => {
    const inFlight = new Set<number>();

    expect(claimStreamRestore(inFlight, streams[0].session)).toBe(true);
    expect(claimStreamRestore(inFlight, streams[0].session)).toBe(false);
    expect(claimStreamRestore(inFlight, streams[1].session)).toBe(true);
    expect([...inFlight]).toEqual([11, 22]);
  });

  it("blocks a UDP restore while a native restore owns the same session", () => {
    const inFlight = new Set<number>();

    expect(claimStreamRestore(inFlight, streams[0].session)).toBe(true);
    expect(claimStreamRestore(inFlight, streams[0].session)).toBe(false);
    releaseStreamRestore(inFlight, streams[0].session);
    expect(claimStreamRestore(inFlight, streams[0].session)).toBe(true);
  });

  it("provides a no-op base subscription for Bun and non-native callers", () => {
    expect(() =>
      subscribeStreamTermination(() => undefined).remove(),
    ).not.toThrow();
  });
});
