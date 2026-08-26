import { resolveTransport, type ResolvedTransport, type UsbAccessoryState } from "./usb";

/**
 * Automatic media switching only owns the UDP <-> AOAP pair. Explicit TCP or
 * ADB-TCP diagnostics must not be rewritten behind the caller's back.
 */
export function shouldSwitchTransport(
  current: string,
  usbState: Pick<UsbAccessoryState, "attached">,
): boolean {
  if (current !== "udp" && current !== "usb") return false;
  const desired: ResolvedTransport = resolveTransport(usbState, "auto");
  return desired !== current;
}
