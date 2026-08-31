import { StyleSheet } from "react-native";
import type { ThemeTokens } from "@leftcar/ui-tokens";

export function createCatalogStyles(colors: ThemeTokens, isDark: boolean) {
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
      paddingHorizontal: 16,
      paddingTop: 12,
      paddingBottom: 32,
      gap: 10,
    },
    headerContainer: {
      gap: 10,
      marginBottom: 4,
    },
    hostStrip: {
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 10,
      paddingHorizontal: 12,
      paddingVertical: 9,
      flexDirection: "row",
      alignItems: "center",
      justifyContent: "space-between",
    },
    hostStripLeft: {
      flexDirection: "row",
      alignItems: "center",
      gap: 8,
      flex: 1,
      minWidth: 0,
    },
    dotConnected: {
      width: 6,
      height: 6,
      borderRadius: 3,
      backgroundColor: colors.textPrimary,
      flexShrink: 0,
    },
    hostStripText: {
      color: colors.textMuted,
      fontSize: 12,
      flex: 1,
    },
    hostStripAddr: {
      color: colors.textPrimary,
      fontWeight: "700",
      fontFamily: "monospace",
      fontVariant: ["tabular-nums"],
    },
    transportStrip: {
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 10,
      paddingHorizontal: 12,
      paddingVertical: 8,
      flexDirection: "row",
      alignItems: "center",
      gap: 8,
    },
    transportDot: {
      width: 6,
      height: 6,
      borderRadius: 3,
      backgroundColor: colors.textDim,
    },
    transportDotUsb: {
      backgroundColor: colors.textPrimary,
    },
    transportText: {
      color: colors.textSecondary,
      fontSize: 12,
    },
    btnHostChange: {
      backgroundColor: colors.btnSecondaryBg,
      borderWidth: 1,
      borderColor: colors.btnSecondaryBorder,
      paddingHorizontal: 8,
      paddingVertical: 4,
      borderRadius: 6,
    },
    btnHostChangeText: {
      color: colors.btnSecondaryText,
      fontSize: 11,
      fontWeight: "600",
    },

    /* Error Card */
    errorCard: {
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderStrong,
      borderRadius: 10,
      padding: 12,
      flexDirection: "row",
      alignItems: "center",
      gap: 8,
    },
    errorText: {
      color: colors.textPrimary,
      fontSize: 12,
      lineHeight: 16,
    },
    errorBody: {
      flex: 1,
      gap: 6,
    },
    errorActions: {
      flexDirection: "row",
      gap: 8,
    },
    errorRetryBtn: {
      backgroundColor: colors.btnPrimaryBg,
      borderRadius: 6,
      paddingHorizontal: 8,
      paddingVertical: 4,
    },
    errorRetryText: {
      color: colors.btnPrimaryText,
      fontSize: 11,
      fontWeight: "600",
    },
    errorHostBtn: {
      backgroundColor: colors.btnSecondaryBg,
      borderWidth: 1,
      borderColor: colors.btnSecondaryBorder,
      borderRadius: 6,
      paddingHorizontal: 8,
      paddingVertical: 4,
    },
    errorHostText: {
      color: colors.btnSecondaryText,
      fontSize: 11,
      fontWeight: "600",
    },

    /* Segmented Quality */
    qualitySegmentWrapper: {
      backgroundColor: colors.bgSurface,
      borderRadius: 10,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      padding: 3,
    },
    qualitySegmentTabs: {
      flexDirection: "row",
      gap: 3,
    },
    qualityTab: {
      flex: 1,
      paddingVertical: 7,
      paddingHorizontal: 4,
      borderRadius: 7,
      alignItems: "center",
      justifyContent: "center",
      gap: 1,
      backgroundColor: "transparent",
    },
    qualityTabActive: {
      backgroundColor: colors.btnPrimaryBg,
    },
    qualityTabLabel: {
      fontSize: 11,
      fontWeight: "600",
      color: colors.textMuted,
    },
    qualityTabLabelActive: {
      color: colors.btnPrimaryText,
      fontWeight: "700",
    },
    qualityTabDetail: {
      fontSize: 9,
      fontFamily: "monospace",
      fontVariant: ["tabular-nums"],
      color: colors.textDim,
    },
    qualityTabDetailActive: {
      color: colors.btnPrimaryText,
      opacity: 0.8,
    },

    /* Collapsible Advanced Streaming Controls */
    advancedToggleRow: {
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 10,
      paddingHorizontal: 12,
      paddingVertical: 10,
      flexDirection: "row",
      alignItems: "center",
      justifyContent: "space-between",
    },
    advancedToggleLeft: {
      flexDirection: "row",
      alignItems: "center",
      gap: 8,
    },
    advancedToggleText: {
      fontSize: 12,
      fontWeight: "600",
      color: colors.textSecondary,
    },
    reconnectDot: {
      width: 6,
      height: 6,
      borderRadius: 3,
      backgroundColor: colors.textPrimary,
    },
    advancedSectionContainer: {
      gap: 10,
      paddingTop: 2,
    },

    /* Section Title */
    sectionTitleRow: {
      flexDirection: "row",
      alignItems: "center",
      justifyContent: "space-between",
      paddingHorizontal: 2,
      marginTop: 4,
    },
    sectionTitleText: {
      fontSize: 12,
      fontWeight: "700",
      color: colors.textMuted,
      textTransform: "uppercase",
      letterSpacing: 0.04,
    },
    btnRefresh: {
      paddingHorizontal: 6,
      paddingVertical: 2,
    },
    btnRefreshText: {
      color: colors.textPrimary,
      fontSize: 11,
      fontWeight: "600",
    },
    btnDisabled: {
      opacity: 0.5,
    },

    /* Miniature Display Aspect-Ratio Box */
    miniatureBox: {
      width: 44,
      height: 40,
      alignItems: "center",
      justifyContent: "center",
      flexShrink: 0,
    },
    miniatureScreen: {
      borderWidth: 1.5,
      borderColor: colors.textPrimary,
      borderRadius: 3,
      backgroundColor: colors.bgSubtle,
      alignItems: "center",
      justifyContent: "center",
    },
    miniatureInner: {
      width: "70%",
      height: "50%",
      backgroundColor: colors.borderCard,
      borderRadius: 1,
    },
    miniatureStand: {
      width: 3,
      height: 3,
      backgroundColor: colors.textPrimary,
    },
    miniatureBase: {
      width: 14,
      height: 2,
      backgroundColor: colors.textPrimary,
      borderRadius: 1,
    },

    /* Display Cards */
    displayCard: {
      backgroundColor: colors.bgSurface,
      borderRadius: 12,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      padding: 12,
      flexDirection: "row",
      alignItems: "center",
      gap: 12,
    },
    displayMain: {
      flex: 1,
      minWidth: 0,
      gap: 3,
    },
    displayName: {
      fontSize: 13,
      fontWeight: "700",
      color: colors.textPrimary,
    },
    displayRecommendation: {
      fontSize: 10,
      color: colors.textMuted,
    },
    chipsRow: {
      flexDirection: "row",
      alignItems: "center",
      gap: 6,
    },
    chip: {
      backgroundColor: colors.bgSubtle,
      paddingHorizontal: 6,
      paddingVertical: 1,
      borderRadius: 4,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
    },
    chipText: {
      fontSize: 10,
      color: colors.textSecondary,
      fontFamily: "monospace",
      fontVariant: ["tabular-nums"],
    },
    openBtn: {
      backgroundColor: colors.btnPrimaryBg,
      paddingHorizontal: 12,
      paddingVertical: 8,
      borderRadius: 6,
      alignItems: "center",
      justifyContent: "center",
      flexShrink: 0,
    },
    openBtnText: {
      color: colors.btnPrimaryText,
      fontSize: 12,
      fontWeight: "600",
    },

    /* Empty State */
    emptyCard: {
      backgroundColor: colors.bgSurface,
      borderRadius: 12,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      padding: 24,
      alignItems: "center",
      gap: 8,
    },
    loadingText: {
      color: colors.textMuted,
      fontSize: 12,
    },
    emptyText: {
      color: colors.textMuted,
      fontSize: 12,
    },

    /* Active Streams */
    activeSection: {
      marginTop: 8,
      gap: 8,
    },
    activeSectionHeader: {
      flexDirection: "row",
      alignItems: "center",
      gap: 6,
    },
    activeSectionTitle: {
      fontSize: 12,
      fontWeight: "700",
      color: colors.textMuted,
      textTransform: "uppercase",
      letterSpacing: 0.04,
    },
    activeCountBadge: {
      backgroundColor: colors.btnPrimaryBg,
      paddingHorizontal: 6,
      paddingVertical: 1,
      borderRadius: 10,
    },
    activeCountText: {
      color: colors.btnPrimaryText,
      fontSize: 10,
      fontWeight: "700",
      fontFamily: "monospace",
      fontVariant: ["tabular-nums"],
    },
    streamCard: {
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderCard,
      borderRadius: 10,
      padding: 10,
      flexDirection: "row",
      alignItems: "center",
      justifyContent: "space-between",
      gap: 8,
    },
    streamInfo: {
      flex: 1,
      minWidth: 0,
      gap: 2,
    },
    streamNameRow: {
      flexDirection: "row",
      alignItems: "center",
      gap: 6,
    },
    dotActive: {
      width: 6,
      height: 6,
      borderRadius: 3,
      backgroundColor: colors.textPrimary,
    },
    streamName: {
      color: colors.textPrimary,
      fontSize: 12,
      fontWeight: "700",
    },
    streamPort: {
      color: colors.textMuted,
      fontSize: 10,
      fontFamily: "monospace",
      fontVariant: ["tabular-nums"],
    },
    stopBtn: {
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      paddingHorizontal: 8,
      paddingVertical: 4,
      borderRadius: 6,
      flexShrink: 0,
    },
    stopBtnText: {
      color: colors.textPrimary,
      fontSize: 11,
      fontWeight: "600",
    },
    itemPressed: {
      opacity: 0.75,
      transform: [{ scale: 0.98 }],
    },
  });
}
