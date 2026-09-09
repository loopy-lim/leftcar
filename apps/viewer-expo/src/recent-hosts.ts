import * as SecureStore from "expo-secure-store";
import { DEFAULT_CONTROL_PORT } from "./defaults";

export interface RecentHostItem {
  host: string;
  port: number;
  name?: string;
  /** 호스트 공개키(b64url 32B) — 핸드셰이크 핀. v2 페어링 이후 채워진다. */
  hostKey?: string;
  lastConnected: number;
}

export const RECENT_HOSTS_KEY = "leftcar.recent_hosts";
export const MAX_RECENT_HOSTS = 5;

export function parseRecentHosts(raw: string | null): RecentHostItem[] {
  if (!raw || typeof raw !== "string") return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((item): item is RecentHostItem => {
        return (
          typeof item === "object" &&
          item !== null &&
          typeof item.host === "string" &&
          item.host.trim().length > 0 &&
          typeof item.port === "number" &&
          Number.isInteger(item.port) &&
          item.port > 0 &&
          item.port <= 65535 &&
          typeof item.lastConnected === "number" &&
          (!("hostKey" in item) ||
            typeof (item as { hostKey?: unknown }).hostKey === "string")
        );
      })
      .slice(0, MAX_RECENT_HOSTS);
  } catch {
    return [];
  }
}

export function updateRecentHostsList(
  current: RecentHostItem[],
  host: string,
  port = DEFAULT_CONTROL_PORT,
  name?: string,
  now = Date.now(),
  hostKey?: string,
): RecentHostItem[] {
  const normalizedHost = host.trim().toLowerCase();
  if (!normalizedHost) return current;

  const previous = current.find(
    (item) => item.host.toLowerCase() === normalizedHost && item.port === port,
  );
  const filtered = current.filter(
    (item) => !(item.host.toLowerCase() === normalizedHost && item.port === port),
  );

  // 핀이 있는 엔트리는 갱신 시에도 유지한다 — 키 없는 재연결이 핀을 지우고
  // 다음 연결을 TOFU로 강등시키지 않게 한다.
  const resolvedHostKey = hostKey !== undefined ? hostKey : previous?.hostKey;

  const newItem: RecentHostItem = {
    host: host.trim(),
    port,
    ...(name?.trim() ? { name: name.trim() } : previous?.name ? { name: previous.name } : {}),
    ...(resolvedHostKey ? { hostKey: resolvedHostKey } : {}),
    lastConnected: now,
  };

  return [newItem, ...filtered].slice(0, MAX_RECENT_HOSTS);
}

export function filterOutHost(
  current: RecentHostItem[],
  host: string,
  port = DEFAULT_CONTROL_PORT,
): RecentHostItem[] {
  const normalizedHost = host.trim().toLowerCase();
  return current.filter(
    (item) => !(item.host.toLowerCase() === normalizedHost && item.port === port),
  );
}

export async function getRecentHosts(): Promise<RecentHostItem[]> {
  try {
    const stored = await SecureStore.getItemAsync(RECENT_HOSTS_KEY);
    return parseRecentHosts(stored);
  } catch {
    return [];
  }
}

export async function saveRecentHost(
  host: string,
  port = DEFAULT_CONTROL_PORT,
  name?: string,
  hostKey?: string,
): Promise<RecentHostItem[]> {
  try {
    const current = await getRecentHosts();
    const updated = updateRecentHostsList(current, host, port, name, Date.now(), hostKey);
    await SecureStore.setItemAsync(RECENT_HOSTS_KEY, JSON.stringify(updated));
    return updated;
  } catch {
    return [];
  }
}

export async function removeRecentHost(
  host: string,
  port = DEFAULT_CONTROL_PORT,
): Promise<RecentHostItem[]> {
  try {
    const current = await getRecentHosts();
    const updated = filterOutHost(current, host, port);
    await SecureStore.setItemAsync(RECENT_HOSTS_KEY, JSON.stringify(updated));
    return updated;
  } catch {
    return [];
  }
}

export async function clearRecentHosts(): Promise<void> {
  try {
    await SecureStore.deleteItemAsync(RECENT_HOSTS_KEY);
  } catch {
    // best-effort
  }
}
