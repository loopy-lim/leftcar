import { describe, expect, it } from "vitest";
import { colors, typography } from "./tokens";
import { cn } from "./cn";
import { TEXT_VARIANT_TYPE_ROLES } from "./theme-css";

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

describe("TEXT_VARIANT_TYPE_ROLES", () => {
  it("matches the CSS variable scale exactly", () => {
    // 런타임 밀도 배율(viewer primitives)과 생성 CSS(--text-*)가 같은 표를
    // 써야 한다 — 이 표가 어긋나면 클래스와 실제 렌더 크기가 벌어진다.
    expect(TEXT_VARIANT_TYPE_ROLES).toEqual({
      caption: "xs",
      label: "sm",
      body: "base",
      title: "lg",
      subtitle: "xl",
      heading: "xxl",
      code: "xs",
    });
    for (const role of Object.values(TEXT_VARIANT_TYPE_ROLES)) {
      expect(typography.fontSize[role]).toBeGreaterThan(0);
      expect(typography.lineHeight[role]).toBeGreaterThan(0);
    }
  });
});
