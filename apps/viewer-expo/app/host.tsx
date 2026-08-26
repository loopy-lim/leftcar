import { useCallback, useEffect, useState } from "react";
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
    Alert.alert("연결 승인 삭제", "이 기기에 저장된 컴퓨터 연결 승인을 삭제했습니다.");
  }, []);

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
        throw new Error("신뢰하는 같은 Wi-Fi 또는 Tailscale의 컴퓨터만 연결할 수 있습니다.");
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
            "연결 승인이 필요해요",
            "컴퓨터 화면의 6자리 연결 번호로 연결을 승인해 주세요.",
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
  }, []);

  const hosts = Object.values(found);

  const connectManual = useCallback(() => {
    const endpoint = parseHostEndpoint(ip);
    if (!endpoint) {
      setError("같은 Wi-Fi 또는 Tailscale의 컴퓨터 주소를 확인해 주세요. (예: 192.168.0.10:7777)");
      return;
    }
    void doConnect(endpoint.host, endpoint.port);
  }, [doConnect, ip]);

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
            <Ionicons name="alert-circle" size={16} color="#09090B" />
            <Text style={styles.errorText}>{error}</Text>
          </View>
        )}

        {/* Nearby Auto Discovered Hosts */}
        <View style={styles.sectionCard}>
          <View style={styles.sectionHeaderRow}>
            <Text style={styles.sectionTitle}>주변 컴퓨터 탐색</Text>
            {nsd && (
              <View style={styles.scanningBadge}>
                <ActivityIndicator size="small" color="#09090B" />
                <Text style={styles.scanningText}>mDNS 탐색 중…</Text>
              </View>
            )}
          </View>

          {hosts.length > 0 ? (
            <View style={styles.hostList}>
              {hosts.map((h) => (
                <Pressable
                  key={h.host}
                  style={styles.hostItem}
                  onPress={() => doConnect(h.host, h.port)}
                  disabled={busy}
                >
                  <View style={styles.hostIconBox}>
                    <Ionicons name="laptop-outline" size={18} color="#09090B" />
                  </View>
                  <View style={styles.hostInfo}>
                    <Text style={styles.hostName} numberOfLines={1}>
                      {h.name || "Leftcar Host"}
                    </Text>
                    <Text style={styles.hostAddr} numberOfLines={1}>
                      {h.host}:{h.port}
                    </Text>
                  </View>
                  <View style={styles.connectChip}>
                    <Text style={styles.connectChipText}>연결</Text>
                  </View>
                </Pressable>
              ))}
            </View>
          ) : (
            <View style={styles.emptyBox}>
              <Ionicons name="wifi-outline" size={24} color="#A1A1AA" style={{ marginBottom: 4 }} />
              <Text style={styles.emptyTitle}>Leftcar가 실행 중인 컴퓨터를 찾는 중</Text>
              <Text style={styles.emptyText}>
                컴퓨터에서 Leftcar Host Studio가 열려 있고 동일한 Wi-Fi에 연결되어 있는지 확인하세요.
              </Text>
            </View>
          )}
        </View>

        {/* Manual IP Entry */}
        <View style={styles.sectionCard}>
          <Text style={styles.sectionTitle}>컴퓨터 주소 직접 입력</Text>
          <Text style={styles.fieldDesc}>
            자동으로 찾지 못한 경우 컴퓨터 화면 하단에 표시된 로컬 제어 포트 주소를 입력하세요.
          </Text>

          <View style={styles.inputRow}>
            <TextInput
              style={styles.textInput}
              placeholder="192.168.0.x:7777"
              placeholderTextColor="#A1A1AA"
              keyboardType="url"
              autoCapitalize="none"
              autoCorrect={false}
              value={ip}
              onChangeText={setIp}
            />
            {ip.length > 0 && (
              <Pressable onPress={() => setIp("")} style={styles.clearBtn} aria-label="입력 지우기">
                <Ionicons name="close-circle" size={16} color="#A1A1AA" />
              </Pressable>
            )}
          </View>

          {__DEV__ && (
            <View style={styles.quickChipsRow}>
              <Pressable onPress={() => setIp("localhost:7777")} style={styles.quickChip}>
                <Text style={styles.quickChipText}>+ localhost (ADB reverse)</Text>
              </Pressable>
              <Pressable onPress={() => setIp("10.0.2.2:7777")} style={styles.quickChip}>
                <Text style={styles.quickChipText}>+ 10.0.2.2 (에뮬레이터)</Text>
              </Pressable>
            </View>
          )}

          <Pressable
            style={[styles.primaryBtn, (!ip.trim() || busy) && styles.btnDisabled]}
            onPress={connectManual}
            disabled={busy || !ip.trim()}
          >
            {busy ? (
              <ActivityIndicator color="#FFFFFF" size="small" />
            ) : (
              <Text style={styles.primaryBtnText}>연결하기</Text>
            )}
          </Pressable>
        </View>

        {/* Pairing Management */}
        <View style={styles.sectionCard}>
          <Text style={styles.sectionTitle}>연결 승인 관리</Text>
          <Text style={styles.fieldDesc}>
            {hasStoredToken
              ? "이 기기는 이전에 연결한 컴퓨터의 인증 토큰을 안전하게 기억하고 있습니다."
              : "기억하고 있는 연결 승인이 없습니다. 새 컴퓨터에서 6자리 연결 번호로 승인해 주세요."}
          </Text>
          <View style={styles.pairingActionRow}>
            {hasStoredToken && (
              <Pressable style={styles.dangerBtn} onPress={handleClearToken}>
                <Text style={styles.dangerBtnText}>저장된 승인 토큰 삭제</Text>
              </Pressable>
            )}
            <Pressable
              style={styles.secondaryBtn}
              onPress={() => router.push("/pairing")}
            >
              <Ionicons name="qr-code-outline" size={14} color="#09090B" style={{ marginRight: 4 }} />
              <Text style={styles.secondaryBtnText}>새 연결 승인하기</Text>
            </Pressable>
          </View>
        </View>
      </ScrollView>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  safeArea: {
    flex: 1,
    backgroundColor: "#FAFAFA",
  },
  root: {
    flex: 1,
    backgroundColor: "#FAFAFA",
  },
  content: {
    paddingHorizontal: 18,
    paddingTop: 14,
    paddingBottom: 32,
    gap: 14,
  },
  errorCard: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#A1A1AA",
    borderRadius: 8,
    padding: 12,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  errorText: {
    color: "#09090B",
    fontSize: 12,
    lineHeight: 16,
    flex: 1,
    fontWeight: "500",
  },
  sectionCard: {
    backgroundColor: "#FFFFFF",
    borderRadius: 12,
    borderWidth: 1,
    borderColor: "#E4E4E7",
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
    color: "#71717A",
    textTransform: "uppercase",
    letterSpacing: 0.04,
  },
  scanningBadge: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  scanningText: {
    color: "#09090B",
    fontSize: 11,
    fontWeight: "600",
  },
  hostList: {
    gap: 8,
  },
  hostItem: {
    backgroundColor: "#FAFAFA",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    borderRadius: 8,
    padding: 12,
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
  },
  hostIconBox: {
    width: 34,
    height: 34,
    borderRadius: 8,
    backgroundColor: "#F4F4F5",
    borderWidth: 1,
    borderColor: "#E4E4E7",
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
    color: "#09090B",
    fontSize: 13,
    fontWeight: "700",
  },
  hostAddr: {
    color: "#71717A",
    fontSize: 11,
    fontFamily: "monospace",
    fontVariant: ["tabular-nums"],
  },
  connectChip: {
    backgroundColor: "#09090B",
    paddingHorizontal: 12,
    paddingVertical: 6,
    borderRadius: 6,
    flexShrink: 0,
  },
  connectChipText: {
    color: "#FFFFFF",
    fontSize: 12,
    fontWeight: "600",
  },
  emptyBox: {
    padding: 20,
    alignItems: "center",
    justifyContent: "center",
    gap: 4,
  },
  emptyTitle: {
    fontSize: 13,
    fontWeight: "600",
    color: "#09090B",
  },
  emptyText: {
    color: "#71717A",
    fontSize: 11,
    textAlign: "center",
    lineHeight: 16,
  },
  fieldDesc: {
    color: "#71717A",
    fontSize: 11,
    lineHeight: 16,
  },
  inputRow: {
    backgroundColor: "#FAFAFA",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    borderRadius: 8,
    paddingHorizontal: 12,
    flexDirection: "row",
    alignItems: "center",
  },
  textInput: {
    color: "#09090B",
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
    backgroundColor: "#F4F4F5",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    borderRadius: 6,
    paddingHorizontal: 8,
    paddingVertical: 4,
  },
  quickChipText: {
    color: "#52525B",
    fontSize: 11,
    fontFamily: "monospace",
  },
  primaryBtn: {
    backgroundColor: "#09090B",
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
    color: "#FFFFFF",
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
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    borderRadius: 8,
    paddingVertical: 9,
    paddingHorizontal: 12,
    alignItems: "center",
    justifyContent: "center",
  },
  dangerBtnText: {
    color: "#71717A",
    fontSize: 12,
    fontWeight: "600",
  },
  secondaryBtn: {
    backgroundColor: "#F4F4F5",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    borderRadius: 8,
    paddingVertical: 9,
    paddingHorizontal: 12,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
  },
  secondaryBtnText: {
    color: "#09090B",
    fontSize: 12,
    fontWeight: "600",
  },
});
