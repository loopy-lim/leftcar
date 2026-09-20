import * as SecureStore from "expo-secure-store";
import { DEFAULT_CONTROL_PORT } from "./defaults";

export interface RecentHostItem {
  host: string;
  port: number;
  name?: string;
  /** 호스트 공개키(b64url 32B) — 핸드셰이크 핀. v2 페어링 이후 채워진다. */
  hostKey?: string;
  /**
   * 같은 호스트의 다른 도달 경로(테일넷 100.x, LAN 주소). 대표 주소는
   * 엔트리가 처음 만들어진 주소가 유지하고, 접속 순서는 저장값이 아니라
   * 현재 네트워크에서 계산한다(resolveConnectCandidates).
   */
  aliases?: string[];
  lastConnected: number;
}

export const RECENT_HOSTS_KEY = "leftcar.recent_hosts";
export const MAX_RECENT_HOSTS = 5;
export const MAX_HOST_ALIASES = 4;

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
            typeof (item as { hostKey?: unknown }).hostKey === "string") &&
          (!("aliases" in item) || isValidAliases((item as { aliases?: unknown }).aliases))
        );
      })
      .slice(0, MAX_RECENT_HOSTS);
  } catch {
    return [];
  }
}

function isPlausibleAddress(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.trim().length > 0 &&
    value.trim().length <= 253 &&
    !value.includes(" ") &&
    // 포트는 별도 필드라 호스트 문자열에 콜론이 오면 저장 오류다.
    !value.includes(":")
  );
}

function isValidAliases(value: unknown): boolean {
  return (
    Array.isArray(value) &&
    value.length <= MAX_HOST_ALIASES &&
    value.every((entry) => isPlausibleAddress(entry))
  );
}

function normalizeAddress(value: string): string {
  return value.trim().toLowerCase();
}

/** trim·중복 제거·개수 제한. 대소문자는 첫 등장 표기를 유지한다. */
function normalizeAliases(values?: string[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const value of values ?? []) {
    if (!isPlausibleAddress(value)) continue;
    const key = normalizeAddress(value);
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(value.trim());
  }
  return result.slice(0, MAX_HOST_ALIASES);
}

export function updateRecentHostsList(
  current: RecentHostItem[],
  host: string,
  port = DEFAULT_CONTROL_PORT,
  name?: string,
  now = Date.now(),
  hostKey?: string,
  discoveredAliases?: string[],
): RecentHostItem[] {
  const normalizedHost = normalizeAddress(host);
  if (!normalizedHost) return current;
  const discovered = normalizeAliases(discoveredAliases).filter(
    (alias) => normalizeAddress(alias) !== normalizedHost,
  );

  const exact = current.find(
    (item) => normalizeAddress(item.host) === normalizedHost && item.port === port,
  );
  // 접속에 성공한 주소가 기존 엔트리의 별칭이면 그 엔트리가 본체다 — 주소마다
  // 엔트리를 쪼개지 않고 본체의 별칭 목록만 넓힌다(LAN·테일넷 한 엔트리).
  const canonical =
    exact ??
    current.find(
      (item) =>
        item.port === port &&
        ((item.aliases ?? []).some((alias) => normalizeAddress(alias) === normalizedHost) ||
          discovered.some(
            (alias) =>
              normalizeAddress(alias) === normalizeAddress(item.host) ||
              (item.aliases ?? []).some(
                (existing) => normalizeAddress(existing) === normalizeAddress(alias),
              ),
          )),
    );

  // 핀이 있는 엔트리는 갱신 시에도 유지한다 — 키 없는 재연결이 핀을 지우고
  // 다음 연결을 TOFU로 강등시키지 않게 한다.
  const resolvedHostKey = hostKey !== undefined ? hostKey : canonical?.hostKey;
  const resolvedName = name?.trim() || canonical?.name;
  const primaryHost = canonical?.host ?? host.trim();
  const aliases = normalizeAliases([
    ...(canonical?.aliases ?? []),
    ...(canonical && normalizeAddress(primaryHost) !== normalizedHost ? [host.trim()] : []),
    ...discovered,
  ]).filter((alias) => normalizeAddress(alias) !== normalizeAddress(primaryHost));

  const filtered = current.filter((item) => item !== canonical);
  const newItem: RecentHostItem = {
    host: primaryHost,
    port: canonical?.port ?? port,
    ...(resolvedName ? { name: resolvedName } : {}),
    ...(resolvedHostKey ? { hostKey: resolvedHostKey } : {}),
    ...(aliases.length > 0 ? { aliases } : {}),
    lastConnected: now,
  };

  return [newItem, ...filtered].slice(0, MAX_RECENT_HOSTS);
}

/** 접속 가능 주소의 경로 유형. 숫자가 클수록 느린/불확실한 경로다. */
export type HostRouteClass = "sameLan" | "lan" | "overlay" | "wan";

const ROUTE_CLASS_ORDER: Record<HostRouteClass, number> = {
  sameLan: 0,
  overlay: 1,
  lan: 2,
  wan: 3,
};

function sameSubnet24(a: string, b: string): boolean {
  const left = a.split(".");
  const right = b.split(".");
  return (
    left.length === 4 &&
    right.length === 4 &&
    left[0] === right[0] &&
    left[1] === right[1] &&
    left[2] === right[2]
  );
}

/**
 * 한 주소가 지금 네트워크에서 어떤 경로인지 분류한다. 같은 /24 사설망이
 * 최상이고, 테일넷(100.64/10·*.ts.net)은 그다음, 나머지 공인 주소는 마지막이다.
 */
