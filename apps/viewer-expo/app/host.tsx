import { useEffect, useState } from "react";
import {
  ActivityIndicator,
  StyleSheet,
  Text,
  TextInput,
  TouchableOpacity,
  View,
} from "react-native";
import { router } from "expo-router";
import { connectHost } from "../src/session";

/**
 * Host connection screen: manual IP entry (NSD discovery arrives in Task 10
 * on top of this same flow).
 */
export default function Host() {
  const [ip, setIp] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
  }, [ip]);

  const doConnect = async () => {
    if (!ip.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await connectHost(ip.trim());
      router.push("/catalog");
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <View style={s.container}>
      <Text style={s.title}>호스트 연결</Text>
      <Text style={s.hint}>Mac에서 Leftcar Host 앱을 실행한 뒤 IP를 입력하세요</Text>
      <TextInput
        style={s.input}
        placeholder="192.168.0.x"
        keyboardType="numeric"
        autoCapitalize="none"
        value={ip}
        onChangeText={setIp}
      />
      <TouchableOpacity style={s.button} onPress={doConnect} disabled={busy || !ip.trim()}>
        {busy ? <ActivityIndicator color="#fff" /> : <Text style={s.buttonText}>연결</Text>}
      </TouchableOpacity>
      {error && <Text style={s.error}>{error}</Text>}
    </View>
  );
}

const s = StyleSheet.create({
  container: { flex: 1, padding: 24, gap: 12, justifyContent: "center", backgroundColor: "#111" },
  title: { color: "#fff", fontSize: 22, fontWeight: "700" },
  hint: { color: "#999", fontSize: 13 },
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
