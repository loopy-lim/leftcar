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
  clearRecentHosts,
  filterOutHost,
  getRecentHosts,
  MAX_RECENT_HOSTS,
  parseRecentHosts,
  removeRecentHost,
  saveRecentHost,
  updateRecentHostsList,
  type RecentHostItem,
} from "./recent-hosts";

const store = (SecureStore as unknown as { __store: Map<string, string> }).__store;

beforeEach(() => {
  store.clear();
  vi.clearAllMocks();
});

describe("recent-hosts module", () => {
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
});
