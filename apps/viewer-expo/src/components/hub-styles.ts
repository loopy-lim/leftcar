import { StyleSheet } from "react-native";
import { typography, type ThemeTokens } from "../theme";

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
      width: "100%",
      maxWidth: 640,
      alignSelf: "center",
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
      fontSize: typography.fontSize.sm,
      lineHeight: typography.lineHeight.sm,
      fontWeight: "600",
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
      borderRadius: 8,
      justifyContent: "center",
    },
    langToggleText: {
      color: colors.textSecondary,
      fontSize: typography.fontSize.xs,
      lineHeight: typography.lineHeight.xs,
      fontWeight: "600",
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
      fontSize: typography.fontSize.lg,
      lineHeight: typography.lineHeight.lg,
      fontWeight: "600",
      color: colors.textPrimary,
      letterSpacing: -0.3,
    },
    heroDesc: {
      fontSize: typography.fontSize.sm,
      lineHeight: typography.lineHeight.sm,
      color: colors.textSecondary,
    },
    /* State is read from dot + text alone — no pill chrome (design.md G1) */
    badgeSuccess: {
      flexDirection: "row",
      alignItems: "center",
      gap: 5,
    },
    dotSuccess: {
      width: 6,
      height: 6,
      borderRadius: 3,
      backgroundColor: colors.statusLive,
    },
    badgeSuccessText: {
      color: colors.statusLive,
      fontSize: typography.fontSize.xs,
      lineHeight: typography.lineHeight.xs,
      fontWeight: "600",
    },
    badgeStandby: {
      flexDirection: "row",
      alignItems: "center",
      gap: 6,
    },
    dotStandby: {
      width: 6,
      height: 6,
      borderRadius: 3,
      backgroundColor: colors.textDim,
    },
    badgeStandbyText: {
      color: colors.textSecondary,
      fontSize: typography.fontSize.xs,
      lineHeight: typography.lineHeight.xs,
      fontWeight: "600",
    },
    endpointLabel: {
      color: colors.textSecondary,
      fontSize: typography.fontSize.xs,
      lineHeight: typography.lineHeight.xs,
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
      fontSize: typography.fontSize.base,
      lineHeight: typography.lineHeight.base,
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
      fontSize: typography.fontSize.sm,
      lineHeight: typography.lineHeight.sm,
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
      fontSize: typography.fontSize.sm,
      lineHeight: typography.lineHeight.sm,
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
    /* Card title sits above the step names — restored hierarchy (was 12px muted) */
    sectionTitle: {
      fontSize: typography.fontSize.lg,
      lineHeight: typography.lineHeight.lg,
      fontWeight: "600",
      color: colors.textPrimary,
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
      fontSize: typography.fontSize.xs,
      lineHeight: typography.lineHeight.xs,
      fontWeight: "600",
      fontFamily: "monospace",
    },
    stepInfo: {
      flex: 1,
      gap: 1,
    },
    stepName: {
      fontSize: typography.fontSize.base,
      lineHeight: typography.lineHeight.base,
      fontWeight: "600",
      color: colors.textPrimary,
    },
    stepText: {
      fontSize: typography.fontSize.sm,
      lineHeight: typography.lineHeight.sm,
      color: colors.textSecondary,
    },
    stepDivider: {
      height: 1,
      backgroundColor: colors.borderSubtle,
      marginLeft: 38,
    },

    /* Feature definition rows — plain title + one-line desc with hairline dividers
       (was a uniform 2-column card grid, design.md G2) */
    featureList: {
      backgroundColor: colors.bgSurface,
      borderRadius: 16,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      paddingHorizontal: 18,
    },
    featureRow: {
      paddingVertical: 14,
      gap: 3,
    },
    featureRowTitle: {
      fontSize: typography.fontSize.base,
      lineHeight: typography.lineHeight.base,
      fontWeight: "600",
      color: colors.textPrimary,
    },
    featureRowDesc: {
      fontSize: typography.fontSize.sm,
      lineHeight: typography.lineHeight.sm,
      color: colors.textSecondary,
    },
    featureDivider: {
      height: 1,
      backgroundColor: colors.borderSubtle,
    },

    recentQuickColumn: {
      gap: 6,
    },
    recentQuickError: {
      color: colors.statusDanger,
      fontSize: typography.fontSize.xs,
      lineHeight: typography.lineHeight.xs,
    },
    btnPressed: {
      opacity: 0.8,
      transform: [{ scale: 0.98 }],
    },
  });
}

export type HubStyles = ReturnType<typeof createHubStyles>;
