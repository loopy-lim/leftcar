import { useConnectionLost } from "../src/use-connection-lost";
import type { TranslationSchema } from "../src/i18n";
import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
  useWindowDimensions,
} from "react-native";
import { Ionicons } from "@expo/vector-icons";
import { SafeAreaView } from "react-native-safe-area-context";
import { router, useFocusEffect } from "expo-router";
import { applyPanelDensity, panelDensityScale } from "../src/panel-density";
import {
  beginHostSelection,
  captureRequestContext,
  connectHost,
  controlClient,
  controlHost,
  disconnectHost,
  isHostSelectionCurrent,
  isRequestContextCurrent,
  reconnectHost,
  type SessionRequestContext,
} from "../src/session";
import {
  markPairingStale,
  markUserDisconnected,
  noteAutoReconnectAttempt,
  shouldAutoReconnectFromGate,
  shouldReconnectRetainedContext,
} from "../src/auto-reconnect";
import { handleUnauthorized } from "../src/connect-flow";
import { formatHostEndpoint } from "../src/pairing";
import {
  formatErrorMessage,
  isUnauthorizedError,
  type CatalogView,
} from "../src/control";
import {
  getRecentHosts,
  saveRecentHost,
  type RecentHostItem,
} from "../src/recent-hosts";
import { useAppTheme, type ThemeTokens } from "../src/theme";
import { useAppLanguage } from "../src/i18n";

function openCatalog() {
  router.push("/catalog");
}

function openHostPicker() {
  router.push("/host");
}

function openPairing() {
  router.push("/pairing");
}

/**
 * 대기 화면의 최근 컴퓨터 원탭 재연결 띠. 연결 진행/실패 상태를 스스로
 * 소유하고, 승인 만료(401)면 페어링 화면으로 안내한다.
 */
function RecentHostQuickConnect({
  item,
  onFinished,
}: {
  item: RecentHostItem;
  onFinished: () => void;
}) {
  const { colors, isDark } = useAppTheme();
  const { t } = useAppLanguage();
  const { width } = useWindowDimensions();
  const density = panelDensityScale(width);
  const styles = useMemo(
    () => applyPanelDensity(createStyles(colors, isDark), density),
    [colors, isDark, density],
  );
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleConnect = useCallback(async () => {
    if (connecting) return;
    setConnecting(true);
    setError(null);
    const selection = beginHostSelection();
    let context: SessionRequestContext | null = null;
    try {
      await connectHost(item.host, item.port, { selection });
      if (!isHostSelectionCurrent(selection)) return;
      context = captureRequestContext();
      if (!context || !isRequestContextCurrent(context)) return;
      try {
        await context.client.request<CatalogView>("getCatalog");
      } catch (e) {
        if (isUnauthorizedError(e)) {
          await handleUnauthorized({
            context,
            markStale: true,
            navigate: { endpoint: formatHostEndpoint(item.host, item.port) },
          });
          return;
        }
        throw e;
      }
      if (!isRequestContextCurrent(context)) return;
      void saveRecentHost(item.host, item.port, item.name);
      router.push("/catalog");
    } catch (e) {
      if (!isHostSelectionCurrent(selection)) return;
      if (context) disconnectHost(context);
      setError(formatErrorMessage(e));
    } finally {
      setConnecting(false);
      onFinished();
    }
  }, [connecting, item, onFinished]);

  return (
    <View style={styles.recentQuickColumn}>
      <Pressable
        onPress={() => void handleConnect()}
        disabled={connecting}
        style={({ pressed }) => [
          styles.recentQuickStrip,
          pressed && !connecting && styles.btnPressed,
        ]}
        accessibilityRole="button"
        accessibilityLabel={`${t.viewer.recentHostsTitle}: ${item.name || item.host}`}
      >
        {connecting ? (
          <ActivityIndicator size="small" color={colors.textSecondary} />
        ) : (
          <Ionicons name="time-outline" size={13} color={colors.textSecondary} />
        )}
        <Text style={styles.recentQuickText} numberOfLines={1}>
          {connecting ? (
            t.viewer.connectingToHost
          ) : (
            <>
              {t.viewer.recentHostsTitle}:{" "}
              <Text style={{ fontWeight: "700", color: colors.textPrimary }}>
                {item.name || item.host}
              </Text>
            </>
          )}
        </Text>
        <Ionicons name="chevron-forward" size={13} color={colors.textDim} />
      </Pressable>
      {error ? <Text style={styles.recentQuickError}>{error}</Text> : null}
    </View>
  );
}

