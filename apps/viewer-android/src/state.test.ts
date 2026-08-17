import { describe, expect, it } from "vitest";
import { canTransition, parseLaunchHandle, statusLine } from "./state";

describe("viewer UI state model (docs/01 §6)", () => {
  it("follows the documented phase graph", () => {
    expect(canTransition("idle", "negotiating")).toBe(true);
    expect(canTransition("negotiating", "waiting_keyframe")).toBe(true);
    expect(canTransition("waiting_keyframe", "playing")).toBe(true);
    expect(canTransition("suspended", "negotiating")).toBe(true);
    // forbidden shortcuts
    expect(canTransition("idle", "playing")).toBe(false);
    expect(canTransition("suspended", "playing")).toBe(false);
  });

  it("terminal states reachable from any phase", () => {
    for (const from of ["idle", "playing", "suspended"] as const) {
      expect(canTransition(from, "source_unavailable")).toBe(true);
      expect(canTransition(from, "permission_revoked")).toBe(true);
      expect(canTransition(from, "decoder_failed")).toBe(true);
      expect(canTransition(from, "stopped")).toBe(true);
    }
  });

  it("every phase has a human status line without raw codes", () => {
    const phases = [
      "idle", "negotiating", "waiting_keyframe", "playing", "degraded",
      "reconnecting", "suspended", "source_unavailable", "permission_revoked",
      "decoder_failed", "stopped",
    ] as const;
    for (const phase of phases) {
      const { label } = statusLine(phase);
      expect(label.length, `phase ${phase} needs a label`).toBeGreaterThan(0);
      expect(
        /\b[a-z]+\.[a-z_]+\b/.test(label),
        `phase ${phase} leaked an error code: ${label}`,
      ).toBe(false);
    }
  });

  it("launch handles are opaque and validated", () => {
    expect(parseLaunchHandle("leftcar-launch://src-1/instance")).toEqual({
      valid: true,
      sourceId: "src-1",
    });
    expect(parseLaunchHandle("not-a-handle").valid).toBe(false);
    expect(parseLaunchHandle("leftcar-launch://").valid).toBe(false);
    // secrets never ride in the handle
    const parsed = parseLaunchHandle("leftcar-launch://s/token:SECRET");
    expect(JSON.stringify(parsed)).not.toContain("SECRET");
  });
});
