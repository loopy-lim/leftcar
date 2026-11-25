import { describe, expect, it } from "vitest";
import { translations } from "./i18n";
import { renderNshCopy } from "./copy-nsh";
import { colors, radii, spacing, typography } from "./tokens";
import { TEXT_VARIANT_TYPE_ROLES } from "./theme-css";
import { renderNshTheme } from "./theme-nsh";

function nshTheme(): { cssVars: { light: Record<string, string>; dark: Record<string, string> }; radius: string } {
  return JSON.parse(renderNshTheme());
}

function nshCopy(): Record<"ko" | "en", Record<string, Record<string, string>>> {
  return JSON.parse(renderNshCopy().slice(renderNshCopy().indexOf("export const copy = ") + "export const copy = ".length).replace(/ as const;\n$/, ""));
}

describe("NSH theme", () => {
  const theme = nshTheme();

  it("gives every color role both appearances", () => {
    for (const mode of ["light", "dark"] as const) {
      expect(theme.cssVars[mode]).toMatchObject({
        canvas: colors[mode].bgCanvas,
        surface: colors[mode].bgSurface,
        subtle: colors[mode].bgSubtle,
        active: colors[mode].bgActive,
        ink: colors[mode].textPrimary,
        muted: colors[mode].textMuted,
        secondary: colors[mode].textSecondary,
        outline: colors[mode].borderCard,
        strong: colors[mode].borderStrong,
        focus: colors[mode].borderFocus,
        action: colors[mode].btnPrimaryBg,
        "action-ink": colors[mode].btnPrimaryText,
        live: colors[mode].statusLive,
        warning: colors[mode].statusWarning,
        danger: colors[mode].statusDanger,
        "danger-subtle": colors[mode].statusDangerSubtle,
      });
    }
  });

  it("binds the shadcn names nsh's components read to the same roles", () => {
    for (const mode of ["light", "dark"] as const) {
      expect(theme.cssVars[mode]).toMatchObject({
        background: colors[mode].bgCanvas,
        foreground: colors[mode].textPrimary,
        card: colors[mode].bgSurface,
        primary: colors[mode].btnPrimaryBg,
        "primary-foreground": colors[mode].btnPrimaryText,
        "muted-foreground": colors[mode].textMuted,
        destructive: colors[mode].statusDanger,
        border: colors[mode].borderCard,
        input: colors[mode].borderCard,
        ring: colors[mode].borderFocus,
      });
    }
  });

  it("keeps the ink roles the screens name over shadcn's backgrounds of the same name", () => {
    expect(theme.cssVars.light.muted).toBe(colors.light.textMuted);
    expect(theme.cssVars.light.secondary).toBe(colors.light.textSecondary);
  });

  it("carries the text scale and the radii as lengths in points", () => {
    for (const mode of ["light", "dark"] as const) {
      for (const [role, size] of Object.entries(TEXT_VARIANT_TYPE_ROLES)) {
        expect(theme.cssVars[mode][`text-${role}`]).toBe(`${typography.fontSize[size]}px`);
        expect(theme.cssVars[mode][`leading-${role}`]).toBe(`${typography.lineHeight[size]}px`);
      }
      for (const radius of ["sm", "md", "lg", "xl"] as const) {
        expect(theme.cssVars[mode][`radius-${radius}`]).toBe(`${radii[radius]}px`);
      }
    }
    expect(theme.radius).toBe(`${radii.md}px`);
  });

  it("relies on a 4px spacing unit, Tailwind's own", () => {
    expect(spacing[1]).toBe(4);
  });
});

describe("NSH copy", () => {
  it("equals the source text of both languages", () => {
    const copy = nshCopy();
    for (const language of ["ko", "en"] as const) {
      for (const [group, entries] of Object.entries(copy[language])) {
        for (const [key, text] of Object.entries(entries)) {
          expect(text, `${language}.${group}.${key}`).toBe(
            (translations[language][group as keyof typeof translations.ko] as Record<string, string>)[key],
          );
        }
      }
    }
    expect(Object.keys(copy.ko)).toEqual(Object.keys(copy.en));
    expect(copy.ko.viewer.navCatalog).toBe("화면 선택");
    expect(copy.en.viewer.navCatalog).toBe("Select Display");
  });

  it("is a module the NSH app imports", () => {
    expect(renderNshCopy().startsWith("export const copy = {\n")).toBe(true);
    expect(renderNshCopy().endsWith("} as const;\n")).toBe(true);
  });
});
