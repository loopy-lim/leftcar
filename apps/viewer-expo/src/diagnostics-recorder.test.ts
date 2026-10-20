import { describe, expect, it } from "vitest";
import {
  DiagnosticsRecorder,
  type DiagnosticsRecorderNative,
  type DiagnosticsRecorderPorts,
} from "./diagnostics-recorder";

function fakeNative(
  overrides: Partial<DiagnosticsRecorderNative> = {},
): DiagnosticsRecorderNative & { lines: string[] } {
  const lines: string[] = [];
  return {
    lines,
    appendRecord: async (line) => {
      lines.push(line);
      return true;
    },
    uptimeMs: async () => 45_000,
    getSummary: async () => ({}),
    exportToDownloads: async () => "leftcar-diagnostics-20261019-000000.ndjson",
    ...overrides,
  };
}

function ports(overrides: Partial<DiagnosticsRecorderPorts> = {}): DiagnosticsRecorderPorts {
  return {
    native: null,
    epochMs: () => 1_760_000_000_000,
    uptimeMs: async () => 45_000,
    subscribeAppState: () => () => undefined,
    currentState: () => "active",
    ...overrides,
  };
}

describe("DiagnosticsRecorder", () => {
  it("records stream start with profile and decoder slots", async () => {
    const native = fakeNative();
    const recorder = new DiagnosticsRecorder(ports({ native }));
    await recorder.recordStreamStart("splitVertical", 2);
    expect(native.lines).toEqual([
      '{"ts":1760000000000,"up_ms":45000,"kind":"session","event":"stream_start","profile":"splitVertical","decoder_slots":2}',
    ]);
  });

  it("records stream stop", async () => {
    const native = fakeNative();
    const recorder = new DiagnosticsRecorder(ports({ native }));
    await recorder.recordStreamStop();
    expect(native.lines).toEqual([
      '{"ts":1760000000000,"up_ms":45000,"kind":"session","event":"stream_stop"}',
    ]);
  });

  it("is a no-op without native and reports unavailability", async () => {
    const recorder = new DiagnosticsRecorder(ports({}));
    await expect(recorder.recordStreamStart("video", 1)).resolves.toBeUndefined();
    expect(await recorder.summary()).toBeNull();
    expect(await recorder.exportToDownloads()).toBeNull();
    expect(recorder.available).toBe(false);
  });

  it("swallows append and clock failures so streams keep flowing", async () => {
    const native = fakeNative({
      appendRecord: async () => {
        throw new Error("io");
      },
      uptimeMs: async () => {
        throw new Error("clock");
      },
    });
    const recorder = new DiagnosticsRecorder(ports({ native }));
    await expect(recorder.recordStreamStop()).resolves.toBeUndefined();
    expect(native.lines).toEqual([]);
  });

  it("degrades an unavailable clock to up_ms zero", async () => {
    const native = fakeNative();
    const recorder = new DiagnosticsRecorder(
      ports({
        native,
        uptimeMs: async () => {
          throw new Error("clock");
        },
      }),
    );
    await recorder.recordStreamStop();
    expect(native.lines[0]).toContain('"up_ms":0');
  });

  it("records lifecycle transitions only, ignoring repeats and inactive", async () => {
    const native = fakeNative();
    const channel: {
      listener: ((state: string) => void | Promise<void>) | null;
    } = { listener: null };
    const recorder = new DiagnosticsRecorder(
      ports({
        native,
        subscribeAppState: (registered) => {
          channel.listener = registered;
          return () => {
            channel.listener = null;
          };
        },
        currentState: () => "active",
      }),
    );
    const stop = recorder.startLifecycle();
    if (!channel.listener) throw new Error("AppState listener가 등록되지 않았다");
    await channel.listener("background");
    await channel.listener("background");
    await channel.listener("inactive");
    await channel.listener("active");
    stop();
    expect(native.lines).toEqual([
      '{"ts":1760000000000,"up_ms":45000,"kind":"session","event":"background"}',
      '{"ts":1760000000000,"up_ms":45000,"kind":"session","event":"foreground"}',
    ]);
  });

  it("delegates summary and export to native", async () => {
    const native = fakeNative({
      getSummary: async () => ({ sample: 3, crash: 0 }),
      exportToDownloads: async () => "leftcar-diagnostics-x.ndjson",
    });
    const recorder = new DiagnosticsRecorder(ports({ native }));
    expect(recorder.available).toBe(true);
    expect(await recorder.summary()).toEqual({ sample: 3, crash: 0 });
    expect(await recorder.exportToDownloads()).toBe("leftcar-diagnostics-x.ndjson");
  });
});

describe("DiagnosticsRecorder thermal events", () => {
  it("records the thermal cap entry with its caps", async () => {
    const native = fakeNative();
    const recorder = new DiagnosticsRecorder(ports({ native }));
    await recorder.recordThermalCap(3, 2, 1_080);
    expect(native.lines).toEqual([
      '{"ts":1760000000000,"up_ms":45000,"kind":"session","event":"thermal_cap","thermal_status":3,"capacity_cap":2,"start_short_side_cap":1080}',
    ]);
  });

  it("records the thermal cap clear", async () => {
    const native = fakeNative();
    const recorder = new DiagnosticsRecorder(ports({ native }));
    await recorder.recordThermalCapCleared(1);
    expect(native.lines).toEqual([
      '{"ts":1760000000000,"up_ms":45000,"kind":"session","event":"thermal_cap_cleared","thermal_status":1}',
    ]);
  });

  it("is a no-op for thermal events without native", async () => {
    const recorder = new DiagnosticsRecorder(ports({}));
    await expect(recorder.recordThermalCap(3, 2)).resolves.toBeUndefined();
    await expect(recorder.recordThermalCapCleared(1)).resolves.toBeUndefined();
  });
});
