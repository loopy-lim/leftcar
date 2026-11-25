import type { CatalogSnapshot } from "./model";

export const FIXTURE_HOST = "192.168.0.10:7777";

export function catalogFixture(overrides: Partial<CatalogSnapshot> = {}): CatalogSnapshot {
  return {
    fps: 60,
    displays: [
      { index: 0, name: "Studio Display", width: 2560, height: 1440 },
      { index: 1, name: "MacBook Pro", width: 1512, height: 982 },
      { index: 2, name: "UltraWide", width: 3440, height: 1440 },
    ],
    streams: [
      { session: 1, sourceIndex: 0, sourceName: "Studio Display", width: 2560, height: 1440, fps: 60, transport: "udp" },
    ],
    ...overrides,
  };
}
