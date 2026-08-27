import { describe, expect, it } from "vitest";
import { encoderDiagnosticsView } from "./encoderDiagnostics";
import type { SessionRow } from "./sessionTypes";

const session = {
  encoderMode: "ave",
  encoderID: "com.apple.videotoolbox.videoencoder.ave.avc",
  encoderHardwareAccelerated: true,
  encoderPreset: "high-speed",
  encoderProfile: "main",
  encoderAppliedProperties: ["HighSpeed", "Quality"],
  encoderUnsupportedProperties: ["SuggestedLookAheadFrameCount"],
  encoderRejectedProperties: ["Quality=-12900"],
  encoderFallbackReason: null,
} as SessionRow;

describe("encoder diagnostics", () => {
  it("keeps the actual AVE identity and property outcomes visible", () => {
    expect(encoderDiagnosticsView(session)).toEqual({
      path: "AVE · 하드웨어",
      identity: "com.apple.videotoolbox.videoencoder.ave.avc",
      configuration: "high-speed · main",
      applied: "HighSpeed, Quality",
      unavailable: "미지원 SuggestedLookAheadFrameCount · 거부 Quality=-12900",
      fallback: null,
    });
  });
});
