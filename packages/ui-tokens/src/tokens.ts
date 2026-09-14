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

    brandPrimary: "#2563EB",
    brandHover: "#1D4ED8",
    brandSubtle: "#EFF6FF",

    statusLive: "#059669",
    statusLiveSubtle: "#ECFDF5",
    statusLiveBorder: "#A7F3D0",
    statusWarning: "#D97706",
    statusWarningSubtle: "#FFFBEB",
    statusDanger: "#DC2626",
    statusDangerSubtle: "#FEF2F2",

    statusDot: "#059669",
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

    brandPrimary: "#3B82F6",
    brandHover: "#2563EB",
    brandSubtle: "rgba(59, 130, 246, 0.15)",

    statusLive: "#10B981",
    statusLiveSubtle: "rgba(16, 185, 129, 0.14)",
    statusLiveBorder: "rgba(16, 185, 129, 0.3)",
    statusWarning: "#F59E0B",
    statusWarningSubtle: "rgba(245, 158, 11, 0.14)",
    statusDanger: "#EF4444",
    statusDangerSubtle: "rgba(239, 68, 68, 0.15)",

    statusDot: "#10B981",
    chipBg: "#1E1E24",
    chipBorder: "rgba(255, 255, 255, 0.12)",
    chipText: "#A1A1AA",
  },
} as const;

export type ThemeMode = "light" | "dark";
export type ThemeTokens = { [K in keyof typeof colors.light]: string };
