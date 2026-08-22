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
import { SafeAreaView } from "react-native-safe-area-context";
import { router } from "expo-router";
import { connectHost, controlClient, controlHost } from "../src/session";
import { isUnauthorizedError, type CatalogView } from "../src/control";

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

function parseManualEndpoint(value: string): { host: string; port: number } | null {
  const normalized = value.trim();
  if (!normalized) return null;
  const separator = normalized.lastIndexOf(":");
  if (separator < 0) return { host: normalized, port: 7777 };
  const host = normalized.slice(0, separator).trim();
  const rawPort = normalized.slice(separator + 1).trim();
  const port = Number(rawPort);
  if (!host || !/^\d+$/.test(rawPort) || !Number.isInteger(port) || port < 1 || port > 65535) {
    return null;
  }
  return { host, port };
}

export default function Host() {
  const [ip, setIp] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [found, setFound] = useState<Record<string, FoundHost>>({});

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
      } catch (e) {
        if (isUnauthorizedError(e)) {
          Alert.alert(
            "기기 페어링 필요",
            "호스트 컴퓨터의 QR 코드 또는 인증 코드를 입력하여 페어링하세요.",
          );
          router.push("/pairing");
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
    const endpoint = parseManualEndpoint(ip);
    if (!endpoint) {
      setError("IP 주소 형식을 확인해 주세요. (예: 192.168.0.10:7777)");
      return;
    }
    void doConnect(endpoint.host, endpoint.port);
  }, [doConnect, ip]);

  return (
    <SafeAreaView style={styles.safeArea} edges={["left", "right", "bottom"]}>
      <ScrollView style={styles.root} contentContainerStyle={styles.content}>
        {/* Header Description */}
        <View style={styles.header}>
          <Text style={styles.title}>호스트 컴퓨터 연결</Text>
          <Text style={styles.sub}>
            동일한 Wi-Fi 네트워크에 있는 Mac 또는 PC의 화면을 찾습니다.
          </Text>
          {controlHost() ? (
            <View style={styles.currentHostBadge}>
              <View style={styles.currentHostDot} />
              <Text style={styles.currentHostText} numberOfLines={1}>
                현재 연결됨: {controlHost()}
              </Text>
            </View>
          ) : null}
        </View>

        {/* Error Alert */}
        {error && (
          <View style={styles.errorCard}>
            <Text style={styles.errorText}>⚠️ {error}</Text>
          </View>
        )}

        {/* Auto Discovered Hosts Section */}
        <View style={styles.card}>
          <View style={styles.cardHeaderRow}>
            <Text style={styles.cardTitle}>주변 기기 자동 검색</Text>
            {nsd && (
              <View style={styles.scanningBadge}>
                <ActivityIndicator size="small" color="#2563EB" />
                <Text style={styles.scanningText}>검색 중</Text>
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
                    <Text style={styles.hostIcon}>💻</Text>
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
            <View style={styles.emptyDiscoverBox}>
              <Text style={styles.emptyDiscoverIcon}>🔍</Text>
              <Text style={styles.emptyDiscoverText}>
                {nsd
                  ? "주변의 Leftcar Host를 찾는 중입니다…\n컴퓨터에서 Host 앱이 실행 중인지 확인하세요."
                  : "자동 검색을 지원하지 않습니다. 아래에서 IP를 직접 입력하세요."}
              </Text>
            </View>
          )}
        </View>

        {/* Manual IP Entry Card */}
        <View style={styles.card}>
          <Text style={styles.cardTitle}>IP 직접 입력</Text>
          <Text style={styles.fieldHint}>컴퓨터의 로컬 IP 주소를 직접 입력하여 연결합니다.</Text>

          <View style={styles.inputWrapper}>
            <TextInput
              style={styles.input}
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
                <Text style={styles.clearBtnText}>✕</Text>
              </Pressable>
            )}
          </View>

          <View style={styles.quickChipsRow}>
            <Pressable onPress={() => setIp("localhost:7777")} style={styles.quickChip}>
              <Text style={styles.quickChipText}>+ localhost:7777</Text>
            </Pressable>
            <Pressable onPress={() => setIp("10.0.2.2:7777")} style={styles.quickChip}>
              <Text style={styles.quickChipText}>+ 10.0.2.2:7777 (에뮬레이터)</Text>
            </Pressable>
          </View>

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

        {/* Tips */}
        <View style={styles.tipBox}>
          <Text style={styles.tipTitle}>💡 연결 팁</Text>
          <Text style={styles.tipText}>
            • XR 헤드셋과 컴퓨터를 같은 5GHz/6GHz Wi-Fi에 연결하면 최상의 지연시간과 화질을 얻을 수 있습니다.
          </Text>
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
    padding: 16,
    gap: 14,
    paddingBottom: 36,
  },
  header: {
    gap: 4,
    marginTop: 4,
  },
  title: {
    color: "#0F172A",
    fontSize: 18,
    fontWeight: "700",
    letterSpacing: -0.3,
  },
  sub: {
    color: "#64748B",
    fontSize: 13,
    lineHeight: 18,
  },
  currentHostBadge: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    backgroundColor: "#EFF6FF",
    borderWidth: 1,
    borderColor: "#DBEAFE",
    paddingHorizontal: 8,
    paddingVertical: 4,
    borderRadius: 6,
    alignSelf: "flex-start",
    marginTop: 4,
    maxWidth: "100%",
  },
  currentHostDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#2563EB",
    flexShrink: 0,
  },
  currentHostText: {
    color: "#1E40AF",
    fontSize: 11,
    fontWeight: "600",
    flex: 1,
  },
  errorCard: {
    backgroundColor: "#FEF2F2",
    borderWidth: 1,
    borderColor: "#FECACA",
    borderRadius: 8,
    padding: 10,
  },
  errorText: {
    color: "#DC2626",
    fontSize: 12,
    lineHeight: 16,
  },
  card: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    borderRadius: 12,
    padding: 16,
    gap: 12,
    shadowColor: "#000000",
    shadowOffset: { width: 0, height: 1 },
    shadowOpacity: 0.04,
    shadowRadius: 2,
    elevation: 1,
  },
  cardHeaderRow: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "center",
  },
  cardTitle: {
    color: "#0F172A",
    fontSize: 14,
    fontWeight: "600",
  },
  scanningBadge: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  scanningText: {
    color: "#64748B",
    fontSize: 12,
  },
  hostList: {
    gap: 8,
  },
  hostItem: {
    backgroundColor: "#F8FAFC",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    borderRadius: 8,
    padding: 10,
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
  },
  hostIconBox: {
    width: 34,
    height: 34,
    borderRadius: 6,
    backgroundColor: "#EFF6FF",
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
  },
  hostIcon: {
    fontSize: 16,
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
    paddingHorizontal: 10,
    paddingVertical: 5,
    borderRadius: 6,
    flexShrink: 0,
  },
  connectChipText: {
    color: "#FFFFFF",
    fontSize: 11,
    fontWeight: "600",
  },
  emptyDiscoverBox: {
    padding: 20,
    alignItems: "center",
    justifyContent: "center",
    gap: 6,
  },
  emptyDiscoverIcon: {
    fontSize: 22,
  },
  emptyDiscoverText: {
    color: "#64748B",
    fontSize: 12,
    textAlign: "center",
    lineHeight: 17,
  },
  fieldHint: {
    color: "#64748B",
    fontSize: 12,
  },
  inputWrapper: {
    backgroundColor: "#F8FAFC",
    borderWidth: 1,
    borderColor: "#CBD5E1",
    borderRadius: 8,
    paddingHorizontal: 10,
    flexDirection: "row",
    alignItems: "center",
  },
  input: {
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
  },
  btnDisabled: {
    opacity: 0.5,
  },
  primaryBtnText: {
    color: "#FFFFFF",
    fontSize: 13,
    fontWeight: "600",
  },
  tipBox: {
    backgroundColor: "#F1F5F9",
    borderRadius: 8,
    padding: 12,
    gap: 4,
  },
  tipTitle: {
    color: "#334155",
    fontSize: 12,
    fontWeight: "600",
  },
  tipText: {
    color: "#64748B",
    fontSize: 11,
    lineHeight: 16,
  },
});
