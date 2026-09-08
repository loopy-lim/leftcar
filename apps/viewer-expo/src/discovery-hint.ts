// "안내는 실패 지점에서 한 번" — the empty-discovery hint only appears after the
// picker has been open long enough for NSD to plausibly find hosts, so it never
// flashes while discovery is still warming up.
export const DISCOVERY_HINT_DELAY_MS = 4000;

export interface DiscoveryHintParams {
  hostCount: number;
  elapsedMs: number;
  delayMs?: number;
}

export function shouldShowDiscoveryHint({
  hostCount,
  elapsedMs,
  delayMs = DISCOVERY_HINT_DELAY_MS,
}: DiscoveryHintParams): boolean {
  return hostCount === 0 && elapsedMs >= delayMs;
}
