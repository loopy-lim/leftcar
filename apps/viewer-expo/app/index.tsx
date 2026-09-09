import { useCallback, useMemo, useState } from "react";
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
  connectHost,
  controlClient,
  controlHost,
  disconnectHost,
} from "../src/session";
import {
  markPairingStale,
  markUserDisconnected,
  noteAutoReconnectAttempt,
  shouldAutoReconnectFromGate,
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
    try {
      await connectHost(item.host, item.port);
      try {
        await controlClient()?.request<CatalogView>("getCatalog");
      } catch (e) {
        if (isUnauthorizedError(e)) {
          await handleUnauthorized({
            markStale: true,
            navigate: { endpoint: formatHostEndpoint(item.host, item.port) },
          });
          return;
        }
        throw e;
      }
      void saveRecentHost(item.host, item.port, item.name);
      router.push("/catalog");
    } catch (e) {
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
      try {
        await connectHost(target.host, target.port);
        try {
          await controlClient()?.request<CatalogView>("getCatalog");
        } catch (e) {
          if (isUnauthorizedError(e)) {
            await handleUnauthorized({ markStale: true });
            return;
          }
          throw e;
        }
        void saveRecentHost(target.host, target.port, target.name);
        checkConnection();
      } catch {
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
      const client = controlClient();
      void getRecentHosts().then((hosts) => {
        const target = hosts[0] ?? null;
        setLastHost(target);
        if (!client) void attemptAutoReconnect(target);
      });
      if (client) {
        client.request<CatalogView>("getCatalog").catch((e) => {
          if (isUnauthorizedError(e)) {
            void handleUnauthorized({
              beforeNavigate: checkConnection,
              navigate: { endpoint: controlHost() },
            });
          }
        });
      }
    }, [attemptAutoReconnect, checkConnection])
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

            <Text style={styles.heroTitle}>{t.viewer.connectedHeroTitle}</Text>

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

            <Text style={styles.heroTitle}>
              {autoConnecting ? t.viewer.connectingToHost : t.viewer.standbyHeroTitle}
            </Text>

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
      </ScrollView>
    </SafeAreaView>
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
      width: 38,
      height: 38,
      borderRadius: 9,
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
      gap: 4,
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      paddingHorizontal: 8,
      paddingVertical: 5,
      borderRadius: 7,
    },
    langToggleText: {
      color: colors.textSecondary,
      fontSize: 11,
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
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderCard,
      paddingHorizontal: 8,
      paddingVertical: 3,
      borderRadius: 12,
    },
    dotSuccess: {
      width: 6,
      height: 6,
      borderRadius: 3,
      backgroundColor: colors.textPrimary,
    },
    badgeSuccessText: {
      color: colors.textPrimary,
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
      paddingVertical: 3,
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
      fontSize: 11,
      fontWeight: "600",
    },
    endpointLabel: {
      color: colors.textMuted,
      fontSize: 12,
      fontFamily: "monospace",
      fontVariant: ["tabular-nums"],
      flex: 1,
      textAlign: "right",
    },
    heroTitle: {
      fontSize: 15,
      fontWeight: "700",
      color: colors.textPrimary,
      letterSpacing: -0.2,
    },
    heroActionRow: {
      flexDirection: "row",
      gap: 8,
      marginTop: 4,
    },
    primaryActionBtn: {
      flex: 1,
      backgroundColor: colors.btnPrimaryBg,
      borderRadius: 8,
      paddingVertical: 10,
      alignItems: "center",
      justifyContent: "center",
    },
    primaryActionText: {
      color: colors.btnPrimaryText,
      fontSize: 12,
      fontWeight: "600",
    },
    secondaryActionBtn: {
      backgroundColor: colors.btnSecondaryBg,
      borderWidth: 1,
      borderColor: colors.btnSecondaryBorder,
      borderRadius: 8,
      paddingHorizontal: 12,
      paddingVertical: 10,
      flexDirection: "row",
      alignItems: "center",
      justifyContent: "center",
    },
    secondaryActionText: {
      color: colors.btnSecondaryText,
      fontSize: 12,
      fontWeight: "600",
    },
    disconnectActionBtn: {
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 8,
      paddingHorizontal: 12,
      paddingVertical: 10,
      alignItems: "center",
      justifyContent: "center",
    },
    disconnectActionText: {
      color: colors.textMuted,
      fontSize: 12,
      fontWeight: "600",
    },
    recentQuickStrip: {
      flexDirection: "row",
      alignItems: "center",
      gap: 6,
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 8,
      paddingHorizontal: 10,
      paddingVertical: 7,
    },
    recentQuickColumn: {
      gap: 6,
    },
    recentQuickError: {
      color: colors.textSecondary,
      fontSize: 11,
      lineHeight: 15,
    },
    recentQuickText: {
      color: colors.textSecondary,
      fontSize: 11,
      flex: 1,
    },
    btnPressed: {
      opacity: 0.8,
      transform: [{ scale: 0.98 }],
    },
  });
}
