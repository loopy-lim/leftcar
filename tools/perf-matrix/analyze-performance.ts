import { readFileSync, writeFileSync } from "node:fs";

export interface HostPerfSample {
  timestampMs: number;
  captureCallbacks: number;
  encodeOutputCallbacks: number;
  captureFps?: number;
  encodeOutputFps?: number;
  encodeOutputIntervalP50Us?: number;
  encodeOutputIntervalP95Us?: number;
  encodeOutputP95Us?: number;
  queueOldestUs?: number;
  encoderMode?: string;
  encoderID?: string;
}

type AndroidRecoveryEvent =
  | { type: "gap"; frameId: number }
  | { type: "idr"; frameId: number };

export interface AndroidPerfSample {
  timestampMs: number;
  eventOrder?: number;
  frames?: number;
  outputDrops?: number;
  decoderInputDrops?: number;
  frameGaps?: number;
  captureAgeMs?: number | null;
  encodeAgeMs?: number | null;
  wireAgeMs?: number | null;
  recoveryEvent?: AndroidRecoveryEvent;
}

export interface CounterSummary {
  sampleCount: number;
  elapsedMs: number | null;
  averageFps: number | null;
  rollingOneSecondP5Fps: number | null;
  maxSampleGapMs: number | null;
  zeroFpsStallDetected: boolean;
  errors: string[];
}

interface QueueTrend {
  first: number | null;
  last: number | null;
  min: number | null;
  max: number | null;
  delta: number | null;
  monotonicallyRising: boolean | null;
  sustainedAboveFrameBudget: boolean | null;
  direction: "increasing" | "stable" | "decreasing" | "unavailable";
}

interface RecoveryPair {
  gapFrameId: number;
  idrFrameId: number;
  distance: number;
}

export interface PerformanceSummary {
  host: {
    capture: CounterSummary;
    encodeOutput: CounterSummary;
    encodeOutputIntervalP50Us: number | null;
    encodeOutputIntervalP95Us: number | null;
    encodeOutputP95Us: number | null;
    queueOldestUsTrend: QueueTrend;
  };
  android: {
    rendered: CounterSummary;
    captureAgeMsP50: number | null;
    captureAgeMsP95: number | null;
    outputDrops: number | null;
    decoderInputDrops: number | null;
    frameGaps: number | null;
    pairedRecoveries: RecoveryPair[];
    unpairedGapFrameIds: number[];
    recoveryVerified: boolean;
  };
  candidate55Fps: boolean;
  final4kCriteriaMet: boolean;
}

const numberToken = /^([^=]+)=(-?\d+(?:\.\d+)?)$/;
const stringToken = /^([^=]+)=(.+)$/;
const queueFrameBudgetUs = 16_667;
const logStreamControlHeaderPrefix = "Filtering the log data using ";

function parseTokens(message: string): Record<string, string> {
  const marker = message.indexOf("LeftcarPerf");
  if (marker < 0) return {};

  const tokens: Record<string, string> = {};
  for (const token of message.slice(marker + "LeftcarPerf".length).trim().split(/\s+/)) {
    const match = token.match(stringToken);
    if (match) tokens[match[1]] = match[2];
  }
  return tokens;
}

