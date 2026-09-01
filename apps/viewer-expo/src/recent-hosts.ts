import * as SecureStore from "expo-secure-store";

export interface RecentHostItem {
  host: string;
  port: number;
  name?: string;
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
          typeof item.lastConnected === "number"
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
  port = 7777,
  name?: string,
  now = Date.now(),
): RecentHostItem[] {
  const normalizedHost = host.trim().toLowerCase();
  if (!normalizedHost) return current;

  const filtered = current.filter(
    (item) => !(item.host.toLowerCase() === normalizedHost && item.port === port),
  );

  const newItem: RecentHostItem = {
    host: host.trim(),
    port,
    ...(name?.trim() ? { name: name.trim() } : {}),
    lastConnected: now,
  };

  return [newItem, ...filtered].slice(0, MAX_RECENT_HOSTS);
}

export function filterOutHost(
  current: RecentHostItem[],
  host: string,
  port = 7777,
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
  port = 7777,
  name?: string,
): Promise<RecentHostItem[]> {
  try {
    const current = await getRecentHosts();
    const updated = updateRecentHostsList(current, host, port, name);
    await SecureStore.setItemAsync(RECENT_HOSTS_KEY, JSON.stringify(updated));
    return updated;
  } catch {
    return [];
  }
}

export async function removeRecentHost(
  host: string,
  port = 7777,
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
