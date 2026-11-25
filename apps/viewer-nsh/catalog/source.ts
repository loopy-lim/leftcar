import { host } from "nsh";
import { catalogFixture, FIXTURE_HOST } from "./fixture";
import { InvalidCatalogAnswer, type ActiveStream, type CatalogSnapshot, type CatalogSource, type DisplayInfo } from "./model";

function fields(value: unknown, path: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new InvalidCatalogAnswer(path);
  return value as Record<string, unknown>;
}

function measure(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) throw new InvalidCatalogAnswer(path);
  return value;
}

function nonNegativeInteger(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0) throw new InvalidCatalogAnswer(path);
  return value;
}

function requireUnique(values: number[], path: string): void {
  const seen = new Set<number>();
  values.forEach((value, at) => {
    if (seen.has(value)) throw new InvalidCatalogAnswer(`${path}[${at}]`);
    seen.add(value);
  });
}

function word(value: unknown, path: string): string {
  if (typeof value !== "string") throw new InvalidCatalogAnswer(path);
  return value;
}

function list<T>(value: unknown, path: string, item: (entry: unknown, path: string) => T): T[] {
  if (!Array.isArray(value)) throw new InvalidCatalogAnswer(path);
  return value.map((entry, at) => item(entry, `${path}[${at}]`));
}

function display(value: unknown, path: string): DisplayInfo {
  const entry = fields(value, path);
  return {
    index: nonNegativeInteger(entry.index, `${path}.index`),
    name: word(entry.name, `${path}.name`),
    width: measure(entry.width, `${path}.width`),
    height: measure(entry.height, `${path}.height`),
  };
}

function stream(value: unknown, path: string): ActiveStream {
  const entry = fields(value, path);
  const { transport } = entry;
  if (transport !== "usb" && transport !== "udp" && transport !== "tcp") throw new InvalidCatalogAnswer(`${path}.transport`);
  return {
    session: nonNegativeInteger(entry.session, `${path}.session`),
    sourceIndex: nonNegativeInteger(entry.sourceIndex, `${path}.sourceIndex`),
    sourceName: word(entry.sourceName, `${path}.sourceName`),
    width: measure(entry.width, `${path}.width`),
    height: measure(entry.height, `${path}.height`),
    fps: measure(entry.fps, `${path}.fps`),
    transport,
  };
}

export function parseCatalog(value: unknown): CatalogSnapshot {
  const answer = fields(value, "catalog");
  const catalog: CatalogSnapshot = {
    fps: measure(answer.fps, "catalog.fps"),
    displays: list(answer.displays, "catalog.displays", display),
    streams: list(answer.streams, "catalog.streams", stream),
  };
  requireUnique(catalog.displays.map((entry) => entry.index), "catalog.displays");
  requireUnique(catalog.streams.map((entry) => entry.session), "catalog.streams");
  return catalog;
}

function hostAddress(value: unknown): string | null {
  return typeof value === "string" && value.trim() !== "" ? value.trim() : null;
}

const STOP_TIMEOUT_MS = 5_000;
const CATALOG_TIMEOUT_MS = 15_000;

const hostSource: CatalogSource = {
  hostAddress: hostAddress(host.params.host),
  load: async () => parseCatalog(await host.call("catalog.load", undefined, { timeout: CATALOG_TIMEOUT_MS })),
  open: async (display) => parseCatalog(await host.call("catalog.open", { index: display.index }, { timeout: CATALOG_TIMEOUT_MS })),
  stop: async (session) => parseCatalog(await host.call("catalog.stop", { session }, { timeout: STOP_TIMEOUT_MS })),
};

function streamFor(snapshot: CatalogSnapshot, display: DisplayInfo): ActiveStream {
  return {
    session: Math.max(0, ...snapshot.streams.map((stream) => stream.session)) + 1,
    sourceIndex: display.index,
    sourceName: display.name,
    width: display.width,
    height: display.height,
    fps: snapshot.fps,
    transport: "udp",
  };
}

export function createFixtureSource(latencyMs = 400): CatalogSource {
  let current = catalogFixture();
  const settle = <T>(answer: () => T) => new Promise<T>((resolve) => setTimeout(() => resolve(answer()), latencyMs));
  return {
    hostAddress: FIXTURE_HOST,
    load: () => settle(() => current),
    open: (display) => settle(() => {
      current = { ...current, streams: [...current.streams, streamFor(current, display)] };
      return current;
    }),
    stop: (session) => settle(() => {
      current = { ...current, streams: current.streams.filter((stream) => stream.session !== session) };
      return current;
    }),
  };
}

export function resolveCatalogSource(): CatalogSource {
  return host.available() ? hostSource : createFixtureSource();
}