function tokenNumber(tokens: Record<string, string>, key: string): number | undefined {
  const value = tokens[key];
  if (value === undefined || !numberToken.test(`${key}=${value}`)) return undefined;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function nonnegativeNumber(tokens: Record<string, string>, key: string): number | undefined {
  const value = tokenNumber(tokens, key);
  return value !== undefined && value >= 0 ? value : undefined;
}

function parseAgeToken(value: string | undefined): number | null | undefined {
  if (value === undefined) return undefined;
  if (value === "None") return null;
  const match = /^Some\((\d+(?:\.\d+)?)\)$/.exec(value);
  if (!match) return undefined;
  const parsed = Number(match[1]);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function nonnegativeSafeInteger(value: string | undefined): number | undefined {
  if (value === undefined || !/^\d+$/.test(value)) return undefined;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : undefined;
}

function parseAndroidTokens(line: string): Record<string, string> {
  const tokens: Record<string, string> = {};
  for (const token of line.split(/\s+/)) {
    const match = /^([A-Za-z][A-Za-z0-9]*)=(\S+)$/.exec(token);
    if (match) tokens[match[1]] = match[2];
  }
  return tokens;
}

export function parseHostLog(text: string): HostPerfSample[] {
  const samples: HostPerfSample[] = [];

  for (const line of text.split(/\r?\n/)) {
    if (!line.trim()) continue;
    if (line.startsWith(logStreamControlHeaderPrefix)) continue;
    const relevant = line.includes("LeftcarPerf");
    let row: unknown;
    try {
      row = JSON.parse(line);
    } catch {
      if (relevant) throw new Error("parse error: malformed LeftcarPerf record");
      continue;
    }
    if (!row || typeof row !== "object") continue;
    const { timestamp, eventMessage } = row as Record<string, unknown>;
    if (typeof eventMessage !== "string" || !eventMessage.includes("LeftcarPerf")) continue;
    if (typeof timestamp !== "string") throw new Error("parse error: malformed LeftcarPerf record");
    const timestampMs = Date.parse(timestamp);
    const tokens = parseTokens(eventMessage);
    const captureCallbacks = nonnegativeSafeInteger(tokens.captureCallbacks);
    const encodeOutputCallbacks = nonnegativeSafeInteger(tokens.encodeOutputCallbacks);
    const encodeOutputIntervalP50Us = nonnegativeNumber(tokens, "encodeOutputIntervalP50Us");
    const encodeOutputIntervalP95Us = nonnegativeNumber(tokens, "encodeOutputIntervalP95Us");
    const encodeOutputP95Us = nonnegativeNumber(tokens, "encodeOutputP95Us");
    const queueOldestUs = nonnegativeNumber(tokens, "queueOldestUs");
    if (
      !Number.isFinite(timestampMs) ||
      captureCallbacks === undefined ||
      encodeOutputCallbacks === undefined ||
      encodeOutputIntervalP50Us === undefined ||
      encodeOutputIntervalP95Us === undefined ||
      encodeOutputP95Us === undefined ||
      queueOldestUs === undefined
    ) {
      throw new Error("parse error: malformed LeftcarPerf record");
    }

    samples.push({
      timestampMs,
      captureCallbacks,
      encodeOutputCallbacks,
      captureFps: tokenNumber(tokens, "captureFps"),
      encodeOutputFps: tokenNumber(tokens, "encodeOutputFps"),
      encodeOutputIntervalP50Us,
      encodeOutputIntervalP95Us,
      encodeOutputP95Us,
      queueOldestUs,
      encoderMode: tokens.encoderMode,
      encoderID: tokens.encoderID,
    });
  }

  return samples.sort((left, right) => left.timestampMs - right.timestampMs);
}

export function parseAndroidLog(text: string): AndroidPerfSample[] {
  const samples: AndroidPerfSample[] = [];
  const timestampPattern = /^[\t ]*(\d+(?:\.\d+)?)\s+/;
  const renderedPattern = /\bRendered\s+(\S+)\s+frames;(?:\s|$)/;
  const gapPattern = /UDP access-unit gap detected at id=(\S+)(?:\s|$)/;
  const idrPattern = /Received IDR access unit id=(\S+)(?:\s|$)/;
  const gapMarker = /UDP access-unit gap detected\b/;
  const idrMarker = /Received IDR access unit\b/;

  for (const [eventOrder, line] of text.split(/\r?\n/).entries()) {
    const relevantRendered = /\bRendered\b/.test(line);
    const relevantRecovery = gapMarker.test(line) || idrMarker.test(line);
    const timestampMatch = timestampPattern.exec(line);
    if (!timestampMatch) {
      if (relevantRendered) throw new Error("parse error: malformed Android Rendered record");
      if (relevantRecovery) throw new Error("parse error: malformed Android recovery record");
      continue;
    }
    const timestampMs = Math.round(Number(timestampMatch[1]) * 1_000);
    if (!Number.isFinite(timestampMs)) {
      if (relevantRendered) throw new Error("parse error: malformed Android Rendered record");
      if (relevantRecovery) throw new Error("parse error: malformed Android recovery record");
      continue;
    }

    const rendered = renderedPattern.exec(line);
    if (relevantRendered) {
      if (!rendered) throw new Error("parse error: malformed Android Rendered record");
      const tokens = parseAndroidTokens(line);
      const frames = nonnegativeSafeInteger(rendered[1]);
      const outputDrops = nonnegativeSafeInteger(tokens.outputDrops);
      const decoderInputDrops = nonnegativeSafeInteger(tokens.decoderInputDrops);
      const frameGaps = nonnegativeSafeInteger(tokens.frameGaps);
      const captureAgeMs = parseAgeToken(tokens.captureAgeMs);
      if (
        frames === undefined ||
        outputDrops === undefined ||
        decoderInputDrops === undefined ||
        frameGaps === undefined ||
        captureAgeMs === undefined
      ) {
        throw new Error("parse error: malformed Android Rendered record");
      }
      samples.push({
        timestampMs,
        eventOrder,
        frames,
        outputDrops,
        decoderInputDrops,
        frameGaps,
        captureAgeMs,
        encodeAgeMs: parseAgeToken(tokens.encodeAgeMs),
        wireAgeMs: parseAgeToken(tokens.wireAgeMs),
      });
      continue;
    }

    const gap = gapPattern.exec(line);
    const idr = idrPattern.exec(line);
    if ((gapMarker.test(line) && !gap) || (idrMarker.test(line) && !idr)) {
      throw new Error("parse error: malformed Android recovery record");
    }
    if (gap || idr) {
      const frameId = nonnegativeSafeInteger(gap?.[1] ?? idr?.[1]);
      if (frameId === undefined) throw new Error("parse error: malformed Android recovery record");
      samples.push({
        timestampMs,
        eventOrder,
        recoveryEvent: gap
          ? { type: "gap", frameId }
          : { type: "idr", frameId },
      });
    }
  }

  return samples.sort((left, right) => left.timestampMs - right.timestampMs);
}

export function percentile(values: number[], quantile: number): number {
  if (values.length === 0 || quantile < 0 || quantile > 1) return Number.NaN;
  const ordered = [...values].sort((left, right) => left - right);
  return ordered[Math.max(0, Math.ceil(quantile * ordered.length) - 1)];
}

function nearestOneSecondFps(samples: Array<{ timestampMs: number; frames: number }>): number[] {
  const fps: number[] = [];
  for (let endpoint = 0; endpoint < samples.length; endpoint += 1) {
    let candidate: { timestampMs: number; frames: number } | undefined;
    for (let prior = endpoint - 1; prior >= 0; prior -= 1) {
      const elapsed = samples[endpoint].timestampMs - samples[prior].timestampMs;
      if (elapsed > 1_200) break;
      if (elapsed >= 800 && (!candidate || elapsed > samples[endpoint].timestampMs - candidate.timestampMs)) {
        candidate = samples[prior];
      }
    }
    if (candidate) {
      fps.push(((samples[endpoint].frames - candidate.frames) * 1_000) / (samples[endpoint].timestampMs - candidate.timestampMs));
    }
  }
  return fps;
}

export function summarizeCounterSeries(
  samples: Array<{ timestampMs: number; frames: number }>,
  expectedDurationMs: number,
): CounterSummary {
  const ordered = [...samples].sort((left, right) => left.timestampMs - right.timestampMs);
  if (ordered.length < 2) {
    return {
      sampleCount: ordered.length,
      elapsedMs: null,
      averageFps: null,
      rollingOneSecondP5Fps: null,
      maxSampleGapMs: null,
      zeroFpsStallDetected: false,
      errors: ["requires at least two counter samples"],
    };
  }

  const first = ordered[0];
  const last = ordered[ordered.length - 1];
  const elapsedMs = last.timestampMs - first.timestampMs;
  const gaps = ordered.slice(1).map((sample, index) => sample.timestampMs - ordered[index].timestampMs);
  const maxSampleGapMs = Math.max(...gaps);
  let lastAdvanceTimestampMs = first.timestampMs;
  let highestFrameCounter = first.frames;
  let unchangedForAtLeastOneSecond = false;
  for (const sample of ordered.slice(1)) {
    if (sample.frames > highestFrameCounter) {
      highestFrameCounter = sample.frames;
      lastAdvanceTimestampMs = sample.timestampMs;
    } else if (sample.timestampMs - lastAdvanceTimestampMs >= 1_000) {
      unchangedForAtLeastOneSecond = true;
    }
  }
  const expectedEndMs = first.timestampMs + expectedDurationMs;
  const missingTailMs = Math.max(0, expectedEndMs - last.timestampMs);
  const rolling = nearestOneSecondFps(ordered);
  const externalStallDetected = unchangedForAtLeastOneSecond || maxSampleGapMs > 1_500 || missingTailMs > 1_500;
  const zeroFpsStallDetected = externalStallDetected || rolling.some((fps) => fps === 0);
  if (externalStallDetected) rolling.push(0);

  return {
    sampleCount: ordered.length,
    elapsedMs,
    averageFps: elapsedMs > 0 ? ((last.frames - first.frames) * 1_000) / elapsedMs : null,
    rollingOneSecondP5Fps: rolling.length > 0 ? percentile(rolling, 0.05) : zeroFpsStallDetected ? 0 : null,
    maxSampleGapMs,
    zeroFpsStallDetected,
    errors: elapsedMs > 0 ? [] : ["counter timestamps must advance"],
  };
}

function metricPercentile(samples: HostPerfSample[], key: keyof HostPerfSample, quantile: number): number | null {
  const values = samples
    .map((sample) => sample[key])
    .filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  return values.length === 0 ? null : percentile(values, quantile);
}

function numericDelta(samples: AndroidPerfSample[], key: "outputDrops" | "decoderInputDrops" | "frameGaps"): number | null {
  const values = samples.map((sample) => sample[key]).filter((value): value is number => typeof value === "number");
  return values.length < 2 ? null : values[values.length - 1] - values[0];
}

function queueTrend(host: HostPerfSample[]): QueueTrend {
  const samples = [...host]
    .sort((left, right) => left.timestampMs - right.timestampMs)
    .filter((sample): sample is HostPerfSample & Required<Pick<HostPerfSample, "queueOldestUs">> => typeof sample.queueOldestUs === "number");
  const values = samples.map((sample) => sample.queueOldestUs);
  if (values.length < 2) {
    return {
      first: values[0] ?? null,
      last: values[0] ?? null,
      min: values[0] ?? null,
      max: values[0] ?? null,
      delta: null,
      monotonicallyRising: null,
      sustainedAboveFrameBudget: null,
      direction: "unavailable",
    };
  }
  const delta = values[values.length - 1] - values[0];
  let aboveBudgetSinceMs: number | null = null;
  let sustainedAboveFrameBudget = false;
  for (const sample of samples) {
    if (sample.queueOldestUs > queueFrameBudgetUs) {
      aboveBudgetSinceMs ??= sample.timestampMs;
      if (sample.timestampMs - aboveBudgetSinceMs >= 1_000) sustainedAboveFrameBudget = true;
    } else {
      aboveBudgetSinceMs = null;
    }
  }
  return {
    first: values[0],
    last: values[values.length - 1],
    min: Math.min(...values),
    max: Math.max(...values),
    delta,
    monotonicallyRising: delta > 0 && values.every((value, index) => index === 0 || value >= values[index - 1]),
    sustainedAboveFrameBudget,
    direction: delta > 0 ? "increasing" : delta < 0 ? "decreasing" : "stable",
  };
}

function summarizeRecovery(android: AndroidPerfSample[], frameGaps: number | null) {
  const events = android
    .map((sample, order) => ({ ...sample, order: sample.eventOrder ?? order }))
    .filter((sample): sample is AndroidPerfSample & { recoveryEvent: AndroidRecoveryEvent; order: number } => sample.recoveryEvent !== undefined)
    .sort((left, right) => left.timestampMs - right.timestampMs || left.order - right.order);
  const gaps = events.filter((sample) => sample.recoveryEvent.type === "gap");
  const idrs = events.filter((sample) => sample.recoveryEvent.type === "idr");
  const usedIdrOrders = new Set<number>();
  const pairedRecoveries: RecoveryPair[] = [];
  const unpairedGapFrameIds: number[] = [];

  for (const gap of gaps) {
    const gapFrameId = gap.recoveryEvent.frameId;
    const idr = idrs.find((candidate) =>
      !usedIdrOrders.has(candidate.order) &&
      (candidate.timestampMs > gap.timestampMs || (candidate.timestampMs === gap.timestampMs && candidate.order > gap.order)) &&
      candidate.recoveryEvent.frameId >= gapFrameId &&
      candidate.recoveryEvent.frameId <= gapFrameId + 2,
    );
    if (!idr) {
      unpairedGapFrameIds.push(gapFrameId);
      continue;
    }
    usedIdrOrders.add(idr.order);
    pairedRecoveries.push({
      gapFrameId,
      idrFrameId: idr.recoveryEvent.frameId,
      distance: idr.recoveryEvent.frameId - gapFrameId,
    });
  }

  return {
    pairedRecoveries,
    unpairedGapFrameIds,
    recoveryVerified: frameGaps === 0 || (
      frameGaps !== null &&
      frameGaps > 0 &&
      pairedRecoveries.length >= frameGaps &&
      unpairedGapFrameIds.length === 0
    ),
  };
}

export function summarizePerformance(
  host: HostPerfSample[],
  android: AndroidPerfSample[],
  expectedDurationMs: number,
): PerformanceSummary {
  const hostCapture = summarizeCounterSeries(host.map((sample) => ({ timestampMs: sample.timestampMs, frames: sample.captureCallbacks })), expectedDurationMs);
  const hostEncodeOutput = summarizeCounterSeries(host.map((sample) => ({ timestampMs: sample.timestampMs, frames: sample.encodeOutputCallbacks })), expectedDurationMs);
  const androidCounters = android.filter((sample): sample is AndroidPerfSample & Required<Pick<AndroidPerfSample, "frames">> => typeof sample.frames === "number");
  const androidRendered = summarizeCounterSeries(androidCounters.map((sample) => ({ timestampMs: sample.timestampMs, frames: sample.frames })), expectedDurationMs);
  const ages = androidCounters.map((sample) => sample.captureAgeMs).filter((value): value is number => typeof value === "number");
  const outputDrops = numericDelta(androidCounters, "outputDrops");
  const decoderInputDrops = numericDelta(androidCounters, "decoderInputDrops");
  const frameGaps = numericDelta(androidCounters, "frameGaps");
  const recovery = summarizeRecovery(android, frameGaps);
  const captureAgeMsP95 = ages.length ? percentile(ages, 0.95) : null;
  const hostIntervalP50 = metricPercentile(host, "encodeOutputIntervalP50Us", 0.5);
  const hostIntervalP95 = metricPercentile(host, "encodeOutputIntervalP95Us", 0.95);
  const hostOutputLatencyP95 = metricPercentile(host, "encodeOutputP95Us", 0.95);
  const hostQueueTrend = queueTrend(host);
  const summariesAreContinuous = [hostEncodeOutput, androidRendered].every(
    (summary) => summary.errors.length === 0 && !summary.zeroFpsStallDetected,
  );
  const fpsCandidate =
    summariesAreContinuous &&
    (hostEncodeOutput.averageFps ?? 0) >= 55 &&
    (androidRendered.averageFps ?? 0) >= 55 &&
    (captureAgeMsP95 ?? Number.POSITIVE_INFINITY) <= 50 &&
    hostQueueTrend.monotonicallyRising === false &&
    hostQueueTrend.sustainedAboveFrameBudget === false &&
    decoderInputDrops === 0;
  const final4kCriteriaMet =
    summariesAreContinuous &&
    (hostEncodeOutput.averageFps ?? 0) >= 59 &&
    (androidRendered.averageFps ?? 0) >= 59 &&
    (hostEncodeOutput.rollingOneSecondP5Fps ?? 0) >= 55 &&
    (androidRendered.rollingOneSecondP5Fps ?? 0) >= 55 &&
    (hostIntervalP50 ?? Number.POSITIVE_INFINITY) <= 16_700 &&
    (hostIntervalP95 ?? Number.POSITIVE_INFINITY) <= 18_500 &&
    (hostOutputLatencyP95 ?? Number.POSITIVE_INFINITY) <= 18_500 &&
    (captureAgeMsP95 ?? Number.POSITIVE_INFINITY) <= 50 &&
    hostQueueTrend.monotonicallyRising === false &&
    hostQueueTrend.sustainedAboveFrameBudget === false &&
    outputDrops === 0 &&
    decoderInputDrops === 0 &&
    frameGaps !== null &&
    frameGaps <= 2 &&
    recovery.recoveryVerified;

  return {
    host: {
      capture: hostCapture,
      encodeOutput: hostEncodeOutput,
      encodeOutputIntervalP50Us: hostIntervalP50,
      encodeOutputIntervalP95Us: hostIntervalP95,
      encodeOutputP95Us: hostOutputLatencyP95,
      queueOldestUsTrend: hostQueueTrend,
    },
    android: {
      rendered: androidRendered,
      captureAgeMsP50: ages.length ? percentile(ages, 0.5) : null,
      captureAgeMsP95,
      outputDrops,
      decoderInputDrops,
      frameGaps,
      ...recovery,
    },
    candidate55Fps: fpsCandidate,
    final4kCriteriaMet,
  };
}

interface CliArguments {
  hostPath: string;
  androidPath: string;
  durationSeconds: number;
  outputPath: string;
}

function parseCliArguments(args: string[]): CliArguments {
  const values: Partial<Record<"host" | "android" | "duration" | "output", string>> = {};
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!value || !["--host", "--android", "--duration", "--output"].includes(flag)) throw new Error("usage: --host <host.ndjson> --android <android.log> --duration <seconds> --output <summary.json>");
    values[flag.slice(2) as keyof typeof values] = value;
  }
  const durationSeconds = Number(values.duration);
  if (!values.host || !values.android || !values.output || !Number.isFinite(durationSeconds) || durationSeconds <= 0) {
    throw new Error("usage: --host <host.ndjson> --android <android.log> --duration <seconds> --output <summary.json>");
  }
  return { hostPath: values.host, androidPath: values.android, durationSeconds, outputPath: values.output };
}

export function runCli(args: string[]): PerformanceSummary {
  const { hostPath, androidPath, durationSeconds, outputPath } = parseCliArguments(args);
  const summary = summarizePerformance(
    parseHostLog(readFileSync(hostPath, "utf8")),
    parseAndroidLog(readFileSync(androidPath, "utf8")),
    durationSeconds * 1_000,
  );
  const errors = [...summary.host.capture.errors, ...summary.host.encodeOutput.errors, ...summary.android.rendered.errors];
  if (errors.length > 0) throw new Error(`parse error: ${[...new Set(errors)].join(", ")}`);
  writeFileSync(outputPath, `${JSON.stringify(summary, null, 2)}\n`);
  return summary;
}

if (process.argv[1]?.endsWith("tools/perf-matrix/analyze-performance.ts")) {
  try {
    runCli(process.argv.slice(2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 2;
  }
}
