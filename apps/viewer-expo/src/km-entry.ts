import type { KmNativeStatus } from "./km-session";
export function shouldShowKmEntry(connected: boolean, state: KmNativeStatus["state"] | null): boolean {
  return connected || (state !== null && state !== "off");
}
