import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, test } from "vitest";

import {
  parseAndroidLog,
  parseHostLog,
  percentile,
  runCli,
  summarizeCounterSeries,
  summarizePerformance,
} from "./analyze-performance";

describe("counter summaries", () => {
  test("derives average and one-second p5 from cumulative counter deltas", () => {
    const samples = [
      { timestampMs: 0, frames: 0 },
      { timestampMs: 500, frames: 30 },
      { timestampMs: 1_000, frames: 60 },
      { timestampMs: 1_500, frames: 90 },
    ];

    expect(summarizeCounterSeries(samples, 1_500)).toMatchObject({
      averageFps: 60,
      rollingOneSecondP5Fps: 60,
      zeroFpsStallDetected: false,
    });
  });

  test("marks unchanged counters as zero-FPS stalls", () => {
    const stalled = [
      { timestampMs: 0, frames: 0 },
      { timestampMs: 1_000, frames: 60 },
      { timestampMs: 3_000, frames: 60 },
    ];

    expect(summarizeCounterSeries(stalled, 3_000)).toMatchObject({
      rollingOneSecondP5Fps: 0,
      zeroFpsStallDetected: true,
    });
  });

  test("marks a counter unchanged for over one second across sub-second samples as stalled", () => {
    expect(
      summarizeCounterSeries(
        [
          { timestampMs: 0, frames: 0 },
          { timestampMs: 700, frames: 42 },
          { timestampMs: 1_400, frames: 42 },
          { timestampMs: 2_100, frames: 42 },
        ],
        2_100,
      ),
    ).toMatchObject({
      rollingOneSecondP5Fps: 0,
      zeroFpsStallDetected: true,
    });
  });

  test("uses the oldest eligible sample for each one-second window", () => {
    expect(
      summarizeCounterSeries(
        [
          { timestampMs: 0, frames: 0 },
          { timestampMs: 200, frames: 20 },
          { timestampMs: 1_000, frames: 80 },
        ],
        1_000,
      ).rollingOneSecondP5Fps,
    ).toBe(80);
  });

  test("marks a long sample gap and missing expected tail as stalls", () => {
    expect(
      summarizeCounterSeries(
        [
          { timestampMs: 0, frames: 0 },
          { timestampMs: 2_000, frames: 120 },
        ],
        4_000,
      ),
    ).toMatchObject({
      maxSampleGapMs: 2_000,
      zeroFpsStallDetected: true,
    });
  });

  test("reports insufficient samples as an explicit error", () => {
    expect(summarizeCounterSeries([{ timestampMs: 0, frames: 0 }], 1_000)).toMatchObject({
      averageFps: null,
      rollingOneSecondP5Fps: null,
      errors: ["requires at least two counter samples"],
    });
  });
});

describe("percentiles", () => {
  test("uses nearest-rank percentiles", () => {
    expect(percentile([25, 30, 40, 50], 0.5)).toBe(30);
    expect(percentile([25, 30, 40, 50], 0.95)).toBe(50);
  });
});

