import { describe, expect, it } from "vitest";
import {
  LEGACY_UDP_STABILITY_KEY,
  UDP_STABILITY_KEY,
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

  it("normalizes and deduplicates selectable presets only", () => {
    // 2026-11-22 단순화: balanced(중복)와 custom(세부 조정)은 선택지에서
    // 제외되고, 나머지 광고 항목은 교집합 규칙을 그대로 따른다.
    expect(
      availableUdpStabilityOptions({
        ...advertised,
        profiles: ["auto", "stable", "stable", "future", "balanced"],
        burstDatagramOptions: [16, 8, 4, 4, 6, 2],
        fecParityOptions: [4, 2, 4, 8],
      }),
    ).toEqual({
      profiles: ["auto", "stable"],
      requiresReconnect: true,
    });
  });

  it("hides stable when four-parity FEC is not in the intersection", () => {
    expect(
      availableUdpStabilityOptions({
        ...advertised,
        fecParityOptions: [2],
      })?.profiles,
    ).toEqual(["auto", "responsive"]);
  });

  it("accepts only advertised selectable presets", () => {
    expect(resolveUdpStabilitySelection({ profile: "stable" }, advertised)).toEqual({
      profile: "stable",
    });
    expect(
      resolveUdpStabilitySelection(
        { profile: "stable" },
        { ...advertised, profiles: ["auto", "responsive"] },
      ),
    ).toBeNull();
    // 뷰어가 더 이상 제공하지 않는 프로파일은 호스트가 광고해도 거절한다.
    expect(
      resolveUdpStabilitySelection(
        { profile: "balanced" as "stable" },
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
      async deleteItemAsync() { undefined; },
    };
    const controller = new PreferencePersistenceController({
      initialValue: { profile: "auto" } as const,
      load: () => readUdpStabilitySelection(store),
      save: (value) => writeUdpStabilitySelection(store, value),
      storageKey: UDP_STABILITY_KEY,
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

  it.each(["broken", "[]", '{"profile":"unknown"}', '{"profile":"custom","burstDatagrams":3}', '{"profile":"balanced"}'])(
    "reports malformed saved UDP preference instead of accepting a replacement default: %s",
    async (raw) => {
      await expect(readUdpStabilitySelection({
        getItemAsync: async () => raw,
        setItemAsync: async () => undefined,
        deleteItemAsync: async () => undefined,
      })).rejects.toThrow();
    },
  );

  it("migrates by key rotation: a missing new key starts at auto and deletes the legacy key", async () => {
    const deleted: string[] = [];
    const selection = await readUdpStabilitySelection({
      getItemAsync: async (key) =>
        key === UDP_STABILITY_KEY ? null : '{"profile":"stable"}',
      setItemAsync: async () => undefined,
      deleteItemAsync: async (key) => { deleted.push(key); },
    });
    expect(selection).toEqual({ profile: "auto" });
    expect(deleted).toEqual([LEGACY_UDP_STABILITY_KEY]);
  });

  it("keeps a valid stored selection under the new key and still clears the legacy key", async () => {
    const deleted: string[] = [];
    const selection = await readUdpStabilitySelection({
      getItemAsync: async (key) =>
        key === UDP_STABILITY_KEY ? '{"profile":"responsive"}' : null,
      setItemAsync: async () => undefined,
      deleteItemAsync: async (key) => { deleted.push(key); },
    });
    expect(selection).toEqual({ profile: "responsive" });
    expect(deleted).toEqual([LEGACY_UDP_STABILITY_KEY]);
  });

  it("ignores a failed legacy key deletion", async () => {
    const selection = await readUdpStabilitySelection({
      getItemAsync: async () => '{"profile":"auto"}',
      setItemAsync: async () => undefined,
      deleteItemAsync: async () => { throw new Error("locked"); },
    });
    expect(selection).toEqual({ profile: "auto" });
  });
});
