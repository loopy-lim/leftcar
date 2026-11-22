export type UdpStabilityProfileId = "auto" | "responsive" | "stable";

export interface UdpStabilitySelection {
  profile: UdpStabilityProfileId;
}

export interface UdpStabilityOptions {
  profiles: UdpStabilityProfileId[];
  requiresReconnect: boolean;
}

export const VIEWER_UDP_CAPABILITIES = {
  version: 1,
  maxFecParityShards: 4,
  splitFeedbackBytes: 120,
} as const;

// 2026-11-22 단순화: 뷰어가 제공하는 선택지는 자동/저지연/안정 세 가지다.
// 'balanced'는 자동과 값이 사실상 같은 중복이라 걷어냈고(호스트 계약에는
// 구버전 협상·자동 폴백용으로 유지), 세부 수동 조정(custom)도 auto 적응
// 사다리가 대신하면서 제거했다.
const SELECTABLE_PROFILE_IDS = ["auto", "responsive", "stable"] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function availableUdpStabilityOptions(
  advertised: unknown,
): UdpStabilityOptions | null {
  if (!isRecord(advertised) || advertised.version !== 1) return null;
  const advertisedProfiles = advertised.profiles;
  if (!Array.isArray(advertisedProfiles)) return null;

  const fecParitySet = new Set<number>(
    Array.isArray(advertised.fecParityOptions)
      ? advertised.fecParityOptions
      : [],
  );
  const advertisedProfileSet = new Set(advertisedProfiles);
  const profiles = SELECTABLE_PROFILE_IDS.filter((profile) =>
    advertisedProfileSet.has(profile),
  ).filter((profile) => profile !== "stable" || fecParitySet.has(4));
  if (profiles.length === 0) return null;

  return {
    profiles,
    requiresReconnect: advertised.requiresReconnect === true,
  };
}

export function resolveUdpStabilitySelection(
  selection: UdpStabilitySelection,
  advertised: unknown,
): UdpStabilitySelection | null {
  const options = availableUdpStabilityOptions(advertised);
  if (!options) return null;
  return options.profiles.includes(selection.profile)
    ? { profile: selection.profile }
    : null;
}

export const UDP_STABILITY_KEY = "leftcar.udpStability.v2";
// 2026-11-22 이전 키. 세부 수동 조정·중복 프리셋이 사라지며 저장값 전부를
// 한 번 auto로 초기화한다(키 회전 — pairing.ts LEGACY_TOKEN_KEY 패턴).
// 실패해도 읽기 결과에 영향을 주지 않는 best-effort 삭제만 한다.
export const LEGACY_UDP_STABILITY_KEY = "leftcar.udpStability";

interface UdpStabilityStore {
  getItemAsync(key: string): Promise<string | null>;
  setItemAsync(key: string, value: string): Promise<void>;
  deleteItemAsync(key: string): Promise<void>;
}

function parseUdpStabilitySelection(raw: string | null): UdpStabilitySelection {
  if (raw === null) return { profile: "auto" };
  const parsed: unknown = JSON.parse(raw);
  if (!isRecord(parsed)) throw new Error("Invalid UDP preference");
  const profile = SELECTABLE_PROFILE_IDS.find(
    (candidate) => candidate === parsed.profile,
  );
  if (!profile) throw new Error("Invalid UDP preference");
  return { profile };
}

/**
 * UDP 안정성 선택을 저장해 다음 세션에서 그대로 쓴다. 호스트가 지원하지
 * 않는 조합은 effectiveUdpStability의 광고 교집합에서 안전하게 강등된다.
 * 새 키에 값이 없으면(마이그레이션 직후) auto로 시작하고 레거시 키를
 * 지워 되살아나지 않게 한다.
 */
export async function readUdpStabilitySelection(
  store: UdpStabilityStore,
): Promise<UdpStabilitySelection> {
  const raw = await store.getItemAsync(UDP_STABILITY_KEY);
  try {
    await store.deleteItemAsync(LEGACY_UDP_STABILITY_KEY);
  } catch {
    // 레거시 키 삭제 실패는 남은 고립 값일 뿐 무시한다.
  }
  return parseUdpStabilitySelection(raw);
}

export async function writeUdpStabilitySelection(
  store: UdpStabilityStore,
  selection: UdpStabilitySelection,
): Promise<void> {
  await store.setItemAsync(UDP_STABILITY_KEY, JSON.stringify(selection));
}
