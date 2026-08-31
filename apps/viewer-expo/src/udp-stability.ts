export type UdpStabilityProfileId =
  | "auto"
  | "responsive"
  | "balanced"
  | "stable"
  | "custom";

export type UdpBurstDatagrams = 2 | 4 | 8 | 16;
export type UdpFecParityShards = 2 | 4;

export interface UdpStabilitySelection {
  profile: UdpStabilityProfileId;
  burstDatagrams?: UdpBurstDatagrams;
  fecParityShards?: UdpFecParityShards;
  adaptivePacing?: boolean;
}

export interface UdpStabilityOptions {
  profiles: Exclude<UdpStabilityProfileId, "custom">[];
  burstDatagrams: UdpBurstDatagrams[];
  fecParityShards: UdpFecParityShards[];
  adaptivePacing: boolean;
  requiresReconnect: boolean;
}

export const VIEWER_UDP_CAPABILITIES = {
  version: 1,
  maxFecParityShards: 4,
  splitFeedbackBytes: 120,
} as const;

const PRESET_IDS = ["auto", "responsive", "balanced", "stable"] as const;
const BURST_OPTIONS = [2, 4, 8, 16] as const;
const PARITY_OPTIONS = [2, 4] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function intersectNumbers<T extends number>(
  advertised: unknown,
  supported: readonly T[],
): T[] {
  if (!Array.isArray(advertised)) return [];
  const advertisedSet = new Set(advertised);
  return supported.filter((candidate) => advertisedSet.has(candidate));
}

export function availableUdpStabilityOptions(
  advertised: unknown,
): UdpStabilityOptions | null {
  if (!isRecord(advertised) || advertised.version !== 1) return null;
  const advertisedProfiles = advertised.profiles;
  if (!Array.isArray(advertisedProfiles)) return null;

  const burstDatagrams = intersectNumbers(
    advertised.burstDatagramOptions,
    BURST_OPTIONS,
  );
  const fecParityShards = intersectNumbers(
    advertised.fecParityOptions,
    PARITY_OPTIONS,
  );
  if (burstDatagrams.length === 0 || fecParityShards.length === 0) return null;

  const advertisedProfileSet = new Set(advertisedProfiles);
  const fecParitySet = new Set(fecParityShards);
  const profiles = PRESET_IDS.filter((profile) =>
    advertisedProfileSet.has(profile),
  ).filter((profile) => profile !== "stable" || fecParitySet.has(4));
  if (profiles.length === 0) return null;

  return {
    profiles,
    burstDatagrams,
    fecParityShards,
    adaptivePacing: advertised.adaptivePacing === true,
    requiresReconnect: advertised.requiresReconnect === true,
  };
}

export function resolveUdpStabilitySelection(
  selection: UdpStabilitySelection,
  advertised: unknown,
): UdpStabilitySelection | null {
  const options = availableUdpStabilityOptions(advertised);
  if (!options) return null;

  if (selection.profile !== "custom") {
    return options.profiles.includes(selection.profile)
      ? { profile: selection.profile }
      : null;
  }

  const { burstDatagrams, fecParityShards, adaptivePacing } = selection;
  if (
    burstDatagrams === undefined
    || fecParityShards === undefined
    || adaptivePacing === undefined
    || !options.burstDatagrams.includes(burstDatagrams)
    || !options.fecParityShards.includes(fecParityShards)
    || (adaptivePacing && !options.adaptivePacing)
  ) {
    return null;
  }
  return {
    profile: "custom",
    burstDatagrams,
    fecParityShards,
    adaptivePacing,
  };
}
