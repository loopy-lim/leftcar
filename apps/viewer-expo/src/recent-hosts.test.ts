import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("expo-secure-store", () => {
  const store = new Map<string, string>();
  return {
    getItemAsync: vi.fn(async (key: string) => store.get(key) ?? null),
    setItemAsync: vi.fn(async (key: string, value: string) => {
      store.set(key, value);
    }),
    deleteItemAsync: vi.fn(async (key: string) => {
      store.delete(key);
    }),
    __store: store,
  };
});

import * as SecureStore from "expo-secure-store";
import {
  classifyHostAddress,
  clearRecentHosts,
  filterOutHost,
  getRecentHosts,
  MAX_RECENT_HOSTS,
  parseRecentHosts,
  removeRecentHost,
  resolveConnectCandidates,
  saveRecentHost,
  saveRecentHostStrict,
  updateRecentHostsList,
  type RecentHostItem,
} from "./recent-hosts";

const store = (SecureStore as unknown as { __store: Map<string, string> }).__store;

beforeEach(() => {
  store.clear();
  vi.clearAllMocks();
});

describe("recent-hosts module", () => {
  it("strict persistence propagates a read failure without replacing the list", async () => {
    vi.mocked(SecureStore.getItemAsync).mockRejectedValueOnce(new Error("secure read failed"));

    await expect(
      saveRecentHostStrict("192.168.0.50", 7777, "Office Mac"),
    ).rejects.toThrow("secure read failed");
    expect(SecureStore.setItemAsync).not.toHaveBeenCalled();
  });

  it("parses empty or invalid string gracefully", () => {
    expect(parseRecentHosts(null)).toEqual([]);
    expect(parseRecentHosts("")).toEqual([]);
    expect(parseRecentHosts("not a json")).toEqual([]);
    expect(parseRecentHosts("{}")).toEqual([]);
    expect(parseRecentHosts(JSON.stringify([{ invalid: true }]))).toEqual([]);
  });

  it("parses valid JSON array correctly", () => {
    const data: RecentHostItem[] = [
      { host: "192.168.1.10", port: 7777, name: "My Mac", lastConnected: 1000 },
      { host: "192.168.1.20", port: 7777, lastConnected: 2000 },
    ];
    const parsed = parseRecentHosts(JSON.stringify(data));
    expect(parsed).toEqual(data);
  });

  it("updates recent hosts list with new entry at the front", () => {
    const list: RecentHostItem[] = [
      { host: "192.168.1.10", port: 7777, lastConnected: 1000 },
    ];
    const updated = updateRecentHostsList(list, "192.168.1.20", 7777, "New PC", 2000);
    expect(updated).toHaveLength(2);
    expect(updated[0]).toEqual({
      host: "192.168.1.20",
      port: 7777,
      name: "New PC",
      lastConnected: 2000,
    });
    expect(updated[1].host).toBe("192.168.1.10");
  });

  it("deduplicates existing host and moves it to the front", () => {
    const list: RecentHostItem[] = [
      { host: "192.168.1.10", port: 7777, name: "Old", lastConnected: 1000 },
      { host: "192.168.1.20", port: 7777, lastConnected: 1500 },
    ];
    const updated = updateRecentHostsList(list, "192.168.1.10", 7777, "Updated", 3000);
    expect(updated).toHaveLength(2);
    expect(updated[0]).toEqual({
      host: "192.168.1.10",
      port: 7777,
      name: "Updated",
      lastConnected: 3000,
    });
    expect(updated[1].host).toBe("192.168.1.20");
  });

  it("caps maximum recent hosts to MAX_RECENT_HOSTS", () => {
    let list: RecentHostItem[] = [];
    for (let i = 1; i <= 10; i++) {
      list = updateRecentHostsList(list, `192.168.1.${i}`, 7777, undefined, i * 1000);
    }
    expect(list).toHaveLength(MAX_RECENT_HOSTS);
    expect(list[0].host).toBe("192.168.1.10");
  });

  it("filters out specific host by IP and port", () => {
    const list: RecentHostItem[] = [
      { host: "192.168.1.10", port: 7777, lastConnected: 1000 },
      { host: "192.168.1.20", port: 7777, lastConnected: 2000 },
    ];
    const filtered = filterOutHost(list, "192.168.1.10", 7777);
    expect(filtered).toHaveLength(1);
    expect(filtered[0].host).toBe("192.168.1.20");
  });

  it("persists and retrieves from SecureStore via getRecentHosts and saveRecentHost", async () => {
    expect(await getRecentHosts()).toEqual([]);
    await saveRecentHost("192.168.0.50", 7777, "Office Mac");
    const hosts = await getRecentHosts();
    expect(hosts).toHaveLength(1);
    expect(hosts[0].host).toBe("192.168.0.50");
    expect(hosts[0].name).toBe("Office Mac");

    await removeRecentHost("192.168.0.50", 7777);
    expect(await getRecentHosts()).toEqual([]);
  });

  it("clears all recent hosts via clearRecentHosts", async () => {
    await saveRecentHost("192.168.0.10", 7777);
    await saveRecentHost("192.168.0.20", 7777);
    expect(await getRecentHosts()).toHaveLength(2);

    await clearRecentHosts();
    expect(await getRecentHosts()).toEqual([]);
  });

  it("keeps one canonical entry when connecting through a tailnet alias", () => {
    const list: RecentHostItem[] = [
      {
        host: "192.168.0.7",
        port: 7777,
        name: "Home Mac",
        aliases: ["100.80.133.120"],
        lastConnected: 1000,
      },
    ];
    // 별칭(테일넷) 주소로 접속해 성공 — 엔트리가 쪼개지지 않고 본체가 갱신된다.
    const updated = updateRecentHostsList(list, "100.80.133.120", 7777, undefined, 2000);
    expect(updated).toHaveLength(1);
    expect(updated[0]).toEqual({
      host: "192.168.0.7",
      port: 7777,
      name: "Home Mac",
      aliases: ["100.80.133.120"],
      lastConnected: 2000,
    });
  });

  it("merges discovered aliases into the existing entry without duplicating", () => {
    const list: RecentHostItem[] = [
      { host: "192.168.0.7", port: 7777, lastConnected: 1000 },
    ];
    const updated = updateRecentHostsList(
      list,
      "192.168.0.7",
      7777,
      undefined,
      2000,
      undefined,
      ["100.80.133.120"],
    );
    expect(updated).toHaveLength(1);
    expect(updated[0].host).toBe("192.168.0.7");
    expect(updated[0].aliases).toEqual(["100.80.133.120"]);
  });

  it("drops malformed aliases when parsing persisted entries", () => {
    const raw = JSON.stringify([
      {
        host: "192.168.0.7",
        port: 7777,
        aliases: ["100.80.133.120", "not a host!", 42],
        lastConnected: 1000,
      },
    ]);
    expect(parseRecentHosts(raw)).toEqual([]);
  });

  it("orders connect candidates by current network reachability", () => {
    const item: RecentHostItem = {
      host: "100.80.133.120",
      port: 7777,
      aliases: ["192.168.0.7", "203.0.113.9", "leftcar-host.local"],
      lastConnected: 1000,
    };
    // 같은 /24 LAN → 테일넷(어디서든 도달) → 낯선 사설망/mDNS → 공인.
    expect(resolveConnectCandidates(item, ["192.168.0.42"])).toEqual([
      "192.168.0.7",
      "100.80.133.120",
      "leftcar-host.local",
      "203.0.113.9",
    ]);
    // 집 밖(다른 서브넷)에서는 테일넷이 도달 불가능한 사설망 주소보다 앞선다.
    expect(resolveConnectCandidates(item, ["10.20.30.40"])).toEqual([
      "100.80.133.120",
      "192.168.0.7",
      "leftcar-host.local",
      "203.0.113.9",
    ]);
  });

  it("classifies overlay, lan, and wan addresses", () => {
    expect(classifyHostAddress("100.64.0.1")).toBe("overlay");
    expect(classifyHostAddress("mac.example.ts.net")).toBe("overlay");
    expect(classifyHostAddress("192.168.0.7", ["192.168.0.9"])).toBe("sameLan");
    expect(classifyHostAddress("192.168.0.7", ["10.0.0.9"])).toBe("lan");
    expect(classifyHostAddress("leftcar-host.local")).toBe("lan");
    expect(classifyHostAddress("203.0.113.9")).toBe("wan");
  });
});