export function classifyHostAddress(
  host: string,
  viewerIps: readonly string[] = [],
): HostRouteClass {
  const normalized = normalizeAddress(host).replace(/\.$/, "");
  const octets = normalized.split(".").map(Number);
  const isIpv4 =
    octets.length === 4 && octets.every((n) => Number.isInteger(n) && n >= 0 && n <= 255);
  if (isIpv4) {
    const [a, b] = octets as [number, number, number, number];
    if (a === 100 && b >= 64 && b <= 127) return "overlay";
    if (
      a === 10 ||
      a === 127 ||
      (a === 172 && b >= 16 && b <= 31) ||
      (a === 192 && b === 168) ||
      (a === 169 && b === 254)
    ) {
      return viewerIps.some((viewerIp) => sameSubnet24(normalized, viewerIp))
        ? "sameLan"
        : "lan";
    }
    return "wan";
  }
  if (normalized.endsWith(".ts.net")) return "overlay";
  if (normalized.endsWith(".local") || normalized === "localhost") return "lan";
  return "wan";
}

/**
 * 호스트가 광고한 다른 경로(테일넷·LAN 주소)를 같은 엔트리의 별칭으로 병합한다.
 * 테일넷 주소로 접속하면 LAN 별칭이, LAN으로 접속하면 테일넷 별칭이 채워져
 * 어느 네트워크에서든 한 엔트리로 접속 후보를 갖는다. 이미 모두 아는 주소면
 * 아무것도 쓰지 않는다.
 */
export async function mergeAdvertisedRoutes(
  target: { host: string; port: number },
  advertised: { tailscaleHost?: string | null; mediaHost?: string | null },
): Promise<void> {
  const routes = [advertised.tailscaleHost, advertised.mediaHost].filter(
    (address): address is string =>
      typeof address === "string" && isPlausibleAddress(address),
  );
  if (routes.length === 0) return;
  const normalizedTarget = target.host.trim().toLowerCase();
  const current = await getRecentHostsStrict();
  const entry = current.find(
    (candidate) =>
      candidate.port === target.port &&
      [candidate.host, ...(candidate.aliases ?? [])].some(
        (address) => address.trim().toLowerCase() === normalizedTarget,
      ),
  );
  const present = new Set(
    [entry?.host, ...(entry?.aliases ?? [])]
      .filter((address): address is string => typeof address === "string")
      .map((address) => address.trim().toLowerCase()),
  );
  const missing = routes.filter(
    (address) => !present.has(address.trim().toLowerCase()),
  );
  if (missing.length === 0) return;
  await saveRecentHostStrict(target.host, target.port, undefined, undefined, undefined, missing);
}

/**
 * 엔트리의 주소 후보를 지금 네트워크에 맞는 접속 순서로 정렬한다. 대표 주소와
 * 별칭을 모두 포함하며, 같은 등급 안에서는 저장 순서(대표 우선)를 유지한다.
 * 같은 /24 사설망이 최상, 테일넷이 그다음(어디서든 도달), 낯선 사설망·mDNS는
 * 집 안에서만 유효하므로 그다음, 공인 주소가 마지막이다.
 */
export function resolveConnectCandidates(
  item: RecentHostItem,
  viewerIps: readonly string[] = [],
): string[] {
  const seen = new Set<string>();
  const candidates: string[] = [];
  for (const address of [item.host, ...(item.aliases ?? [])]) {
    if (!isPlausibleAddress(address)) continue;
    const key = normalizeAddress(address);
    if (seen.has(key)) continue;
    seen.add(key);
    candidates.push(address.trim());
  }
  return candidates.sort(
    (a, b) =>
      ROUTE_CLASS_ORDER[classifyHostAddress(a, viewerIps)] -
      ROUTE_CLASS_ORDER[classifyHostAddress(b, viewerIps)],
  );
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
    return await getRecentHostsStrict();
  } catch {
    return [];
  }
}

/** Read the persisted list without converting provider failures into an empty list. */
export async function getRecentHostsStrict(): Promise<RecentHostItem[]> {
  const stored = await SecureStore.getItemAsync(RECENT_HOSTS_KEY);
  return parseRecentHosts(stored);
}

export async function saveRecentHost(
  host: string,
  port = DEFAULT_CONTROL_PORT,
  name?: string,
  hostKey?: string,
  discoveredAliases?: string[],
): Promise<RecentHostItem[]> {
  try {
    return await saveRecentHostStrict(host, port, name, hostKey, undefined, discoveredAliases);
  } catch {
    return [];
  }
}

/**
 * Pairing credential transactions need persistence failures to be observable so
 * token and endpoint-alias changes can be rolled back together. Ordinary UI
 * callers keep the best-effort `saveRecentHost` behavior above.
 */
export async function saveRecentHostStrict(
  host: string,
  port = DEFAULT_CONTROL_PORT,
  name?: string,
  hostKey?: string,
  signal?: AbortSignal,
  discoveredAliases?: string[],
): Promise<RecentHostItem[]> {
  if (signal?.aborted) throw abortError();
  const current = await getRecentHostsStrict();
  if (signal?.aborted) throw abortError();
  const updated = updateRecentHostsList(
    current,
    host,
    port,
    name,
    Date.now(),
    hostKey,
    discoveredAliases,
  );
  await SecureStore.setItemAsync(RECENT_HOSTS_KEY, JSON.stringify(updated));
  if (signal?.aborted) throw abortError();
  return updated;
}

function abortError(): Error {
  const error = new Error("recent host persistence cancelled");
  error.name = "AbortError";
  return error;
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
