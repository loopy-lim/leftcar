/**
 * Leftcar Design System - Ultra-Simple Dark & White Theme Tokens
 * Single source of truth shared across Desktop (Tauri) and Mobile (React Native / Expo).
 */

export const colors = {
  light: {
    bgCanvas: "#FAFAFA",
    bgSurface: "#FFFFFF",
    bgSubtle: "#F4F4F5",
    bgHover: "#E4E4E7",
    bgActive: "#D4D4D8",

    borderSubtle: "#E4E4E7",
    borderCard: "#D4D4D8",
    borderStrong: "#A1A1AA",
    borderFocus: "#18181B",

    btnPrimaryBg: "#09090B",
    btnPrimaryText: "#FFFFFF",
    btnSecondaryBg: "#F4F4F5",
    btnSecondaryText: "#09090B",
    btnSecondaryBorder: "#E4E4E7",

    textPrimary: "#09090B",
    textSecondary: "#52525B",
    textMuted: "#71717A",
    textDim: "#A1A1AA",

    statusDot: "#09090B",
    chipBg: "#F4F4F5",
    chipBorder: "#E4E4E7",
    chipText: "#52525B",
  },
  dark: {
    bgCanvas: "#09090B",
    bgSurface: "#141417",
    bgSubtle: "#1E1E24",
    bgHover: "#2A2A32",
    bgActive: "#383844",

    borderSubtle: "rgba(255, 255, 255, 0.08)",
    borderCard: "rgba(255, 255, 255, 0.14)",
    borderStrong: "rgba(255, 255, 255, 0.25)",
    borderFocus: "#FAFAFA",

    btnPrimaryBg: "#FAFAFA",
    btnPrimaryText: "#09090B",
    btnSecondaryBg: "#1E1E24",
    btnSecondaryText: "#FAFAFA",
    btnSecondaryBorder: "rgba(255, 255, 255, 0.14)",

    textPrimary: "#FAFAFA",
    textSecondary: "#A1A1AA",
    textMuted: "#71717A",
    textDim: "#52525B",

    statusDot: "#FAFAFA",
    chipBg: "#1E1E24",
    chipBorder: "rgba(255, 255, 255, 0.12)",
    chipText: "#A1A1AA",
  },
} as const;

export type ThemeMode = "light" | "dark";
export type ThemeTokens = { [K in keyof typeof colors.light]: string };

export const radii = {
  sm: 4,
  md: 8,
  lg: 12,
  xl: 16,
  full: 9999,
} as const;
