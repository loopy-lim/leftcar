import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  NativeEventEmitter,
  NativeModules,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { Ionicons } from "@expo/vector-icons";
import { SafeAreaView } from "react-native-safe-area-context";
import { router } from "expo-router";
import { connectHost, controlClient, disconnectHost } from "../src/session";
import { formatErrorMessage, isUnauthorizedError, type CatalogView } from "../src/control";
import {
  clearToken,
  formatHostEndpoint,
  getStoredToken,
  isTrustedHost,
  parseHostEndpoint,
} from "../src/pairing";
import { useAppTheme, type ThemeTokens } from "../src/theme";
import { useAppLanguage } from "../src/i18n";

type NsdNative = {
  startDiscovery(): void;
  stopDiscovery(): void;
};

const nsd = NativeModules.NsdDiscovery as NsdNative | undefined;

interface FoundHost {
  name: string;
  host: string;
  port: number;
}

export default function Host() {
  const { colors, isDark } = useAppTheme();
  const { t } = useAppLanguage();
  const styles = useMemo(() => createStyles(colors, isDark), [colors, isDark]);

  const [ip, setIp] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [found, setFound] = useState<Record<string, FoundHost>>({});
  const [hasStoredToken, setHasStoredToken] = useState(false);

  useEffect(() => {
    void getStoredToken().then((token) => setHasStoredToken(!!token));
  }, []);

  const handleClearToken = useCallback(async () => {
    await clearToken();
    disconnectHost();
    setHasStoredToken(false);
    Alert.alert(t.viewer.clearTokenAlertTitle, t.viewer.clearTokenAlertDesc);
  }, [t]);

  useEffect(() => {
    if (!nsd) return;
    const emitter = new NativeEventEmitter(nsd as never);
    const sub1 = emitter.addListener("leftcar:host-found", (raw) => {
      const h = raw as FoundHost;
      setFound((prev) => ({ ...prev, [h.host]: h }));
    });
    const sub2 = emitter.addListener("leftcar:host-lost", (serviceName) => {
      setFound((prev) =>
        Object.fromEntries(
          Object.entries(prev).filter(([, host]) => host.name !== String(serviceName)),
        ),
      );
    });
    nsd.startDiscovery();
    return () => {
      nsd.stopDiscovery();
      sub1.remove();
      sub2.remove();
    };
  }, []);

  useEffect(() => {
    setError(null);
  }, [ip]);

  const doConnect = useCallback(async (target: string, port = 7777) => {
    setBusy(true);
    setError(null);
    try {
      if (!isTrustedHost(target)) {
        throw new Error(t.viewer.trustedHostError);
      }
      let lastError: unknown = null;
      for (let attempt = 0; attempt < 3; attempt += 1) {
        try {
          await connectHost(target, port);
          lastError = null;
          break;
        } catch (e) {
          lastError = e;
          if (attempt < 2) {
            await new Promise((resolve) => setTimeout(resolve, 300 * (attempt + 1)));
          }
        }
      }
      if (lastError) throw lastError;
      try {
        await controlClient()?.request<CatalogView>("getCatalog");
        setHasStoredToken(true);
      } catch (e) {
        if (isUnauthorizedError(e)) {
          await clearToken();
          disconnectHost();
          setHasStoredToken(false);
          Alert.alert(
            t.viewer.pairingRequiredTitle,
            t.viewer.pairingRequiredDesc,
          );
          router.push({
            pathname: "/pairing",
            params: { endpoint: formatHostEndpoint(target, port) },
          });
          return;
        }
        throw e;
      }
      router.push("/catalog");
    } catch (e) {
      setError(formatErrorMessage(e));
    } finally {
      setBusy(false);
    }
  }, [t]);

  const hosts = Object.values(found);

  const connectManual = useCallback(() => {
    const endpoint = parseHostEndpoint(ip);
    if (!endpoint) {
      setError(t.viewer.invalidHostError);
      return;
    }
    void doConnect(endpoint.host, endpoint.port);
  }, [doConnect, ip, t]);

  return (
    <SafeAreaView style={styles.safeArea} edges={["left", "right", "bottom"]}>
      <ScrollView
        style={styles.root}
        contentContainerStyle={styles.content}
        showsVerticalScrollIndicator={false}
      >
        {/* Error Alert */}
        {error && (
          <View style={styles.errorCard}>
            <Ionicons name="alert-circle" size={16} color={colors.textPrimary} />
            <Text style={styles.errorText}>{error}</Text>
          </View>
        )}

        {/* Nearby Auto Discovered Hosts */}
        <View style={styles.sectionCard}>
          <View style={styles.sectionHeaderRow}>
            <Text style={styles.sectionTitle}>{t.viewer.searchTitle}</Text>
            {nsd && (
              <View style={styles.scanningBadge}>
                <ActivityIndicator size="small" color={colors.textPrimary} />
                <Text style={styles.scanningText}>{t.viewer.searching}</Text>
              </View>
            )}
          </View>

          {hosts.length > 0 ? (
            <View style={styles.hostList}>
              {hosts.map((h) => (
                <Pressable
                  key={h.host}
                  style={({ pressed }) => [
                    styles.hostItem,
                    pressed && styles.itemPressed,
                  ]}
                  onPress={() => doConnect(h.host, h.port)}
                  disabled={busy}
                >
                  <View style={styles.hostIconBox}>
                    <Ionicons name="laptop-outline" size={18} color={colors.textPrimary} />
                  </View>
                  <View style={styles.hostInfo}>
                    <Text style={styles.hostName} numberOfLines={1}>
                      {h.name || t.common.myComputer}
                    </Text>
                    <Text style={styles.hostAddr} numberOfLines={1}>
                      {h.port === 7777 ? h.host : `${h.host}:${h.port}`}
                    </Text>
                  </View>
                  <View style={styles.connectChip}>
                    <Text style={styles.connectChipText}>{t.common.connect}</Text>
                  </View>
                </Pressable>
              ))}
            </View>
          ) : (
            <View style={styles.emptyBox}>
              <Ionicons name="wifi-outline" size={24} color={colors.textDim} style={{ marginBottom: 4 }} />
              <Text style={styles.emptyTitle}>{t.viewer.emptyHostsTitle}</Text>
              <Text style={styles.emptyText}>
                {t.viewer.emptyHostsDesc}
              </Text>
            </View>
          )}
        </View>

        {/* Manual IP Entry */}
        <View style={styles.sectionCard}>
          <Text style={styles.sectionTitle}>{t.viewer.manualTitle}</Text>
          <Text style={styles.fieldDesc}>
            {t.viewer.manualDesc}
          </Text>

          <View style={styles.inputRow}>
            <TextInput
              style={styles.textInput}
              placeholder={t.viewer.manualPlaceholder}
              placeholderTextColor={colors.textDim}
              keyboardType="url"
              autoCapitalize="none"
              autoCorrect={false}
              value={ip}
              onChangeText={setIp}
            />
            {ip.length > 0 && (
              <Pressable onPress={() => setIp("")} style={styles.clearBtn} aria-label={t.common.cancel}>
                <Ionicons name="close-circle" size={16} color={colors.textDim} />
              </Pressable>
            )}
          </View>

          {__DEV__ && (
            <View style={styles.quickChipsRow}>
              <Pressable onPress={() => setIp("localhost")} style={styles.quickChip}>
                <Text style={styles.quickChipText}>+ localhost (ADB)</Text>
              </Pressable>
              <Pressable onPress={() => setIp("10.0.2.2")} style={styles.quickChip}>
                <Text style={styles.quickChipText}>+ 10.0.2.2 (에뮬레이터)</Text>
              </Pressable>
            </View>
          )}

          <Pressable
            style={({ pressed }) => [
              styles.primaryBtn,
              (!ip.trim() || busy) && styles.btnDisabled,
              pressed && ip.trim() && !busy && styles.btnPressed,
            ]}
            onPress={connectManual}
            disabled={busy || !ip.trim()}
          >
            {busy ? (
              <ActivityIndicator color={colors.btnPrimaryText} size="small" />
            ) : (
              <Text style={styles.primaryBtnText}>{t.viewer.btnConnectAction}</Text>
            )}
          </Pressable>
        </View>

        {/* Pairing Management */}
        <View style={styles.sectionCard}>
          <Text style={styles.sectionTitle}>{t.viewer.rememberTitle}</Text>
          <Text style={styles.fieldDesc}>
            {hasStoredToken
              ? t.viewer.rememberHasToken
              : t.viewer.rememberNoToken}
          </Text>
          <View style={styles.pairingActionRow}>
            {hasStoredToken && (
              <Pressable
                style={({ pressed }) => [
                  styles.dangerBtn,
                  pressed && styles.btnPressed,
                ]}
                onPress={handleClearToken}
              >
                <Text style={styles.dangerBtnText}>{t.viewer.btnClearToken}</Text>
              </Pressable>
            )}
            <Pressable
              style={({ pressed }) => [
                styles.secondaryBtn,
                pressed && styles.btnPressed,
              ]}
              onPress={() => router.push("/pairing")}
            >
              <Ionicons
                name="qr-code-outline"
                size={14}
                color={colors.textPrimary}
                style={{ marginRight: 4 }}
              />
              <Text style={styles.secondaryBtnText}>{t.viewer.btnNewPair}</Text>
            </Pressable>
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
      flex: 1,
      fontWeight: "500",
    },
    sectionCard: {
      backgroundColor: colors.bgSurface,
      borderRadius: 14,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      padding: 16,
      gap: 12,
    },
    sectionHeaderRow: {
      flexDirection: "row",
      justifyContent: "space-between",
      alignItems: "center",
    },
    sectionTitle: {
      fontSize: 11,
      fontWeight: "700",
      color: colors.textMuted,
      textTransform: "uppercase",
      letterSpacing: 0.04,
    },
    scanningBadge: {
      flexDirection: "row",
      alignItems: "center",
      gap: 6,
    },
    scanningText: {
      color: colors.textPrimary,
      fontSize: 11,
      fontWeight: "600",
    },
    hostList: {
      gap: 8,
    },
    hostItem: {
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 10,
      padding: 12,
      flexDirection: "row",
      alignItems: "center",
      gap: 12,
    },
    hostIconBox: {
      width: 36,
      height: 36,
      borderRadius: 8,
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      alignItems: "center",
      justifyContent: "center",
      flexShrink: 0,
    },
    hostInfo: {
      flex: 1,
      minWidth: 0,
      gap: 2,
    },
    hostName: {
      color: colors.textPrimary,
      fontSize: 13,
      fontWeight: "700",
    },
    hostAddr: {
      color: colors.textMuted,
      fontSize: 11,
      fontFamily: "monospace",
      fontVariant: ["tabular-nums"],
    },
    connectChip: {
      backgroundColor: colors.btnPrimaryBg,
      paddingHorizontal: 12,
      paddingVertical: 6,
      borderRadius: 6,
      flexShrink: 0,
    },
    connectChipText: {
      color: colors.btnPrimaryText,
      fontSize: 12,
      fontWeight: "600",
    },
    emptyBox: {
      padding: 22,
      alignItems: "center",
      justifyContent: "center",
      gap: 4,
    },
    emptyTitle: {
      fontSize: 13,
      fontWeight: "600",
      color: colors.textPrimary,
    },
    emptyText: {
      color: colors.textMuted,
      fontSize: 11,
      textAlign: "center",
      lineHeight: 16,
    },
    fieldDesc: {
      color: colors.textSecondary,
      fontSize: 11,
      lineHeight: 16,
    },
    inputRow: {
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 8,
      paddingHorizontal: 12,
      flexDirection: "row",
      alignItems: "center",
    },
    textInput: {
      color: colors.textPrimary,
      paddingVertical: 10,
      fontSize: 13,
      fontFamily: "monospace",
      fontVariant: ["tabular-nums"],
      flex: 1,
    },
    clearBtn: {
      padding: 4,
    },
    quickChipsRow: {
      flexDirection: "row",
      flexWrap: "wrap",
      gap: 6,
    },
    quickChip: {
      backgroundColor: colors.bgSubtle,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 6,
      paddingHorizontal: 8,
      paddingVertical: 4,
    },
    quickChipText: {
      color: colors.textSecondary,
      fontSize: 11,
      fontFamily: "monospace",
    },
    primaryBtn: {
      backgroundColor: colors.btnPrimaryBg,
      borderRadius: 8,
      paddingVertical: 11,
      alignItems: "center",
      justifyContent: "center",
      marginTop: 2,
    },
    btnDisabled: {
      opacity: 0.4,
    },
    primaryBtnText: {
      color: colors.btnPrimaryText,
      fontSize: 13,
      fontWeight: "600",
    },
    pairingActionRow: {
      flexDirection: "row",
      flexWrap: "wrap",
      gap: 8,
      marginTop: 4,
    },
    dangerBtn: {
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 8,
      paddingVertical: 9,
      paddingHorizontal: 12,
      alignItems: "center",
      justifyContent: "center",
    },
    dangerBtnText: {
      color: colors.textMuted,
      fontSize: 12,
      fontWeight: "600",
    },
    secondaryBtn: {
      backgroundColor: colors.btnSecondaryBg,
      borderWidth: 1,
      borderColor: colors.btnSecondaryBorder,
      borderRadius: 8,
      paddingVertical: 9,
      paddingHorizontal: 12,
      flexDirection: "row",
      alignItems: "center",
      justifyContent: "center",
    },
    secondaryBtnText: {
      color: colors.btnSecondaryText,
      fontSize: 12,
      fontWeight: "600",
    },
    btnPressed: {
      opacity: 0.8,
      transform: [{ scale: 0.98 }],
    },
    itemPressed: {
      opacity: 0.7,
    },
  });
}
