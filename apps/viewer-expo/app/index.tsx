import { useCallback, useMemo, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { Ionicons } from "@expo/vector-icons";
import { SafeAreaView } from "react-native-safe-area-context";
import { router, useFocusEffect } from "expo-router";
import {
  connectHost,
  controlClient,
  controlHost,
  disconnectHost,
} from "../src/session";
import { clearToken, formatHostEndpoint } from "../src/pairing";
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
  const styles = useMemo(() => createStyles(colors, isDark), [colors, isDark]);
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
          await clearToken();
          disconnectHost();
          Alert.alert(t.viewer.pairingRequiredTitle, t.viewer.pairingRequiredDesc);
          router.push({
            pathname: "/pairing",
            params: { endpoint: formatHostEndpoint(item.host, item.port) },
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
  }, [connecting, item, onFinished, t]);

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
  const styles = useMemo(() => createStyles(colors, isDark), [colors, isDark]);

  const [hostAddr, setHostAddr] = useState<string>("");
  const [isConnected, setIsConnected] = useState<boolean>(false);
  const [lastHost, setLastHost] = useState<RecentHostItem | null>(null);

  const checkConnection = useCallback(() => {
    const client = controlClient();
    const addr = controlHost();
    setIsConnected(!!client);
    setHostAddr(addr);
  }, []);

  const handleDisconnect = useCallback(() => {
    disconnectHost();
    checkConnection();
  }, [checkConnection]);

  useFocusEffect(
    useCallback(() => {
      checkConnection();
      void getRecentHosts().then((hosts) => {
        setLastHost(hosts[0] ?? null);
      });
      const client = controlClient();
      if (client) {
        client.request<CatalogView>("getCatalog").catch((e) => {
          if (isUnauthorizedError(e)) {
            void (async () => {
              const endpoint = controlHost();
              await clearToken();
              disconnectHost();
              checkConnection();
              Alert.alert(t.viewer.pairingRequiredTitle, t.viewer.pairingRequiredDesc);
              router.push({
                pathname: "/pairing",
                params: { endpoint },
              });
            })();
          }
        });
      }
    }, [checkConnection, t])
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
              <View style={styles.titleBadgeRow}>
                <Text style={styles.appTitle}>{t.viewer.brandTitle}</Text>
                <View style={styles.versionBadge}>
                  <Text style={styles.versionBadgeText}>{t.viewer.brandBadge}</Text>
                </View>
              </View>
              <Text style={styles.appSubtitle}>{t.viewer.brandSubtitle}</Text>
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
              <Text style={styles.heroDesc}>
                {t.viewer.connectedHeroDesc}
              </Text>
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
                <View style={styles.dotStandby} />
                <Text style={styles.badgeStandbyText}>{t.viewer.standbyBadge}</Text>
              </View>
              <Text style={styles.networkHintText}>{t.viewer.wifiHint}</Text>
            </View>

            <View style={styles.heroBody}>
              <Text style={styles.heroTitle}>{t.viewer.standbyHeroTitle}</Text>
              <Text style={styles.heroDesc}>
                {t.viewer.standbyHeroDesc}
              </Text>
            </View>

            {lastHost && (
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

        {/* 3-Step Setup Guide */}
        <View style={styles.sectionCard}>
          <Text style={styles.sectionTitle}>{t.viewer.guideTitle}</Text>

          <View style={styles.stepsContainer}>
            {/* Step 1 */}
            <View style={styles.stepItem}>
              <View style={styles.stepBadgeColumn}>
                <View style={styles.stepBadge}>
                  <Text style={styles.stepNum}>1</Text>
                </View>
                <View style={styles.stepLine} />
              </View>
              <View style={styles.stepInfo}>
                <Text style={styles.stepName}>{t.viewer.step1Title}</Text>
                <Text style={styles.stepText}>{t.viewer.step1Desc}</Text>
              </View>
            </View>

            {/* Step 2 */}
            <View style={styles.stepItem}>
              <View style={styles.stepBadgeColumn}>
                <View style={styles.stepBadge}>
                  <Text style={styles.stepNum}>2</Text>
                </View>
                <View style={styles.stepLine} />
              </View>
              <View style={styles.stepInfo}>
                <Text style={styles.stepName}>{t.viewer.step2Title}</Text>
                <Text style={styles.stepText}>{t.viewer.step2Desc}</Text>
              </View>
            </View>

            {/* Step 3 */}
            <View style={styles.stepItem}>
              <View style={styles.stepBadgeColumn}>
                <View style={styles.stepBadge}>
                  <Text style={styles.stepNum}>3</Text>
                </View>
              </View>
              <View style={styles.stepInfo}>
                <Text style={styles.stepName}>{t.viewer.step3Title}</Text>
                <Text style={styles.stepText}>{t.viewer.step3Desc}</Text>
              </View>
            </View>
          </View>
        </View>

        {/* Quick Specs Grid (2 Column Clean Layout) */}
        <View style={styles.featureGrid}>
          <View style={styles.featureBox}>
            <View style={styles.featureIconBox}>
              <Ionicons name="flash" size={15} color={colors.textPrimary} />
            </View>
            <Text style={styles.featureValue}>{t.viewer.feature1Title}</Text>
            <Text style={styles.featureLabel}>{t.viewer.feature1Desc}</Text>
          </View>
          <View style={styles.featureBox}>
            <View style={styles.featureIconBox}>
              <Ionicons name="tv" size={15} color={colors.textPrimary} />
            </View>
            <Text style={styles.featureValue}>{t.viewer.feature2Title}</Text>
            <Text style={styles.featureLabel}>{t.viewer.feature2Desc}</Text>
          </View>
        </View>
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
    titleBadgeRow: {
      flexDirection: "row",
      alignItems: "center",
      gap: 6,
    },
    appTitle: {
      color: colors.textPrimary,
      fontSize: 18,
      fontWeight: "700",
      letterSpacing: -0.3,
    },
    versionBadge: {
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      paddingHorizontal: 6,
      paddingVertical: 1,
      borderRadius: 5,
    },
    versionBadgeText: {
      color: colors.textSecondary,
      fontSize: 10,
      fontWeight: "700",
      fontFamily: "monospace",
    },
    appSubtitle: {
      color: colors.textMuted,
      fontSize: 12,
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
    networkHintText: {
      color: colors.textDim,
      fontSize: 11,
    },
    endpointLabel: {
      color: colors.textMuted,
      fontSize: 12,
      fontFamily: "monospace",
      fontVariant: ["tabular-nums"],
      flex: 1,
      textAlign: "right",
    },
    heroBody: {
      gap: 3,
    },
    heroTitle: {
      fontSize: 15,
      fontWeight: "700",
      color: colors.textPrimary,
      letterSpacing: -0.2,
    },
    heroDesc: {
      fontSize: 12,
      color: colors.textSecondary,
      lineHeight: 17,
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

    /* Setup Guide */
    sectionCard: {
      backgroundColor: colors.bgSurface,
      borderRadius: 14,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      padding: 16,
      gap: 14,
    },
    sectionTitle: {
      fontSize: 11,
      fontWeight: "700",
      color: colors.textMuted,
      textTransform: "uppercase",
      letterSpacing: 0.04,
    },
    stepsContainer: {
      gap: 0,
    },
    stepItem: {
      flexDirection: "row",
      alignItems: "flex-start",
      gap: 12,
    },
    stepBadgeColumn: {
      alignItems: "center",
      width: 22,
    },
    stepBadge: {
      width: 22,
      height: 22,
      borderRadius: 11,
      backgroundColor: colors.btnPrimaryBg,
      alignItems: "center",
      justifyContent: "center",
    },
    stepNum: {
      color: colors.btnPrimaryText,
      fontSize: 10,
      fontWeight: "700",
      fontFamily: "monospace",
    },
    stepLine: {
      width: 1.5,
      height: 22,
      backgroundColor: colors.borderSubtle,
      marginVertical: 2,
    },
    stepInfo: {
      flex: 1,
      gap: 2,
      paddingBottom: 10,
    },
    stepName: {
      fontSize: 13,
      fontWeight: "600",
      color: colors.textPrimary,
    },
    stepText: {
      fontSize: 11,
      color: colors.textSecondary,
      lineHeight: 16,
    },

    /* 2-Column Feature Grid */
    featureGrid: {
      flexDirection: "row",
      gap: 10,
    },
    featureBox: {
      flex: 1,
      backgroundColor: colors.bgSurface,
      borderRadius: 14,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      padding: 14,
      gap: 4,
    },
    featureIconBox: {
      width: 28,
      height: 28,
      borderRadius: 7,
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      alignItems: "center",
      justifyContent: "center",
      marginBottom: 2,
    },
    featureValue: {
      fontSize: 12,
      fontWeight: "700",
      color: colors.textPrimary,
      fontVariant: ["tabular-nums"],
    },
    featureLabel: {
      fontSize: 11,
      color: colors.textMuted,
      lineHeight: 15,
    },
  });
}
