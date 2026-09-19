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

    brandPrimary: "#09090B",
    brandHover: "#27272A",
    brandSubtle: "#F4F4F5",

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

    brandPrimary: "#FAFAFA",
    brandHover: "#E4E4E7",
    brandSubtle: "#1E1E24",

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

/**
 * Standardized Open UI Typography Hierarchy
 * Eliminates 10px / 11px micro-fonts for legible, accessible typography across platforms.
 */
export const typography = {
  fontSize: {
    /** 12px: Minimum readable caption, subtle timestamps, tags */
    xs: 12,
    /** 13-14px: Secondary descriptions, hints, table items, compact buttons */
    sm: 13,
    /** 15-16px: Primary body text, form inputs, standard action buttons */
    base: 15,
    /** 16px: Strong body text */
    md: 16,
    /** 18px: Card subtitles, group headers, prominent actions */
    lg: 18,
    /** 22px: Section titles, modal titles */
    xl: 22,
    /** 26px: Hero headings, PIN / OTP display */
    xxl: 26,
  },
  lineHeight: {
    xs: 16,
    sm: 18,
    base: 22,
    md: 24,
    lg: 26,
    xl: 30,
    xxl: 34,
  },
  fontWeight: {
    normal: "400",
    medium: "500",
    semibold: "600",
    bold: "700",
    extrabold: "800",
  },
} as const;

/**
 * Standard 8pt-based Spacing Scale
 */
export const spacing = {
  px: 1,
  0.5: 2,
  1: 4,
  1.5: 6,
  2: 8,
  2.5: 10,
  3: 12,
  3.5: 14,
  4: 16,
  5: 20,
  6: 24,
  8: 32,
  10: 40,
  12: 48,
} as const;

/**
 * Standard Surface and Component Radii
 */
export const radii = {
  none: 0,
  sm: 6,
  md: 10,
  lg: 14,
  xl: 18,
  full: 9999,
} as const;

/**
 * Standardized Touch and Click Hit Targets
 */
export const hitTargets = {
  /** 44x44pt: Apple HIG and Material minimum touch target for mobile */
  mobileMin: 44,
  /** 32x32px: Standard desktop mouse click target for compact controls */
  desktopMin: 32,
} as const;
