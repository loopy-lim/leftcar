export type EncoderExperimentId =
  | "auto"
  | "rateControl"
  | "adaptiveQp"
  | "encoderPool"
  | "splitVertical";

export interface EncoderExperimentInfo {
  id: EncoderExperimentId;
  label: string;
  hint: string;
  requiresReconnect: boolean;
}

export interface StreamDimensions {
  width: number;
  height: number;
}

const PHASE_A_ENCODER_EXPERIMENT_IDS = new Set<EncoderExperimentId>([
  "auto",
  "rateControl",
  "adaptiveQp",
  "encoderPool",
  "splitVertical",
]);

function isEncoderExperimentId(value: unknown): value is EncoderExperimentId {
  return typeof value === "string"
    && PHASE_A_ENCODER_EXPERIMENT_IDS.has(value as EncoderExperimentId);
}

export function normalizeEncoderExperiments(
  advertised: unknown,
): EncoderExperimentInfo[] {
  if (!Array.isArray(advertised)) return [];

  const normalized: EncoderExperimentInfo[] = [];
  const seen = new Set<EncoderExperimentId>();

  for (const candidate of advertised) {
    if (!candidate || typeof candidate !== "object") continue;
    const entry = candidate as Record<string, unknown>;
    if (
      !isEncoderExperimentId(entry.id)
      || typeof entry.label !== "string"
      || entry.label.trim().length === 0
      || typeof entry.hint !== "string"
      || entry.hint.trim().length === 0
      || entry.requiresReconnect !== true
      || seen.has(entry.id)
    ) {
      continue;
    }
    seen.add(entry.id);
    normalized.push({
      id: entry.id,
      label: entry.label,
      hint: entry.hint,
      requiresReconnect: true,
    });
  }

  return normalized;
}

export function is4KResolution(width: number, height: number): boolean {
  // Stream profiles use source width/height as-is; they do not rotate the
  // dimensions, so portrait 2160x3840 is intentionally not an equivalent 4K
  // selector resolution.
  return width >= 3840 && height >= 2160;
}

export function availableEncoderExperiments(
  advertised: unknown,
  width: number,
  height: number,
): EncoderExperimentInfo[] {
  const available = normalizeEncoderExperiments(advertised);
  if (!is4KResolution(width, height)) {
    return available.filter((experiment) => experiment.id === "auto");
  }
  return available;
}

export function availableEncoderExperimentsForStreams(
  advertised: unknown,
  streams: readonly StreamDimensions[],
): EncoderExperimentInfo[] {
  const has4KStream = streams.some((stream) =>
    is4KResolution(stream.width, stream.height),
  );
  return availableEncoderExperiments(
    advertised,
    has4KStream ? 3840 : 0,
    has4KStream ? 2160 : 0,
  );
}

export function resolveEncoderExperimentForStream(
  selected: EncoderExperimentId,
  advertised: unknown,
  width: number,
  height: number,
): EncoderExperimentId {
  const available = availableEncoderExperiments(advertised, width, height);
  return available.some((experiment) => experiment.id === selected)
    ? selected
    : "auto";
}

export function resolveEncoderExperiment(
  selected: EncoderExperimentId,
  advertised: unknown,
): EncoderExperimentId {
  return resolveEncoderExperimentForStream(selected, advertised, 3840, 2160);
}

export function selectAutomaticEncoderExperiment(
  selected: EncoderExperimentId,
  advertised: unknown,
  width: number,
  height: number,
  mediaTransport: string,
): EncoderExperimentId {
  const resolved = resolveEncoderExperimentForStream(
    selected,
    advertised,
    width,
    height,
  );
  if (resolved !== "auto" || !is4KResolution(width, height)) return resolved;
  if (mediaTransport.trim().toLowerCase() !== "udp") return resolved;

  const available = availableEncoderExperiments(advertised, width, height);
  return available.some((experiment) => experiment.id === "auto")
    && available.some((experiment) => experiment.id === "splitVertical")
    ? "splitVertical"
    : resolved;
}
