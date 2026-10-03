import { describe, expect, it } from "vitest";
import { colors } from "./tokens";
import { cn } from "./cn";

function luminance(hex: string): number {
  const channels = hex.slice(1).match(/.{2}/g)!.map((value) => parseInt(value, 16) / 255);
  const linear = channels.map((value) => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);
  return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
}

function contrast(foreground: string, background: string): number {
  const values = [luminance(foreground), luminance(background)].sort((a, b) => b - a);
  return (values[0] + 0.05) / (values[1] + 0.05);
}

describe("shared readable theme roles", () => {
  for (const [theme, palette] of Object.entries(colors)) {
    for (const role of ["textPrimary", "textSecondary", "textMuted", "textDim"] as const) {
      for (const surface of ["bgCanvas", "bgSurface", "bgSubtle"] as const) {
        it(`${theme} ${role} remains readable on ${surface}`, () => {
          expect(contrast(palette[role], palette[surface])).toBeGreaterThanOrEqual(4.5);
        });
      }
    }
  }
});

it("composes semantic type size with its color without losing either", () => {
  expect(cn("text-caption text-ink", "text-body")).toBe("text-ink text-body");
  expect(cn("text-body", "text-muted")).toBe("text-body text-muted");
});