export default function Hub() {
  const { colors, isDark } = useAppTheme();
  const { t, language, toggleLanguage } = useAppLanguage();
  const { width } = useWindowDimensions();
  const density = panelDensityScale(width);
  const styles = useMemo(
    () => applyPanelDensity(createStyles(colors, isDark), density),
    [colors, isDark, density],
  );

  const [hostAddr, setHostAddr] = useState<string>("");
  const [isConnected, setIsConnected] = useState<boolean>(false);
  const [lastHost, setLastHost] = useState<RecentHostItem | null>(null);
  const [autoConnecting, setAutoConnecting] = useState<boolean>(false);

  const checkConnection = useCallback(() => {
    const client = controlClient();
    const addr = controlHost();
    setIsConnected(!!client);
    setHostAddr(addr);
  }, []);

  /**
   * 조용한 백그라운드 재연결. 저장된 연결이 있으면 홈 화면 진입 시 스스로
   * 다시 연결해 "계속 연결됨" 상태를 유지한다. 실패는 알리지 않고 대기
   * 상태와 원탭 띠로 안내한다 — 사용자가 직접 해제했거나(userDisconnected),
   * 자동 시도 중 승인 만료(401)를 만났다면(pairingStale) 더 시도하지 않는다.
   */
  const attemptAutoReconnect = useCallback(
    async (target: RecentHostItem | null) => {
      const now = Date.now();
      if (
        target === null ||
        !shouldAutoReconnectFromGate(!!controlClient(), true, now)
      ) {
        return;
      }
      noteAutoReconnectAttempt(now);
      setAutoConnecting(true);
      let context: SessionRequestContext | null = null;
      try {
        // 같은 호스트 재연결은 남은 컨텍스트로 되살린다(세대 무효화 방지 —
        // shouldReconnectRetainedContext 주석). 새 대상만 선택 세대를 만든다.
        const retained = captureRequestContext();
        const useRetained =
          retained !== null && isRequestContextCurrent(retained) &&
          shouldReconnectRetainedContext(retained, target);
        if (useRetained && retained) {
          await reconnectHost(retained);
        } else {
          const selection = beginHostSelection();
          await connectHost(target.host, target.port, { selection });
          if (!isHostSelectionCurrent(selection)) return;
        }
        context = captureRequestContext();
        if (!context || !isRequestContextCurrent(context)) return;
        try {
          await context.client.request<CatalogView>("getCatalog");
        } catch (e) {
          if (isUnauthorizedError(e)) {
            await handleUnauthorized({ context, markStale: true, beforeNavigate: checkConnection });
            return;
          }
          throw e;
        }
        if (!isRequestContextCurrent(context)) return;
        void saveRecentHost(target.host, target.port, target.name);
        checkConnection();
      } catch {
        if (context && disconnectHost(context)) checkConnection();
        // 네트워크 실패는 조용히 넘긴다. 대기 화면의 원탭 띠가 재시도 경로.
      } finally {
        setAutoConnecting(false);
      }
    },
    [checkConnection],
  );

  const handleDisconnect = useCallback(() => {
    markUserDisconnected();
    disconnectHost();
    checkConnection();
  }, [checkConnection]);

  useFocusEffect(
    useCallback(() => {
      checkConnection();
      void getRecentHosts().then((hosts) => {
        const target = hosts[0] ?? null;
        setLastHost(target);
        if (!controlClient()) void attemptAutoReconnect(target);
      });
      const client = controlClient();
      if (client) {
        const context = captureRequestContext();
        context?.client.request<CatalogView>("getCatalog").catch((e) => {
          if (isUnauthorizedError(e)) {
            void handleUnauthorized({
              context,
              beforeNavigate: checkConnection,
              navigate: { endpoint: controlHost() },
            });
          } else if (disconnectHost(context)) {
            checkConnection();
          }
        });
      }
    }, [attemptAutoReconnect, checkConnection])
  );

  // 연결 상태 통지: 소켓이 저절로 닫혀도 배지가 즉시 "연결 안 됨"으로 바뀌고,
  // 저장된 최근 호스트로 조용한 재연결을 시도한다(간격은 게이트가 흡수).
  useConnectionLost(
    useCallback((target: RecentHostItem | null) => {
      setLastHost(target);
      if (!controlClient()) void attemptAutoReconnect(target);
    }, [attemptAutoReconnect]),
    checkConnection,
  );

  return (
    <SafeAreaView style={styles.safeArea} edges={["top", "left", "right", "bottom"]}>
      <ScrollView
        style={styles.root}
        contentContainerStyle={styles.content}
        showsVerticalScrollIndicator={false}
      >
        {/* Top App Header */}
        <View style={styles.brandHeader}>
          <View style={styles.logoRow}>
            <View style={styles.logoBadge}>
              <Ionicons name="desktop-outline" size={18} color={colors.btnPrimaryText} />
            </View>
            <View style={styles.titleColumn}>
              <Text style={styles.appTitle}>{t.viewer.brandTitle}</Text>
            </View>
            <Pressable
              onPress={toggleLanguage}
              style={({ pressed }) => [styles.langToggleBtn, pressed && styles.btnPressed]}
              accessibilityRole="button"
              accessibilityLabel={t.common.toggleLanguage}
            >
              <Ionicons name="globe-outline" size={13} color={colors.textSecondary} />
              <Text style={styles.langToggleText}>{language === "ko" ? "EN" : "한국어"}</Text>
            </Pressable>
          </View>
        </View>

        {/* Hero Connection Card */}
        {isConnected ? (
          <View style={styles.heroCardConnected}>
            <View style={styles.cardTopRow}>
              <View style={styles.badgeSuccess}>
                <View style={styles.dotSuccess} />
                <Text style={styles.badgeSuccessText}>{t.viewer.connectedBadge}</Text>
              </View>
              <Text style={styles.endpointLabel} numberOfLines={1}>
                {hostAddr}
              </Text>
            </View>

            <View style={styles.heroBody}>
              <Text style={styles.heroTitle}>{t.viewer.connectedHeroTitle}</Text>
              <Text style={styles.heroDesc}>{t.viewer.connectedHeroDesc}</Text>
            </View>

            <View style={styles.heroActionRow}>
              <Pressable
                onPress={openCatalog}
                style={({ pressed }) => [
                  styles.primaryActionBtn,
                  pressed && styles.btnPressed,
                ]}
              >
                <Text style={styles.primaryActionText}>{t.viewer.btnViewDisplays}</Text>
              </Pressable>
              <Pressable
                onPress={openHostPicker}
                style={({ pressed }) => [
                  styles.secondaryActionBtn,
                  pressed && styles.btnPressed,
                ]}
              >
                <Text style={styles.secondaryActionText}>{t.viewer.btnChangeHost}</Text>
              </Pressable>
              <Pressable
                onPress={handleDisconnect}
                style={({ pressed }) => [
                  styles.disconnectActionBtn,
                  pressed && styles.btnPressed,
                ]}
              >
                <Text style={styles.disconnectActionText}>{t.common.disconnect}</Text>
              </Pressable>
            </View>
          </View>
        ) : (
          <View style={styles.heroCardStandby}>
            <View style={styles.cardTopRow}>
              <View style={styles.badgeStandby}>
                {autoConnecting ? null : <View style={styles.dotStandby} />}
                {autoConnecting ? (
                  <ActivityIndicator size="small" color={colors.textSecondary} />
                ) : null}
                <Text style={styles.badgeStandbyText}>
                  {autoConnecting ? t.viewer.connectingToHost : t.viewer.standbyBadge}
                </Text>
              </View>
            </View>

            <View style={styles.heroBody}>
              <Text style={styles.heroTitle}>
                {autoConnecting ? t.viewer.connectingToHost : t.viewer.standbyHeroTitle}
              </Text>
              <Text style={styles.heroDesc}>{t.viewer.standbyHeroDesc}</Text>
            </View>

            {lastHost && !autoConnecting && (
              <RecentHostQuickConnect item={lastHost} onFinished={checkConnection} />
            )}

            <View style={styles.heroActionRow}>
              <Pressable
                onPress={openHostPicker}
                style={({ pressed }) => [
                  styles.primaryActionBtn,
                  pressed && styles.btnPressed,
                ]}
              >
                <Text style={styles.primaryActionText}>{t.viewer.btnFindHost}</Text>
              </Pressable>
              <Pressable
                onPress={openPairing}
                style={({ pressed }) => [
                  styles.secondaryActionBtn,
                  pressed && styles.btnPressed,
                ]}
              >
                <Ionicons
                  name="qr-code-outline"
                  size={14}
                  color={colors.textPrimary}
                  style={{ marginRight: 4 }}
                />
                <Text style={styles.secondaryActionText}>{t.viewer.btnQrConnect}</Text>
              </Pressable>
            </View>
          </View>
        )}

        <SetupGuideCard styles={styles} t={t} />

        {/* 2-Column Feature Grid */}
        <View style={styles.featureGrid}>
          <View style={styles.featureCard}>
            <View style={styles.featureIconBox}>
              <Ionicons name="speedometer-outline" size={16} color={colors.textPrimary} />
            </View>
            <Text style={styles.featureValue}>{t.viewer.feature1Title}</Text>
            <Text style={styles.featureLabel}>{t.viewer.feature1Desc}</Text>
          </View>
          <View style={styles.featureCard}>
            <View style={styles.featureIconBox}>
              <Ionicons name="copy-outline" size={16} color={colors.textPrimary} />
            </View>
            <Text style={styles.featureValue}>{t.viewer.feature2Title}</Text>
            <Text style={styles.featureLabel}>{t.viewer.feature2Desc}</Text>
          </View>
        </View>
      </ScrollView>
    </SafeAreaView>
  );
}


