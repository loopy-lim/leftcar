import { signal, type Signal } from "nsh";
import { t } from "../i18n";

export interface DisplayInfo {
  index: number;
  name: string;
  width: number;
  height: number;
}

export interface ActiveStream {
  session: number;
  sourceIndex: number;
  sourceName: string;
  width: number;
  height: number;
  fps: number;
  transport: "usb" | "udp" | "tcp";
}

export interface CatalogSnapshot {
  fps: number;
  displays: DisplayInfo[];
  streams: ActiveStream[];
}

export class InvalidCatalogAnswer extends Error {
  constructor(readonly path: string) {
    super(`invalid catalog answer: ${path}`);
  }
}

export interface CatalogSource {
  readonly hostAddress: string | null;
  load(): Promise<CatalogSnapshot>;
  open(display: DisplayInfo): Promise<CatalogSnapshot>;
  stop(session: number): Promise<CatalogSnapshot>;
}

export interface CatalogModel {
  readonly hostAddress: string | null;
  snapshot: Signal<CatalogSnapshot | null>;
  loading: Signal<boolean>;
  error: Signal<string | null>;
  launchingIndex: Signal<number | null>;
  stoppingSession: Signal<number | null>;
  stopFailed: Signal<number[]>;
  openingOrStopping(): boolean;
  refresh(): Promise<void>;
  open(display: DisplayInfo): Promise<void>;
  stop(session: number): Promise<void>;
}

function hostGuidance(said: string): boolean {
  return /[가-힣]/.test(said);
}

function failureText(cause: unknown): string {
  const code = typeof cause === "object" && cause !== null ? (cause as { code?: unknown }).code : undefined;
  if (cause instanceof InvalidCatalogAnswer) return t().viewer.errGeneric;
  if (code === "ETIMEDOUT") return t().viewer.errHostTimeout;
  const said = cause instanceof Error ? cause.message : typeof cause === "string" ? cause : "";
  if (hostGuidance(said)) return said;
  if (said) console.warn("[leftcar] unmapped catalog error:", said);
  return t().viewer.errGeneric;
}

export function createCatalogModel(source: CatalogSource): CatalogModel {
  const snapshot = signal<CatalogSnapshot | null>(null);
  const loading = signal(false);
  const error = signal<string | null>(null);
  const launchingIndex = signal<number | null>(null);
  const stoppingSession = signal<number | null>(null);
  const stopFailed = signal<number[]>([]);
  let newestRequest = 0;

  const openingOrStopping = () => launchingIndex() !== null || stoppingSession() !== null;

  function show(catalog: CatalogSnapshot) {
    snapshot.set(catalog);
    const failed = stopFailed().filter((session) => catalog.streams.some((stream) => stream.session === session));
    if (failed.length !== stopFailed().length) stopFailed.set(failed);
  }

  async function refresh() {
    if (openingOrStopping() || loading()) return;
    const request = ++newestRequest;
    error.set(null);
    loading.set(true);
    try {
      const catalog = await source.load();
      if (request === newestRequest) show(catalog);
    } catch (cause) {
      if (request === newestRequest) error.set(failureText(cause));
    } finally {
      if (request === newestRequest) loading.set(false);
    }
  }

  void refresh();

  function supersedePendingRead(): void {
    newestRequest += 1;
    loading.set(false);
  }

  function beginChange(): boolean {
    if (openingOrStopping()) return false;
    supersedePendingRead();
    return true;
  }

  return {
    hostAddress: source.hostAddress,
    snapshot,
    loading,
    error,
    launchingIndex,
    stoppingSession,
    stopFailed,
    openingOrStopping,
    refresh,
    async open(display) {
      if (!beginChange()) return;
      error.set(null);
      launchingIndex.set(display.index);
      try {
        show(await source.open(display));
      } catch (cause) {
        error.set(failureText(cause));
      } finally {
        launchingIndex.set(null);
      }
    },
    async stop(session) {
      if (!beginChange()) return;
      stopFailed.set(stopFailed().filter((failed) => failed !== session));
      stoppingSession.set(session);
      try {
        show(await source.stop(session));
      } catch (cause) {
        stopFailed.set([...stopFailed(), session]);
        error.set(failureText(cause));
      } finally {
        stoppingSession.set(null);
      }
    },
  };
}
