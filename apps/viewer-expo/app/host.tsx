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
import { router } from "expo-router";
import { connectHost, controlClient, controlHost } from "../src/session";
import { isUnauthorizedError, type CatalogView } from "../src/control";

/**
 * Host connection screen: NSD auto-discovery list + manual IP entry.
 */

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
      // Pairing gate: the host closes the connection on tokenless requests,
      // so probe a real command before declaring the connection usable.
      try {
        await controlClient()?.request<CatalogView>("getCatalog");
      } catch (e) {
        if (isUnauthorizedError(e)) {
          Alert.alert(
            "페어링이 필요합니다",
            "호스트에서 QR 코드와 인증 코드를 확인한 뒤 페어링을 진행하세요.",
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
      setError("IP 주소 또는 포트를 확인해 주세요. 예: 192.168.0.10:7777");
      return;
    }
    void doConnect(endpoint.host, endpoint.port);
  }, [doConnect, ip]);

  return (
    <ScrollView style={styles.root} contentContainerStyle={styles.content}>
      {/* Header Description */}
      <View style={styles.header}>
        <Text style={styles.title}>호스트 탐색 및 연결</Text>
        <Text style={styles.sub}>
          Mac 또는 PC에서 Leftcar Host Studio를 실행하면 로컬 네트워크를 통해 자동으로 탐색됩니다.
        </Text>
        {controlHost() ? (
          <Text style={styles.currentHost}>현재 연결됨: {controlHost()} · 다른 컴퓨터를 선택하면 전환됩니다.</Text>
        ) : null}
      </View>

      {/* Error Alert */}
      {error && (
        <View style={styles.errorCard}>
          <Text style={styles.errorIcon}>⚠️</Text>
          <Text style={styles.errorText}>{error}</Text>
        </View>
      )}

      {/* Auto Discovered Hosts Section */}
      <View style={styles.card}>
        <View style={styles.cardHeaderRow}>
          <View style={styles.cardHeaderLeft}>
            <Text style={styles.cardTitle}>발견된 호스트 (mDNS)</Text>
            {hosts.length > 0 && (
              <View style={styles.countBadge}>
                <Text style={styles.countBadgeText}>{hosts.length}</Text>
              </View>
            )}
          </View>
          {nsd && (
            <View style={styles.scanningBadge}>
              <View style={styles.scanningDot} />
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
                  <Text style={styles.hostName}>{h.name || "Leftcar Host"}</Text>
                  <Text style={styles.hostAddr}>
                    {h.host}:{h.port}
                  </Text>
                </View>
                <View style={styles.connectChip}>
                  <Text style={styles.connectChipText}>연결 →</Text>
                </View>
              </Pressable>
            ))}
          </View>
        ) : (
          <View style={styles.emptyDiscoverBox}>
            <Text style={styles.emptyDiscoverIcon}>📡</Text>
            <Text style={styles.emptyDiscoverText}>
              {nsd
                ? "로컬 네트워크에서 Leftcar 호스트를 찾는 중입니다…"
                : "mDNS 모듈 비활성화 — 아래 수동 입력을 이용하세요"}
            </Text>
          </View>
        )}
      </View>

      {/* Manual IP Entry Card */}
      <View style={styles.card}>
        <Text style={styles.cardTitle}>수동 IP 직접 연결</Text>
        <Text style={styles.fieldHint}>
          호스트 Mac/PC의 로컬 IP 주소와 선택적 포트 (기본: 7777)
        </Text>

        <View style={styles.inputWrapper}>
          <Text style={styles.inputPrefix}>IP</Text>
          <TextInput
            style={styles.input}
            placeholder="192.168.0.x:7777"
            placeholderTextColor="#64748b"
            keyboardType="url"
            autoCapitalize="none"
            autoCorrect={false}
            value={ip}
            onChangeText={setIp}
          />
        </View>

        <Pressable
          style={[styles.primaryBtn, (!ip.trim() || busy) && styles.btnDisabled]}
          onPress={connectManual}
          disabled={busy || !ip.trim()}
        >
          {busy ? (
            <ActivityIndicator color="#ffffff" size="small" />
          ) : (
            <Text style={styles.primaryBtnText}>호스트 연결하기</Text>
          )}
        </Pressable>
      </View>

      {/* Quick LAN Tips */}
      <View style={styles.tipBox}>
        <Text style={styles.tipTitle}>💡 연결 팁</Text>
        <Text style={styles.tipText}>
          • XR 헤드셋과 Mac이 동일한 5GHz / 6GHz Wi-Fi에 연결되어 있어야 최저 지연 시간을 보장합니다.
        </Text>
        <Text style={styles.tipText}>
          • Mac 터미널에서 `ipconfig getifaddr en0` 명령어로 로컬 IP를 확인할 수 있습니다.
        </Text>
      </View>
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
    backgroundColor: "#080b11",
  },
  content: {
    padding: 20,
    gap: 16,
    paddingBottom: 40,
  },
  header: {
    gap: 6,
    marginTop: 8,
  },
  title: {
    color: "#f8fafc",
    fontSize: 22,
    fontWeight: "800",
    letterSpacing: -0.4,
  },
  sub: {
    color: "#94a3b8",
    fontSize: 13,
    lineHeight: 18,
  },
  currentHost: {
    color: "#a5b4fc",
    fontSize: 12,
    lineHeight: 18,
    marginTop: 4,
  },
  errorCard: {
    backgroundColor: "rgba(239, 68, 68, 0.15)",
    borderWidth: 1,
    borderColor: "rgba(239, 68, 68, 0.35)",
    borderRadius: 12,
    padding: 14,
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
  },
  errorIcon: {
    fontSize: 16,
  },
  errorText: {
    color: "#fca5a5",
    fontSize: 13,
    flex: 1,
  },
  card: {
    backgroundColor: "#0f172a",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.08)",
    borderRadius: 14,
    padding: 16,
    gap: 12,
  },
  cardHeaderRow: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "center",
  },
  cardHeaderLeft: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  cardTitle: {
    color: "#f8fafc",
    fontSize: 15,
    fontWeight: "700",
  },
  countBadge: {
    backgroundColor: "#10b981",
    paddingHorizontal: 6,
    paddingVertical: 1,
    borderRadius: 10,
  },
  countBadgeText: {
    color: "#ffffff",
    fontSize: 11,
    fontWeight: "700",
    fontFamily: "monospace",
  },
  scanningBadge: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    backgroundColor: "rgba(16, 185, 129, 0.12)",
    paddingHorizontal: 8,
    paddingVertical: 4,
    borderRadius: 12,
  },
  scanningDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#10b981",
  },
  scanningText: {
    color: "#34d399",
    fontSize: 11,
    fontWeight: "600",
  },
  hostList: {
    gap: 8,
  },
  hostItem: {
    backgroundColor: "#161f33",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.06)",
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
    backgroundColor: "#0f172a",
    alignItems: "center",
    justifyContent: "center",
  },
  hostIcon: {
    fontSize: 18,
  },
  hostInfo: {
    flex: 1,
    gap: 2,
  },
  hostName: {
    color: "#f8fafc",
    fontSize: 14,
    fontWeight: "600",
  },
  hostAddr: {
    color: "#38bdf8",
    fontSize: 12,
    fontFamily: "monospace",
  },
  connectChip: {
    backgroundColor: "#6366f1",
    paddingHorizontal: 10,
    paddingVertical: 6,
    borderRadius: 6,
  },
  connectChipText: {
    color: "#ffffff",
    fontSize: 11,
    fontWeight: "700",
  },
  emptyDiscoverBox: {
    padding: 20,
    alignItems: "center",
    justifyContent: "center",
    gap: 8,
  },
  emptyDiscoverIcon: {
    fontSize: 24,
  },
  emptyDiscoverText: {
    color: "#64748b",
    fontSize: 12,
    textAlign: "center",
    lineHeight: 18,
  },
  fieldHint: {
    color: "#94a3b8",
    fontSize: 12,
  },
  inputWrapper: {
    flexDirection: "row",
    alignItems: "center",
    backgroundColor: "#161f33",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.1)",
    borderRadius: 10,
    paddingHorizontal: 12,
  },
  inputPrefix: {
    color: "#64748b",
    fontSize: 12,
    fontWeight: "700",
    fontFamily: "monospace",
    marginRight: 8,
  },
  input: {
    flex: 1,
    color: "#f8fafc",
    paddingVertical: 12,
    fontSize: 15,
    fontFamily: "monospace",
  },
  primaryBtn: {
    backgroundColor: "#6366f1",
    borderRadius: 10,
    paddingVertical: 14,
    alignItems: "center",
    justifyContent: "center",
  },
  btnDisabled: {
    opacity: 0.5,
  },
  primaryBtnText: {
    color: "#ffffff",
    fontSize: 14,
    fontWeight: "700",
  },
  tipBox: {
    backgroundColor: "rgba(15, 23, 42, 0.5)",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.06)",
    borderRadius: 12,
    padding: 14,
    gap: 6,
  },
  tipTitle: {
    color: "#f8fafc",
    fontSize: 12,
    fontWeight: "700",
  },
  tipText: {
    color: "#94a3b8",
    fontSize: 11,
    lineHeight: 16,
  },
});
