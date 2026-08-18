import { useEffect, useState } from "react";
import {
  ActivityIndicator,
  NativeEventEmitter,
  NativeModules,
  StyleSheet,
  Text,
  TextInput,
  TouchableOpacity,
  View,
} from "react-native";
import { router } from "expo-router";
import { connectHost } from "../src/session";

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
    const sub2 = emitter.addListener("leftcar:host-lost", () => {});
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

  const doConnect = async (target: string, port = 7777) => {
    setBusy(true);
    setError(null);
    try {
      await connectHost(target, port);
      router.push("/catalog");
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  const hosts = Object.values(found);

  return (
    <View style={s.container}>
      <Text style={s.title}>호스트 연결</Text>
      <Text style={s.hint}>Mac에서 Leftcar Host 앱을 실행하세요</Text>

      {hosts.length > 0 && (
        <>
          <Text style={s.subtitle}>발견된 호스트 (mDNS)</Text>
          {hosts.map((h) => (
            <TouchableOpacity
              key={h.host}
              style={s.foundRow}
              onPress={() => doConnect(h.host, h.port)}
              disabled={busy}
            >
              <Text style={s.foundName}>{h.name}</Text>
              <Text style={s.foundAddr}>
                {h.host}:{h.port}
              </Text>
            </TouchableOpacity>
          ))}
        </>
      )}
      {nsd && hosts.length === 0 && <Text style={s.scanning}>mDNS 검색 중…</Text>}
      {!nsd && <Text style={s.hint}>NSD 모듈 없음 — 수동 IP 입력 사용</Text>}

      <Text style={s.subtitle}>수동 입력</Text>
      <TextInput
        style={s.input}
        placeholder="192.168.0.x"
        keyboardType="numeric"
        autoCapitalize="none"
        value={ip}
        onChangeText={setIp}
      />
      <TouchableOpacity
        style={s.button}
        onPress={() => doConnect(ip.trim())}
        disabled={busy || !ip.trim()}
      >
        {busy ? <ActivityIndicator color="#fff" /> : <Text style={s.buttonText}>연결</Text>}
      </TouchableOpacity>
      {error && <Text style={s.error}>{error}</Text>}
    </View>
  );
}

const s = StyleSheet.create({
  container: { flex: 1, padding: 24, gap: 10, backgroundColor: "#111" },
  title: { color: "#fff", fontSize: 22, fontWeight: "700" },
  subtitle: { color: "#aaa", fontSize: 13, fontWeight: "600", marginTop: 10 },
  hint: { color: "#777", fontSize: 12 },
  scanning: { color: "#66bb6a", fontSize: 12 },
  foundRow: {
    backgroundColor: "#1d2b1d",
    borderRadius: 8,
    padding: 12,
    gap: 2,
  },
  foundName: { color: "#fff", fontSize: 15 },
  foundAddr: { color: "#81c784", fontSize: 12, fontFamily: "monospace" },
  input: {
    color: "#fff",
    backgroundColor: "#222",
    borderRadius: 8,
    padding: 12,
    fontSize: 16,
  },
  button: {
    backgroundColor: "#2e7d32",
    borderRadius: 8,
    padding: 14,
    alignItems: "center",
  },
  buttonText: { color: "#fff", fontSize: 16, fontWeight: "600" },
  error: { color: "#ef5350", fontSize: 13 },
});
