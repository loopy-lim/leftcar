import type { ResolvedTransport } from "./usb";

/**
 * Maps the control contract's media_transport value to a short badge label.
 * Unknown/legacy values (only adbTcp predates the typed transport set) fall
 * back to the generic "Wi-Fi" — raw protocol names are dev jargon on a
 * consumer status chip.
 */
export function transportBadgeLabel(transport: ResolvedTransport | string): string {
  const normalized = transport.trim().toLowerCase();
  if (normalized === "usb") return "USB";
  if (normalized === "udp") return "Wi-Fi";
  if (normalized === "tcp") return "Wi-Fi (TCP)";
  return "Wi-Fi";
}