describe("log parsers", () => {
  test("parses Android epoch logs and exact Some age syntax", () => {
    const line =
      "1777777777.500  1234  1234 I LeftcarNative: Rendered 120 frames; outputDrops=2 staleInputs=0 staleInputDrops=0 outputBurst=0 fecRecovered=0 decoderInputsQueued=120 decoderInputDrops=3 completedBatch=1 liveEdgeBatch=1 maxCompletedBatch=1 frameGaps=4 intentionalLiveEdgeGaps=0 recoverySkippedFrames=0 feedUs=100 maxFeedUs=200 captureAgeMs=Some(41) encodeAgeMs=Some(12) wireAgeMs=Some(7)";

    expect(parseAndroidLog(line)).toEqual([
      expect.objectContaining({
        timestampMs: 1_777_777_777_500,
        frames: 120,
        outputDrops: 2,
        decoderInputDrops: 3,
        frameGaps: 4,
        captureAgeMs: 41,
        encodeAgeMs: 12,
        wireAgeMs: 7,
      }),
    ]);
  });

  test("parses Android Rendered records with leading epoch whitespace", () => {
    const line =
      "         1777777777.500  1234  1234 I LeftcarNative: Rendered 120 frames; outputDrops=2 decoderInputDrops=3 frameGaps=4 captureAgeMs=Some(41) encodeAgeMs=Some(12) wireAgeMs=Some(7)";

    expect(parseAndroidLog(line)).toEqual([
      expect.objectContaining({
        timestampMs: 1_777_777_777_500,
        frames: 120,
        outputDrops: 2,
        decoderInputDrops: 3,
        frameGaps: 4,
        captureAgeMs: 41,
      }),
    ]);
  });

  test("preserves explicit None Android ages", () => {
    const samples = parseAndroidLog(
      "1777777778.000  1234  1234 I LeftcarNative: Rendered 121 frames; outputDrops=2 decoderInputDrops=3 frameGaps=4 captureAgeMs=None encodeAgeMs=None wireAgeMs=None",
    );

    expect(samples[0]).toMatchObject({ captureAgeMs: null, encodeAgeMs: null, wireAgeMs: null });
  });

  test("parses macOS NDJSON timestamps and LeftcarPerf event-message tokens", () => {
    const text = JSON.stringify({
      timestamp: "2026-08-27T10:00:00.500Z",
      eventMessage:
        "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=59 captureFps=60 encodeOutputFps=59 encodeOutputIntervalP50Us=16600 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=24000 queueOldestUs=2100 encoderWatchdogRestarts=2 encoderWatchdogTerminations=1 encoderLateCallbacks=3 encoderWatchdogOldestUs=250000 encoderMode=hevc encoderID=com.apple.videotoolbox.videoencoder",
    });

    expect(parseHostLog(text)).toEqual([
      expect.objectContaining({
        timestampMs: Date.parse("2026-08-27T10:00:00.500Z"),
        captureCallbacks: 60,
        encodeOutputCallbacks: 59,
        encodeOutputIntervalP50Us: 16_600,
        encodeOutputIntervalP95Us: 18_000,
        encodeOutputP95Us: 24_000,
        queueOldestUs: 2_100,
        encoderWatchdogRestarts: 2,
        encoderWatchdogTerminations: 1,
        encoderLateCallbacks: 3,
        encoderWatchdogOldestUs: 250_000,
        encoderMode: "hevc",
        encoderID: "com.apple.videotoolbox.videoencoder",
      }),
    ]);
  });

  test("ignores the exact log-stream filter header without accepting malformed performance records", () => {
    const filterHeader =
      'Filtering the log data using "process == "leftcar-host-desktop" AND composedMessage CONTAINS "LeftcarPerf""';

    expect(parseHostLog(filterHeader)).toEqual([]);
    expect(() => parseHostLog("not NDJSON LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=60")).toThrow(
      "parse error: malformed LeftcarPerf record",
    );
  });

  test("rejects malformed relevant Host performance records", () => {
    expect(() => parseHostLog(JSON.stringify({
      timestamp: "2026-08-27T10:00:00.500Z",
      eventMessage: "LeftcarPerf captureCallbacks=60",
    }))).toThrow("parse error: malformed LeftcarPerf record");
    expect(() => parseHostLog("not NDJSON LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=60")).toThrow(
      "parse error: malformed LeftcarPerf record",
    );
  });

  test("rejects Host records missing queue or finite latency summary fields", () => {
    expect(() => parseHostLog(JSON.stringify({
      timestamp: "2026-08-27T10:00:00.500Z",
      eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=59 encodeOutputIntervalP50Us=16600 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=24000",
    }))).toThrow("parse error: malformed LeftcarPerf record");
    expect(() => parseHostLog(JSON.stringify({
      timestamp: "2026-08-27T10:00:00.500Z",
      eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=59 encodeOutputIntervalP50Us=16600 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=not-a-number queueOldestUs=2100",
    }))).toThrow("parse error: malformed LeftcarPerf record");
  });

  test("rejects negative, fractional, and oversized Host cumulative counters", () => {
    for (const eventMessage of [
      "LeftcarPerf captureCallbacks=-1 encodeOutputCallbacks=59 encodeOutputIntervalP50Us=16600 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=24000 queueOldestUs=2100",
      "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=-1 encodeOutputIntervalP50Us=16600 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=24000 queueOldestUs=2100",
      "LeftcarPerf captureCallbacks=60.5 encodeOutputCallbacks=59 encodeOutputIntervalP50Us=16600 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=24000 queueOldestUs=2100",
      "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=59.5 encodeOutputIntervalP50Us=16600 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=24000 queueOldestUs=2100",
      "LeftcarPerf captureCallbacks=999999999999999999999999999999 encodeOutputCallbacks=59 encodeOutputIntervalP50Us=16600 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=24000 queueOldestUs=2100",
      "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=999999999999999999999999999999 encodeOutputIntervalP50Us=16600 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=24000 queueOldestUs=2100",
    ]) {
      expect(() => parseHostLog(JSON.stringify({ timestamp: "2026-08-27T10:00:00.500Z", eventMessage }))).toThrow(
        "parse error: malformed LeftcarPerf record",
      );
    }
  });

  test("rejects negative queue age while retaining finite decimal latency metrics", () => {
    expect(() => parseHostLog(JSON.stringify({
      timestamp: "2026-08-27T10:00:00.500Z",
      eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=59 encodeOutputIntervalP50Us=16600.5 encodeOutputIntervalP95Us=18000.5 encodeOutputP95Us=24000.5 queueOldestUs=-1",
    }))).toThrow("parse error: malformed LeftcarPerf record");
    expect(parseHostLog(JSON.stringify({
      timestamp: "2026-08-27T10:00:00.500Z",
      eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=59 encodeOutputIntervalP50Us=16600.5 encodeOutputIntervalP95Us=18000.5 encodeOutputP95Us=24000.5 queueOldestUs=0.5",
    }))[0]).toMatchObject({ queueOldestUs: 0.5, encodeOutputIntervalP50Us: 16_600.5 });
  });

  test("rejects negative required Host latency metrics while retaining decimals", () => {
    for (const eventMessage of [
      "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=59 encodeOutputIntervalP50Us=-0.5 encodeOutputIntervalP95Us=18000.5 encodeOutputP95Us=24000.5 queueOldestUs=2100",
      "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=59 encodeOutputIntervalP50Us=16600.5 encodeOutputIntervalP95Us=-0.5 encodeOutputP95Us=24000.5 queueOldestUs=2100",
      "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=59 encodeOutputIntervalP50Us=16600.5 encodeOutputIntervalP95Us=18000.5 encodeOutputP95Us=-0.5 queueOldestUs=2100",
    ]) {
      expect(() => parseHostLog(JSON.stringify({ timestamp: "2026-08-27T10:00:00.500Z", eventMessage }))).toThrow(
        "parse error: malformed LeftcarPerf record",
      );
    }
    expect(parseHostLog(JSON.stringify({
      timestamp: "2026-08-27T10:00:00.500Z",
      eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=59 encodeOutputIntervalP50Us=16600.5 encodeOutputIntervalP95Us=18000.5 encodeOutputP95Us=24000.5 queueOldestUs=2100",
    }))[0]).toMatchObject({
      encodeOutputIntervalP50Us: 16_600.5,
      encodeOutputIntervalP95Us: 18_000.5,
      encodeOutputP95Us: 24_000.5,
    });
  });

  test("rejects malformed relevant Android rendered records", () => {
    expect(() => parseAndroidLog(
      "I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=None",
    )).toThrow("parse error: malformed Android Rendered record");
    expect(() => parseAndroidLog(
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 frameGaps=0 captureAgeMs=None",
    )).toThrow("parse error: malformed Android Rendered record");
  });

  test("rejects Android records with missing or malformed capture age evidence", () => {
    expect(() => parseAndroidLog(
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(not-a-number)",
    )).toThrow("parse error: malformed Android Rendered record");
    expect(() => parseAndroidLog(
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0",
    )).toThrow("parse error: malformed Android Rendered record");
  });

  test("rejects Android Rendered token suffixes and oversized cumulative counters", () => {
    expect(() => parseAndroidLog(
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0junk decoderInputDrops=0 frameGaps=0 captureAgeMs=None",
    )).toThrow("parse error: malformed Android Rendered record");
    expect(() => parseAndroidLog(
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)junk",
    )).toThrow("parse error: malformed Android Rendered record");
    expect(() => parseAndroidLog(
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 999999999999999999999999999999 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=None",
    )).toThrow("parse error: malformed Android Rendered record");
    expect(() => parseAndroidLog(
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=999999999999999999999999999999 decoderInputDrops=0 frameGaps=0 captureAgeMs=None",
    )).toThrow("parse error: malformed Android Rendered record");
  });

  test("rejects malformed relevant gap and IDR recovery records", () => {
    expect(() => parseAndroidLog(
      "1777777777.500  1234  1234 W LeftcarNative: UDP access-unit gap detected at id=not-a-number",
    )).toThrow("parse error: malformed Android recovery record");
    expect(() => parseAndroidLog(
      "I LeftcarNative: Received IDR access unit id=101",
    )).toThrow("parse error: malformed Android recovery record");
  });

  test("rejects non-whitespace prefixes before relevant Android timestamps", () => {
    expect(() => parseAndroidLog(
      "junk1777777777.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=None",
    )).toThrow("parse error: malformed Android Rendered record");
    expect(() => parseAndroidLog(
      "junk1777777777.500  1234  1234 W LeftcarNative: UDP access-unit gap detected at id=101",
    )).toThrow("parse error: malformed Android recovery record");
  });

  test("rejects recovery ID suffixes and oversized IDs", () => {
    expect(() => parseAndroidLog(
      "1777777777.500  1234  1234 W LeftcarNative: UDP access-unit gap detected at id=101junk",
    )).toThrow("parse error: malformed Android recovery record");
    expect(() => parseAndroidLog(
      "1777777777.600  1234  1234 I LeftcarNative: Received IDR access unit id=999999999999999999999999999999",
    )).toThrow("parse error: malformed Android recovery record");
  });

  test("ignores unrelated Host and Android headers", () => {
    expect(parseHostLog('{"eventMessage":"unrelated header"}')).toEqual([]);
    expect(parseAndroidLog("--------- beginning of main\n1777777777.000  1234  1234 I OtherTag: unrelated")).toEqual([]);
  });
});

