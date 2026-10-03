import { describe, expect, it } from "vitest";
import {
  VIEWER_UDP_CAPABILITIES,
  availableUdpStabilityOptions,
  readUdpStabilitySelection,
  resolveUdpStabilitySelection,
  writeUdpStabilitySelection,
} from "./udp-stability";
import { PreferencePersistenceController } from "./preference-persistence";

const advertised = {
  version: 1,
  profiles: ["auto", "responsive", "balanced", "stable"],
  burstDatagramOptions: [2, 4, 8, 16],
  fecParityOptions: [2, 4],
  adaptivePacing: true,
  requiresReconnect: true,
};

describe("UDP stability capability intersection", () => {
  it("treats absent and malformed Host capabilities as legacy", () => {
    expect(availableUdpStabilityOptions(undefined)).toBeNull();
    expect(availableUdpStabilityOptions({ version: "1" })).toBeNull();
  });

  it("normalizes, intersects, and deduplicates discrete options", () => {
    expect(
      availableUdpStabilityOptions({
        ...advertised,
        profiles: ["auto", "stable", "stable", "future"],
        burstDatagramOptions: [16, 8, 4, 4, 6, 2],
        fecParityOptions: [4, 2, 4, 8],
      }),
    ).toEqual({
      profiles: ["auto", "stable"],
      burstDatagrams: [2, 4, 8, 16],
      fecParityShards: [2, 4],
      adaptivePacing: true,
      requiresReconnect: true,
    });
  });

  it("hides stable when four-parity FEC is not in the intersection", () => {
    expect(
      availableUdpStabilityOptions({
        ...advertised,
        fecParityOptions: [2],
      })?.profiles,
    ).toEqual(["auto", "responsive", "balanced"]);
  });

  it("accepts only advertised preset and custom values", () => {
    expect(resolveUdpStabilitySelection({ profile: "stable" }, advertised)).toEqual({
      profile: "stable",
    });
    expect(
      resolveUdpStabilitySelection(
        {
          profile: "custom",
          burstDatagrams: 16,
          fecParityShards: 2,
          adaptivePacing: false,
        },
        advertised,
      ),
    ).toEqual({
      profile: "custom",
      burstDatagrams: 16,
      fecParityShards: 2,
      adaptivePacing: false,
    });
    expect(
      resolveUdpStabilitySelection(
        {
          profile: "custom",
          burstDatagrams: 2,
          fecParityShards: 4,
          adaptivePacing: false,
        },
        advertised,
      ),
    ).toEqual({
      profile: "custom",
      burstDatagrams: 2,
      fecParityShards: 4,
      adaptivePacing: false,
    });
    expect(
      resolveUdpStabilitySelection(
        {
          profile: "custom",
          burstDatagrams: 8,
          fecParityShards: 8 as 4,
          adaptivePacing: false,
        },
        advertised,
      ),
    ).toBeNull();
  });

  it("advertises the exact Viewer receiver capability", () => {
    expect(VIEWER_UDP_CAPABILITIES).toEqual({
      version: 1,
      maxFecParityShards: 4,
      splitFeedbackBytes: 120,
    });
  });
});

describe("UDP preference persistence", () => {
  it("preserves the stored selection after a failed read and restores it on retry", async () => {
    let raw = '{"profile":"stable"}';
    let failRead = true;
    const store = {
      async getItemAsync() {
        if (failRead) throw new Error("secure storage unavailable");
        return raw;
      },
      async setItemAsync(_key: string, value: string) { raw = value; },
    };
    const controller = new PreferencePersistenceController({
      initialValue: { profile: "auto" } as const,
      load: () => readUdpStabilitySelection(store),
      save: (value) => writeUdpStabilitySelection(store, value),
      storageKey: "leftcar.udpStability",
      storageOwner: store,
    });

    await controller.load();
    expect(controller.state.status).toBe("load-error");
    expect(raw).toBe('{"profile":"stable"}');

    failRead = false;
    await controller.retry();
    expect(controller.state).toEqual({ status: "ready", value: { profile: "stable" } });
    expect(raw).toBe('{"profile":"stable"}');
  });

  it.each(["broken", "[]", '{"profile":"unknown"}', '{"profile":"custom","burstDatagrams":3}'])(
    "reports malformed saved UDP preference instead of accepting a replacement default: %s",
    async (raw) => {
      await expect(readUdpStabilitySelection({
        getItemAsync: async () => raw,
        setItemAsync: async () => undefined,
      })).rejects.toThrow();
    },
  );
});