function SetupGuideCard({
  styles,
  t,
}: {
  styles: ReturnType<typeof createStyles>;
  t: TranslationSchema;
}) {
  return (
    <View style={styles.sectionCard}>
      <Text style={styles.sectionTitle}>{t.viewer.guideTitle}</Text>

      <View style={styles.stepsContainer}>
        {([
          [1, t.viewer.step1Title, t.viewer.step1Desc],
          [2, t.viewer.step2Title, t.viewer.step2Desc],
          [3, t.viewer.step3Title, t.viewer.step3Desc],
        ] as const).map(([num, name, desc], index, steps) => (
          <Fragment key={num}>
            <View style={styles.stepItem}>
              <View style={styles.stepBadge}>
                <Text style={styles.stepNum}>{num}</Text>
              </View>
              <View style={styles.stepInfo}>
                <Text style={styles.stepName}>{name}</Text>
                <Text style={styles.stepText}>{desc}</Text>
              </View>
            </View>
            {index < steps.length - 1 && <View style={styles.stepDivider} />}
          </Fragment>
        ))}
      </View>
    </View>
  );
}

function createStyles(colors: ThemeTokens, isDark: boolean) {
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
      paddingHorizontal: 18,
      paddingTop: 14,
      paddingBottom: 32,
      gap: 14,
    },
    brandHeader: {
      paddingVertical: 6,
    },
    logoRow: {
      flexDirection: "row",
      alignItems: "center",
      gap: 12,
    },
    logoBadge: {
      width: 40,
      height: 40,
      borderRadius: 10,
      backgroundColor: colors.btnPrimaryBg,
      alignItems: "center",
      justifyContent: "center",
    },
    titleColumn: {
      flex: 1,
      gap: 2,
    },
    appTitle: {
      color: colors.textPrimary,
      fontSize: 18,
      fontWeight: "700",
      letterSpacing: -0.3,
    },
    langToggleBtn: {
      flexDirection: "row",
      alignItems: "center",
      gap: 5,
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      paddingHorizontal: 12,
      minHeight: 36,
      borderRadius: 8,
      justifyContent: "center",
    },
    langToggleText: {
      color: colors.textSecondary,
      fontSize: 13,
      fontWeight: "700",
    },

    /* Hero Cards */
    heroCardConnected: {
      backgroundColor: colors.bgSurface,
      borderRadius: 14,
      borderWidth: 1,
      borderColor: colors.borderCard,
      padding: 16,
      gap: 12,
    },
    heroCardStandby: {
      backgroundColor: colors.bgSurface,
      borderRadius: 14,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      padding: 16,
      gap: 12,
    },
    cardTopRow: {
      flexDirection: "row",
      alignItems: "center",
      justifyContent: "space-between",
      gap: 8,
    },
    badgeSuccess: {
      flexDirection: "row",
      alignItems: "center",
      gap: 6,
      backgroundColor: colors.statusLiveSubtle,
      borderWidth: 1,
      borderColor: colors.statusLiveBorder,
      paddingHorizontal: 8,
      paddingVertical: 4,
      borderRadius: 12,
    },
    dotSuccess: {
      width: 6,
      height: 6,
      borderRadius: 3,
      backgroundColor: colors.statusLive,
    },
    badgeSuccessText: {
      color: colors.statusLive,
      fontSize: 12,
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
      borderRadius: 12,
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
      fontSize: 13,
      fontFamily: "monospace",
      fontVariant: ["tabular-nums"],
      flex: 1,
      textAlign: "right",
    },
    heroBody: {
      gap: 3,
    },
    heroTitle: {
      fontSize: 16,
      fontWeight: "700",
      color: colors.textPrimary,
      letterSpacing: -0.2,
    },
    heroDesc: {
      fontSize: 13,
      color: colors.textSecondary,
      lineHeight: 18,
    },
    heroActionRow: {
      flexDirection: "row",
      gap: 8,
      marginTop: 4,
    },
    primaryActionBtn: {
      flex: 1,
      backgroundColor: colors.btnPrimaryBg,
      borderRadius: 10,
      minHeight: 44,
      paddingHorizontal: 16,
      paddingVertical: 12,
      alignItems: "center",
      justifyContent: "center",
    },
    primaryActionText: {
      color: colors.btnPrimaryText,
      fontSize: 14,
      fontWeight: "600",
    },
    secondaryActionBtn: {
      backgroundColor: colors.btnSecondaryBg,
      borderWidth: 1,
      borderColor: colors.btnSecondaryBorder,
      borderRadius: 10,
      minHeight: 44,
      paddingHorizontal: 16,
      paddingVertical: 12,
      flexDirection: "row",
      alignItems: "center",
      justifyContent: "center",
    },
    secondaryActionText: {
      color: colors.btnSecondaryText,
      fontSize: 14,
      fontWeight: "600",
    },
    disconnectActionBtn: {
      backgroundColor: colors.statusDangerSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 10,
      minHeight: 44,
      paddingHorizontal: 16,
      paddingVertical: 12,
      alignItems: "center",
      justifyContent: "center",
    },
    disconnectActionText: {
      color: colors.statusDanger,
      fontSize: 14,
      fontWeight: "600",
    },

    /* Setup Guide */
    sectionCard: {
      backgroundColor: colors.bgSurface,
      borderRadius: 14,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      padding: 16,
      gap: 12,
    },
    sectionTitle: {
      fontSize: 12,
      fontWeight: "700",
      color: colors.textMuted,
      textTransform: "uppercase",
      letterSpacing: 0.04,
    },
    stepsContainer: {
      gap: 10,
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
      borderRadius: 12,
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

    recentQuickStrip: {
      flexDirection: "row",
      alignItems: "center",
      gap: 8,
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 10,
      paddingHorizontal: 12,
      minHeight: 44,
    },
    recentQuickColumn: {
      gap: 6,
    },
    recentQuickError: {
      color: colors.statusDanger,
      fontSize: 12,
      lineHeight: 16,
    },
    recentQuickText: {
      color: colors.textSecondary,
      fontSize: 13,
      flex: 1,
    },
    btnPressed: {
      opacity: 0.8,
      transform: [{ scale: 0.98 }],
    },
  });
}