describe("performance evidence", () => {
  test("pairs leading-space epoch gap and IDR recovery records", () => {
    const host = parseHostLog([
      { timestamp: "2026-08-27T10:00:00.000Z", eventMessage: "LeftcarPerf captureCallbacks=0 encodeOutputCallbacks=0 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=0" },
      { timestamp: "2026-08-27T10:00:01.000Z", eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=60 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=0" },
    ].map((row) => JSON.stringify(row)).join("\n"));
    const android = parseAndroidLog([
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 0 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=None",
      "         1777777777.500  1234  1234 W LeftcarNative: UDP access-unit gap detected at id=101",
      "\t1777777777.600  1234  1234 I LeftcarNative: Received IDR access unit id=102",
      "1777777778.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=1 captureAgeMs=None",
    ].join("\n"));

    expect(summarizePerformance(host, android, 1_000).android).toMatchObject({
      frameGaps: 1,
      pairedRecoveries: [{ gapFrameId: 101, idrFrameId: 102, distance: 1 }],
      unpairedGapFrameIds: [],
      recoveryVerified: true,
    });
  });

  test("fails closed before malformed Host counters can satisfy candidate or final criteria", () => {
    const malformedHost = [
      { timestamp: "2026-08-27T10:00:00.000Z", eventMessage: "LeftcarPerf captureCallbacks=0 encodeOutputCallbacks=0 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=100" },
      { timestamp: "2026-08-27T10:00:01.000Z", eventMessage: "LeftcarPerf captureCallbacks=60.5 encodeOutputCallbacks=60 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=100" },
      { timestamp: "2026-08-27T10:00:02.000Z", eventMessage: "LeftcarPerf captureCallbacks=120 encodeOutputCallbacks=120 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=100" },
    ].map((row) => JSON.stringify(row)).join("\n");
    const android = parseAndroidLog([
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 0 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
      "1777777778.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
      "1777777779.000  1234  1234 I LeftcarNative: Rendered 120 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
    ].join("\n"));

    expect(() => summarizePerformance(parseHostLog(malformedHost), android, 2_000)).toThrow(
      "parse error: malformed LeftcarPerf record",
    );
  });

  test("fails closed before malformed Android evidence can satisfy final 4K criteria", () => {
    const host = parseHostLog([
      { timestamp: "2026-08-27T10:00:00.000Z", eventMessage: "LeftcarPerf captureCallbacks=0 encodeOutputCallbacks=0 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=100" },
      { timestamp: "2026-08-27T10:00:01.000Z", eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=60 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=100" },
      { timestamp: "2026-08-27T10:00:02.000Z", eventMessage: "LeftcarPerf captureCallbacks=120 encodeOutputCallbacks=120 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=100" },
    ].map((row) => JSON.stringify(row)).join("\n"));
    const malformedAndroid = [
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 0 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
      "1777777778.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0junk decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
      "1777777779.000  1234  1234 I LeftcarNative: Rendered 120 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
    ].join("\n");

    expect(() => summarizePerformance(host, parseAndroidLog(malformedAndroid), 2_000)).toThrow(
      "parse error: malformed Android Rendered record",
    );
  });

  test("uses Android deltas and pairs a nearby IDR recovery with a gap", () => {
    const host = parseHostLog(
      [
        { timestamp: "2026-08-27T10:00:00.000Z", eventMessage: "LeftcarPerf captureCallbacks=0 encodeOutputCallbacks=0 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=20000 queueOldestUs=100" },
        { timestamp: "2026-08-27T10:00:01.000Z", eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=58 encodeOutputIntervalP50Us=17000 encodeOutputIntervalP95Us=19000 encodeOutputP95Us=22000 queueOldestUs=150" },
      ].map((row) => JSON.stringify(row)).join("\n"),
    );
    const android = parseAndroidLog([
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 0 frames; outputDrops=10 decoderInputDrops=4 frameGaps=3 captureAgeMs=Some(25)",
      "1777777777.500  1234  1234 W LeftcarNative: UDP access-unit gap detected at id=101",
      "1777777777.600  1234  1234 I LeftcarNative: Received IDR access unit id=102",
      "1777777778.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=12 decoderInputDrops=5 frameGaps=4 captureAgeMs=Some(50)",
    ].join("\n"));

    expect(summarizePerformance(host, android, 1_000)).toMatchObject({
      host: expect.objectContaining({
        encodeOutputIntervalP50Us: 16_000,
        encodeOutputIntervalP95Us: 19_000,
        encodeOutputP95Us: 22_000,
        queueOldestUsTrend: expect.objectContaining({ direction: "increasing", delta: 50 }),
      }),
      android: expect.objectContaining({
        outputDrops: 2,
        decoderInputDrops: 1,
        frameGaps: 1,
        captureAgeMsP50: 25,
        captureAgeMsP95: 50,
        recoveryVerified: true,
      }),
    });
  });

  test("leaves a nonzero gap delta failed when no IDR recovery can be paired", () => {
    const host = parseHostLog([
      { timestamp: "2026-08-27T10:00:00.000Z", eventMessage: "LeftcarPerf captureCallbacks=0 encodeOutputCallbacks=0 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=0" },
      { timestamp: "2026-08-27T10:00:01.000Z", eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=60 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=0" },
    ].map((row) => JSON.stringify(row)).join("\n"));
    const android = parseAndroidLog([
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 0 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=None",
      "1777777778.000  1234  1234 W LeftcarNative: UDP access-unit gap detected at id=101",
      "1777777778.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=1 captureAgeMs=None",
    ].join("\n"));

    expect(summarizePerformance(host, android, 1_000).android).toMatchObject({
      frameGaps: 1,
      recoveryVerified: false,
      unpairedGapFrameIds: [101],
    });
  });

  test("rejects an IDR that arrived before its gap", () => {
    const host = parseHostLog([
      { timestamp: "2026-08-27T10:00:00.000Z", eventMessage: "LeftcarPerf captureCallbacks=0 encodeOutputCallbacks=0 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=0" },
      { timestamp: "2026-08-27T10:00:01.000Z", eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=60 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=0" },
    ].map((row) => JSON.stringify(row)).join("\n"));
    const android = parseAndroidLog([
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 0 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=None",
      "1777777777.300  1234  1234 I LeftcarNative: Received IDR access unit id=101",
      "1777777777.500  1234  1234 W LeftcarNative: UDP access-unit gap detected at id=101",
      "1777777778.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=1 captureAgeMs=None",
    ].join("\n"));

    expect(summarizePerformance(host, android, 1_000).android).toMatchObject({
      recoveryVerified: false,
      unpairedGapFrameIds: [101],
    });
  });

  test("requires recovery pair count to cover the cumulative gap delta", () => {
    const host = parseHostLog([
      { timestamp: "2026-08-27T10:00:00.000Z", eventMessage: "LeftcarPerf captureCallbacks=0 encodeOutputCallbacks=0 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=0" },
      { timestamp: "2026-08-27T10:00:01.000Z", eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=60 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=0" },
    ].map((row) => JSON.stringify(row)).join("\n"));
    const android = parseAndroidLog([
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 0 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=None",
      "1777777777.500  1234  1234 W LeftcarNative: UDP access-unit gap detected at id=101",
      "1777777777.600  1234  1234 I LeftcarNative: Received IDR access unit id=102",
      "1777777778.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=2 captureAgeMs=None",
    ].join("\n"));

    expect(summarizePerformance(host, android, 1_000).android).toMatchObject({
      frameGaps: 2,
      pairedRecoveries: [expect.objectContaining({ gapFrameId: 101, idrFrameId: 102 })],
      recoveryVerified: false,
    });
  });

  test("does not fail final criteria for a queue-age spike that recovers", () => {
    const host = parseHostLog([
      { timestamp: "2026-08-27T10:00:00.000Z", eventMessage: "LeftcarPerf captureCallbacks=0 encodeOutputCallbacks=0 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=100" },
      { timestamp: "2026-08-27T10:00:01.000Z", eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=60 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=20000" },
      { timestamp: "2026-08-27T10:00:02.000Z", eventMessage: "LeftcarPerf captureCallbacks=120 encodeOutputCallbacks=120 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=100" },
    ].map((row) => JSON.stringify(row)).join("\n"));
    const android = parseAndroidLog([
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 0 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
      "1777777778.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
      "1777777779.000  1234  1234 I LeftcarNative: Rendered 120 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
    ].join("\n"));

    expect(summarizePerformance(host, android, 2_000)).toMatchObject({
      host: { queueOldestUsTrend: { min: 100, max: 20_000, monotonicallyRising: false } },
      final4kCriteriaMet: true,
    });
  });

  test("fails final criteria for queue age sustained above budget for one second", () => {
    const host = parseHostLog([
      { timestamp: "2026-08-27T10:00:00.000Z", eventMessage: "LeftcarPerf captureCallbacks=0 encodeOutputCallbacks=0 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=100" },
      { timestamp: "2026-08-27T10:00:01.000Z", eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=60 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=20000" },
      { timestamp: "2026-08-27T10:00:02.000Z", eventMessage: "LeftcarPerf captureCallbacks=120 encodeOutputCallbacks=120 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=19900" },
      { timestamp: "2026-08-27T10:00:03.000Z", eventMessage: "LeftcarPerf captureCallbacks=180 encodeOutputCallbacks=180 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=20000" },
    ].map((row) => JSON.stringify(row)).join("\n"));
    const android = parseAndroidLog([
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 0 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
      "1777777778.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
      "1777777779.000  1234  1234 I LeftcarNative: Rendered 120 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
      "1777777780.000  1234  1234 I LeftcarNative: Rendered 180 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
    ].join("\n"));

    expect(summarizePerformance(host, android, 3_000)).toMatchObject({
      host: { queueOldestUsTrend: { sustainedAboveFrameBudget: true } },
      final4kCriteriaMet: false,
    });
  });
});

describe("CLI", () => {
  test("writes a full summary from the documented host and Android inputs", () => {
    const directory = mkdtempSync(join(tmpdir(), "leftcar-perf-matrix-"));
    const hostPath = join(directory, "host.ndjson");
    const androidPath = join(directory, "android.log");
    const outputPath = join(directory, "summary.json");
    writeFileSync(hostPath, [
      { timestamp: "2026-08-27T10:00:00.000Z", eventMessage: "LeftcarPerf captureCallbacks=0 encodeOutputCallbacks=0 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=19000 queueOldestUs=100" },
      { timestamp: "2026-08-27T10:00:01.000Z", eventMessage: "LeftcarPerf captureCallbacks=60 encodeOutputCallbacks=60 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=19000 queueOldestUs=100" },
    ].map((row) => JSON.stringify(row)).join("\n"));
    writeFileSync(androidPath, [
      "1777777777.000  1234  1234 I LeftcarNative: Rendered 0 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(25)",
      "1777777778.000  1234  1234 I LeftcarNative: Rendered 60 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=Some(30)",
    ].join("\n"));

    try {
      expect(runCli(["--host", hostPath, "--android", androidPath, "--duration", "1", "--output", outputPath])).toMatchObject({
        candidate55Fps: true,
        final4kCriteriaMet: false,
      });
      expect(JSON.parse(readFileSync(outputPath, "utf8"))).toMatchObject({
        host: { encodeOutput: { averageFps: 60 } },
        android: { rendered: { averageFps: 60 } },
      });
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });

  test("returns a parse error instead of writing a one-sample pass", () => {
    const directory = mkdtempSync(join(tmpdir(), "leftcar-perf-matrix-"));
    const hostPath = join(directory, "host.ndjson");
    const androidPath = join(directory, "android.log");
    const outputPath = join(directory, "summary.json");
    writeFileSync(hostPath, JSON.stringify({ timestamp: "2026-08-27T10:00:00.000Z", eventMessage: "LeftcarPerf captureCallbacks=0 encodeOutputCallbacks=0 encodeOutputIntervalP50Us=16000 encodeOutputIntervalP95Us=18000 encodeOutputP95Us=18000 queueOldestUs=0" }));
    writeFileSync(androidPath, "1777777777.000  1234  1234 I LeftcarNative: Rendered 0 frames; outputDrops=0 decoderInputDrops=0 frameGaps=0 captureAgeMs=None");

    try {
      expect(() => runCli(["--host", hostPath, "--android", androidPath, "--duration", "1", "--output", outputPath])).toThrow(
        "parse error: requires at least two counter samples",
      );
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});
