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
import { connectHost, controlClient, controlHost, disconnectHost } from "../src/session";
import { isUnauthorizedError, type CatalogView } from "../src/control";
import {
  clearToken,
  formatHostEndpoint,
  getStoredToken,
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
    Alert.alert("페어링 정보 초기화", "이 기기에 저장된 인증 토큰이 삭제되었습니다.");
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
            "기기 페어링 필요",
            "호스트 컴퓨터의 QR 코드 또는 인증 코드를 입력하여 페어링하세요.",
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
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  }, []);

  const hosts = Object.values(found);

  const connectManual = useCallback(() => {
    const endpoint = parseHostEndpoint(ip);
    if (!endpoint) {
      setError("IP 주소 형식을 확인해 주세요. (예: 192.168.0.10:7777)");
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
            <Ionicons name="alert-circle-outline" size={16} color="#DC2626" />
            <Text style={styles.errorText}>{error}</Text>
          </View>
        )}

        {/* Nearby Auto Discovered Hosts */}
        <View style={styles.sectionCard}>
          <View style={styles.sectionHeaderRow}>
            <Text style={styles.sectionTitle}>주변 기기 검색</Text>
            {nsd && (
              <View style={styles.scanningBadge}>
                <ActivityIndicator size="small" color="#2563EB" />
                <Text style={styles.scanningText}>검색 중…</Text>
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
                    <Ionicons name="laptop-outline" size={20} color="#2563EB" />
                  </View>
                  <View style={styles.hostInfo}>
                    <Text style={styles.hostName} numberOfLines={1}>
                      {h.name || "내 컴퓨터"}
                    </Text>
                    <Text style={styles.hostAddr} numberOfLines={1}>
                      {h.host}
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
              <Ionicons name="wifi-outline" size={28} color="#94A3B8" style={{ marginBottom: 4 }} />
              <Text style={styles.emptyTitle}>주변의 Leftcar Host를 찾는 중</Text>
              <Text style={styles.emptyText}>
                컴퓨터에서 Leftcar Host 앱이 켜져 있고 같은 Wi-Fi에 연결되어 있는지 확인하세요.
              </Text>
            </View>
          )}
        </View>

        {/* Manual IP Entry */}
        <View style={styles.sectionCard}>
          <Text style={styles.sectionTitle}>IP 주소 직접 입력</Text>
          <Text style={styles.fieldDesc}>
            자동 검색이 되지 않는 경우 컴퓨터의 로컬 IP를 직접 입력합니다.
          </Text>

          <View style={styles.inputRow}>
            <TextInput
              style={styles.textInput}
              placeholder="192.168.0.x:7777"
              placeholderTextColor="#94A3B8"
              keyboardType="url"
              autoCapitalize="none"
              autoCorrect={false}
              value={ip}
              onChangeText={setIp}
            />
            {ip.length > 0 && (
              <Pressable onPress={() => setIp("")} style={styles.clearBtn}>
                <Ionicons name="close" size={16} color="#94A3B8" />
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
          <Text style={styles.sectionTitle}>페어링 관리</Text>
          <Text style={styles.fieldDesc}>
            {hasStoredToken
              ? "이 기기에 호스트 인증 토큰이 저장되어 있습니다."
              : "저장된 인증 토큰이 없습니다. 새 컴퓨터와 연결하려면 페어링을 진행하세요."}
          </Text>
          <View style={styles.pairingActionRow}>
            {hasStoredToken && (
              <Pressable style={styles.dangerBtn} onPress={handleClearToken}>
                <Text style={styles.dangerBtnText}>저장된 페어링 정보 삭제</Text>
              </Pressable>
            )}
            <Pressable
              style={styles.secondaryBtn}
              onPress={() => router.push("/pairing")}
            >
              <Text style={styles.secondaryBtnText}>QR / 코드 페어링 열기</Text>
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
    backgroundColor: "#F8FAFC",
  },
  root: {
    flex: 1,
    backgroundColor: "#F8FAFC",
  },
  content: {
    paddingHorizontal: 16,
    paddingTop: 14,
    paddingBottom: 32,
    gap: 14,
  },
  errorCard: {
    backgroundColor: "#FEF2F2",
    borderWidth: 1,
    borderColor: "#FECACA",
    borderRadius: 8,
    padding: 10,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  errorText: {
    color: "#DC2626",
    fontSize: 12,
    lineHeight: 16,
    flex: 1,
  },
  sectionCard: {
    backgroundColor: "#FFFFFF",
    borderRadius: 14,
    borderWidth: 1,
    borderColor: "#E2E8F0",
    padding: 16,
    gap: 12,
    boxShadow: "0 1px 3px rgba(0, 0, 0, 0.03)",
  },
  sectionHeaderRow: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "center",
  },
  sectionTitle: {
    fontSize: 14,
    fontWeight: "600",
    color: "#0F172A",
  },
  scanningBadge: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  scanningText: {
    color: "#2563EB",
    fontSize: 12,
    fontWeight: "500",
  },
  hostList: {
    gap: 8,
  },
  hostItem: {
    backgroundColor: "#F8FAFC",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    borderRadius: 10,
    padding: 12,
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
  },
  hostIconBox: {
    width: 36,
    height: 36,
    borderRadius: 8,
    backgroundColor: "#EFF6FF",
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
  },
  hostIcon: {
    fontSize: 18,
  },
  hostInfo: {
    flex: 1,
    minWidth: 0,
    gap: 2,
  },
  hostName: {
    color: "#0F172A",
    fontSize: 13,
    fontWeight: "600",
  },
  hostAddr: {
    color: "#64748B",
    fontSize: 11,
    fontFamily: "monospace",
  },
  connectChip: {
    backgroundColor: "#2563EB",
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
    padding: 24,
    alignItems: "center",
    justifyContent: "center",
    gap: 6,
  },
  emptyIcon: {
    fontSize: 26,
    marginBottom: 2,
  },
  emptyTitle: {
    fontSize: 13,
    fontWeight: "600",
    color: "#0F172A",
  },
  emptyText: {
    color: "#64748B",
    fontSize: 12,
    textAlign: "center",
    lineHeight: 17,
  },
  fieldDesc: {
    color: "#64748B",
    fontSize: 12,
    lineHeight: 16,
  },
  inputRow: {
    backgroundColor: "#F8FAFC",
    borderWidth: 1,
    borderColor: "#CBD5E1",
    borderRadius: 8,
    paddingHorizontal: 12,
    flexDirection: "row",
    alignItems: "center",
  },
  textInput: {
    color: "#0F172A",
    paddingVertical: 10,
    fontSize: 14,
    fontFamily: "monospace",
    flex: 1,
  },
  clearBtn: {
    padding: 6,
  },
  clearBtnText: {
    color: "#94A3B8",
    fontSize: 13,
  },
  quickChipsRow: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 6,
  },
  quickChip: {
    backgroundColor: "#F1F5F9",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    borderRadius: 6,
    paddingHorizontal: 8,
    paddingVertical: 4,
  },
  quickChipText: {
    color: "#475569",
    fontSize: 11,
    fontFamily: "monospace",
  },
  primaryBtn: {
    backgroundColor: "#2563EB",
    borderRadius: 8,
    paddingVertical: 11,
    alignItems: "center",
    justifyContent: "center",
    marginTop: 2,
  },
  btnDisabled: {
    opacity: 0.5,
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
    backgroundColor: "#FEE2E2",
    borderWidth: 1,
    borderColor: "#FECACA",
    borderRadius: 8,
    paddingVertical: 9,
    paddingHorizontal: 12,
    alignItems: "center",
    justifyContent: "center",
  },
  dangerBtnText: {
    color: "#DC2626",
    fontSize: 12,
    fontWeight: "600",
  },
  secondaryBtn: {
    backgroundColor: "#F1F5F9",
    borderWidth: 1,
    borderColor: "#CBD5E1",
    borderRadius: 8,
    paddingVertical: 9,
    paddingHorizontal: 12,
    alignItems: "center",
    justifyContent: "center",
  },
  secondaryBtnText: {
    color: "#334155",
    fontSize: 12,
    fontWeight: "600",
  },
});
