import { StyleSheet } from "react-native";
import type { ThemeTokens } from "../theme";

export function createHubStyles(colors: ThemeTokens, _isDark: boolean) {
  return StyleSheet.create({
    safeArea: {
      flex: 1,
      backgroundColor: colors.bgCanvas,
    },
    root: {
      flex: 1,
      backgroundColor: colors.bgCanvas,
    },
    content: {
      paddingHorizontal: 20,
      paddingTop: 12,
      paddingBottom: 36,
      gap: 16,
    },
    // Minimal, unpretentious top header (no tacky giant branding banner)
    topBar: {
      flexDirection: "row",
      alignItems: "center",
      justifyContent: "space-between",
      paddingVertical: 4,
      minHeight: 40,
    },
    topBarLeft: {
      flexDirection: "row",
      alignItems: "center",
      gap: 8,
    },
    topBarDot: {
      width: 7,
      height: 7,
      borderRadius: 4,
      backgroundColor: colors.statusLive,
    },
    topBarTitle: {
      fontSize: 14,
      fontWeight: "700",
      color: colors.textPrimary,
      letterSpacing: -0.2,
    },
    langToggleBtn: {
      flexDirection: "row",
      alignItems: "center",
      gap: 5,
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      paddingHorizontal: 12,
      minHeight: 34,
      borderRadius: 17,
      justifyContent: "center",
    },
    langToggleText: {
      color: colors.textSecondary,
      fontSize: 12,
      fontWeight: "700",
    },

    /* Hero Cards */
    heroCardConnected: {
      backgroundColor: colors.bgSurface,
      borderRadius: 16,
      borderWidth: 1,
      borderColor: colors.borderCard,
      padding: 18,
      gap: 14,
    },
    heroCardStandby: {
      backgroundColor: colors.bgSurface,
      borderRadius: 16,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      padding: 18,
      gap: 14,
    },
    deviceHeaderRow: {
      flexDirection: "row",
      alignItems: "center",
      gap: 12,
    },
    deviceIconBox: {
      width: 44,
      height: 44,
      borderRadius: 12,
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      alignItems: "center",
      justifyContent: "center",
    },
    deviceInfoColumn: {
      flex: 1,
      gap: 3,
    },
    deviceNameRow: {
      flexDirection: "row",
      alignItems: "center",
      gap: 8,
    },
    heroTitle: {
      fontSize: 17,
      fontWeight: "700",
      color: colors.textPrimary,
      letterSpacing: -0.3,
    },
    heroDesc: {
      fontSize: 13,
      color: colors.textSecondary,
      lineHeight: 19,
    },
    badgeSuccess: {
      flexDirection: "row",
      alignItems: "center",
      gap: 5,
      backgroundColor: colors.statusLiveSubtle,
      borderWidth: 1,
      borderColor: colors.statusLiveBorder,
      paddingHorizontal: 8,
      paddingVertical: 3,
      borderRadius: 10,
    },
    dotSuccess: {
      width: 6,
      height: 6,
      borderRadius: 3,
      backgroundColor: colors.statusLive,
    },
    badgeSuccessText: {
      color: colors.statusLive,
      fontSize: 11,
      fontWeight: "700",
    },
    badgeStandby: {
      flexDirection: "row",
      alignItems: "center",
      gap: 6,
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      paddingHorizontal: 8,
      paddingVertical: 4,
      borderRadius: 10,
    },
    dotStandby: {
      width: 6,
      height: 6,
      borderRadius: 3,
      backgroundColor: colors.textDim,
    },
    badgeStandbyText: {
      color: colors.textMuted,
      fontSize: 12,
      fontWeight: "600",
    },
    endpointLabel: {
      color: colors.textMuted,
      fontSize: 12,
      fontFamily: "monospace",
      fontVariant: ["tabular-nums"],
    },
    heroActionRow: {
      flexDirection: "row",
      gap: 8,
      marginTop: 2,
    },
    primaryActionBtn: {
      backgroundColor: colors.btnPrimaryBg,
      borderRadius: 12,
      minHeight: 48,
      paddingHorizontal: 18,
      paddingVertical: 12,
      alignItems: "center",
      justifyContent: "center",
    },
    primaryActionText: {
      color: colors.btnPrimaryText,
      fontSize: 15,
      fontWeight: "700",
    },
    secondaryActionBtn: {
      backgroundColor: colors.btnSecondaryBg,
      borderWidth: 1,
      borderColor: colors.btnSecondaryBorder,
      borderRadius: 10,
      minHeight: 44,
      paddingHorizontal: 14,
      paddingVertical: 10,
      flexDirection: "row",
      alignItems: "center",
      justifyContent: "center",
    },
    secondaryActionText: {
      color: colors.btnSecondaryText,
      fontSize: 13,
      fontWeight: "600",
    },
    disconnectActionBtn: {
      backgroundColor: colors.statusDangerSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 10,
      minHeight: 44,
      paddingHorizontal: 14,
      paddingVertical: 10,
      alignItems: "center",
      justifyContent: "center",
    },
    disconnectActionText: {
      color: colors.statusDanger,
      fontSize: 13,
      fontWeight: "600",
    },

    /* Setup Guide */
    sectionCard: {
      backgroundColor: colors.bgSurface,
      borderRadius: 16,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      padding: 18,
      gap: 14,
    },
    sectionTitle: {
      fontSize: 12,
      fontWeight: "700",
      color: colors.textMuted,
      textTransform: "uppercase",
      letterSpacing: 0.04,
    },
    stepsContainer: {
      gap: 12,
    },
    stepItem: {
      flexDirection: "row",
      alignItems: "center",
      gap: 12,
    },
    stepBadge: {
      width: 26,
      height: 26,
      borderRadius: 13,
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderStrong,
      alignItems: "center",
      justifyContent: "center",
    },
    stepNum: {
      color: colors.textPrimary,
      fontSize: 12,
      fontWeight: "700",
      fontFamily: "monospace",
    },
    stepInfo: {
      flex: 1,
      gap: 1,
    },
    stepName: {
      fontSize: 14,
      fontWeight: "600",
      color: colors.textPrimary,
    },
    stepText: {
      fontSize: 13,
      color: colors.textSecondary,
      lineHeight: 18,
    },
    stepDivider: {
      height: 1,
      backgroundColor: colors.borderSubtle,
      marginLeft: 38,
    },

    /* 2-Column Feature Grid */
    featureGrid: {
      flexDirection: "row",
      gap: 10,
    },
    featureCard: {
      flex: 1,
      backgroundColor: colors.bgSurface,
      borderRadius: 14,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      padding: 14,
      gap: 4,
    },
    featureIconBox: {
      width: 32,
      height: 32,
      borderRadius: 8,
      backgroundColor: colors.bgSubtle,
      alignItems: "center",
      justifyContent: "center",
      marginBottom: 2,
    },
    featureValue: {
      fontSize: 13,
      fontWeight: "700",
      color: colors.textPrimary,
      fontVariant: ["tabular-nums"],
    },
    featureLabel: {
      fontSize: 12,
      color: colors.textSecondary,
      lineHeight: 16,
    },

    recentQuickColumn: {
      gap: 6,
    },
    recentQuickError: {
      color: colors.statusDanger,
      fontSize: 12,
      lineHeight: 16,
    },
    btnPressed: {
      opacity: 0.8,
      transform: [{ scale: 0.98 }],
    },
  });
}

export type HubStyles = ReturnType<typeof createHubStyles>;
