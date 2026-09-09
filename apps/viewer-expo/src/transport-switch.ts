import type { ResolvedTransport } from "./usb";
import type { EncoderExperimentId } from "./encoder-experiment";

/**
 * Automatic media switching only owns the UDP <-> AOAP pair. Explicit TCP or
 * ADB-TCP diagnostics must not be rewritten behind the caller's back.
 */
export function shouldSwitchTransport(
  current: string,
  desired: ResolvedTransport,
  encoderExperiment?: EncoderExperimentId,
): boolean {
  if (encoderExperiment === "splitVertical") return false;
  if (current !== "udp" && current !== "usb") return false;
  return desired !== current;
}
