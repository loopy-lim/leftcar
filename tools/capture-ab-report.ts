import { readFileSync } from "node:fs";

type SessionSample = {
  session: number;
  state: string;
  frames?: number;
  dropped?: number;
  captureBackend?: string;
  firstSendMs?: number;
  captureIntervalP95Us?: number;
  captureToEncodeP95Us?: number;
  sendBlockP95Us?: number;
  cpuPercent?: number;
  gpuPercent?: number;
  error?: string | null;
};

function samplesFrom(path: string): SessionSample[] {
  const rows = readFileSync(path, "utf8")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => JSON.parse(line) as unknown);
  const samples: SessionSample[] = [];
  for (const row of rows) {
    const value = row as {
      sessions?: SessionSample[];
      result?: { sessions?: SessionSample[] };
      session?: number;
    };
    if (Array.isArray(value.sessions)) samples.push(...value.sessions);
    else if (Array.isArray(value.result?.sessions)) samples.push(...value.result.sessions);
    else if (typeof value.session === "number") samples.push(value as SessionSample);
  }
  return samples;
}

function p95(values: number[]): number {
  if (!values.length) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.max(0, Math.ceil(sorted.length * 0.95) - 1)] ?? 0;
}

function summarize(path: string) {
  const samples = samplesFrom(path);
  const latest = new Map<number, SessionSample>();
  for (const sample of samples) latest.set(sample.session, sample);
  const sessions = [...latest.values()];
  const successful = sessions.filter(
    (sample) => sample.state === "running" && (sample.frames ?? 0) > 0 && !sample.error,
  );
  const values = (key: keyof SessionSample) =>
    successful
      .map((sample) => sample[key])
      .filter((value): value is number => typeof value === "number");
  return {
    file: path,
    backend: successful[0]?.captureBackend ?? sessions[0]?.captureBackend ?? "unknown",
    attempts: sessions.length,
    successful: successful.length,
    tenConnectionGate: sessions.length >= 10 && successful.length === sessions.length,
    totalDrops: successful.reduce((sum, sample) => sum + (sample.dropped ?? 0), 0),
    firstSendP95Ms: p95(values("firstSendMs")),
    captureIntervalP95Ms: p95(values("captureIntervalP95Us")) / 1000,
    captureToEncodeP95Ms: p95(values("captureToEncodeP95Us")) / 1000,
    sendBlockP95Ms: p95(values("sendBlockP95Us")) / 1000,
    cpuP95Percent: p95(values("cpuPercent")),
    gpuP95Percent: p95(values("gpuPercent")),
    errors: sessions.flatMap((sample) => (sample.error ? [sample.error] : [])),
  };
}

const paths = process.argv.slice(2).filter((arg) => arg !== "--");
if (paths.length !== 2) {
  throw new Error(
    "usage: bun run benchmark:capture -- <screenCaptureKit.jsonl> <cgDisplayStream.jsonl>",
  );
}

const report = paths.map(summarize);
process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
