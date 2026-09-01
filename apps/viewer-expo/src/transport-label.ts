import type { ResolvedTransport } from "./usb";

/**
 * Maps the control contract's media_transport value to a short badge label.
 * Unknown/legacy values fall back to "ADB" because only adbTcp predates the
 * typed transport set.
 */
export function transportBadgeLabel(transport: ResolvedTransport | string): string {
  const normalized = transport.trim().toLowerCase();
  if (normalized === "usb") return "USB";
  if (normalized === "udp") return "Wi-Fi";
  if (normalized === "tcp") return "Wi-Fi (TCP)";
  return "ADB";
}
