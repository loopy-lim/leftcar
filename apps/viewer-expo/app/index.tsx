import { useCallback, useEffect, useState } from "react";
import { NativeModules, ScrollView, StyleSheet, Text, View } from "react-native";

/**
 * Leftcar viewer hub (docs/03 §3.2 HubActivity 역할, Expo/RN 구현).
 *
 * H09 실측 화면: JS -> NativeModules.Rustra -> JNI -> Rust rustra package.
 * addNumbers(20,22)=42가 이 화면에서 실기기로 증명된다.
 * 응답 프로토콜은 @rustra/react-native의 {ok, result|error} 형식을 따른다.
 */

type RustraNativeModule = {
  invoke(command: string, argsJson: string): Promise<string>;
  contractHash(): Promise<string>;
};

const native = NativeModules.Rustra as RustraNativeModule | undefined;

async function rustraInvoke<T>(command: string, args: unknown): Promise<T> {
  if (!native) throw new Error("NativeModules.Rustra 없음 — dev build 확인");
  const out = await native.invoke(command, JSON.stringify(args ?? {}));
  return JSON.parse(out) as T;
}

type CheckState = "pending" | "pass" | "fail";

export default function Hub() {
  const [addResult, setAddResult] = useState("—");
  const [addState, setAddState] = useState<CheckState>("pending");
  const [hash, setHash] = useState("—");
  const [hashState, setHashState] = useState<CheckState>("pending");
  const [error, setError] = useState<string | null>(null);

  const runProof = useCallback(async () => {
    setAddState("pending");
    setHashState("pending");
    setError(null);
    try {
      const out = await rustraInvoke<{ value: number }>("addNumbers", { a: 20, b: 22 });
      setAddResult(String(out.value));
      setAddState(out.value === 42 ? "pass" : "fail");
      const h = await native!.contractHash();
      setHash(h.slice(0, 16));
      setHashState(h.length === 16 ? "pass" : "fail");
    } catch (e) {
      setError(String(e));
      setAddState("fail");
      setHashState("fail");
    }
  }, []);

  useEffect(() => {
    runProof();
  }, [runProof]);

  return (
    <ScrollView style={styles.root} contentContainerStyle={styles.content}>
      <Text style={styles.title}>Leftcar</Text>
      <Text style={styles.sub}>Expo · React Native · Rustra 네이티브 경로</Text>

      <View style={styles.card}>
        <Text style={styles.cardTitle}>H09 계약 증명 (JS → JNI → Rust)</Text>
        <Row label="addNumbers(20, 22)" value={addResult} expect="42" state={addState} />
        <Row label="contract hash" value={hash} expect="16 hex" state={hashState} />
      </View>

      {error ? (
        <View style={styles.errorBox}>
          <Text style={styles.errorText}>{error}</Text>
        </View>
      ) : null}

      <Text style={styles.footnote}>
        video plane은 이 경로를 통과하지 않는다 (docs/04 §1). 이 화면은 제어
        계약만 증명한다.
      </Text>
    </ScrollView>
  );
}

function Row({
  label,
  value,
  expect,
  state,
}: {
  label: string;
  value: string;
  expect: string;
  state: CheckState;
}) {
  const color = state === "pass" ? "#10b981" : state === "fail" ? "#ef4444" : "#94a3b8";
  return (
    <View style={styles.row}>
      <Text style={styles.rowLabel}>{label}</Text>
      <Text style={styles.rowMono}>
        {value} <Text style={{ color }}>{"(기대 " + expect + ")"}</Text>
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1, backgroundColor: "#090d16" },
  content: { padding: 24, gap: 16 },
  title: { color: "#f8fafc", fontSize: 32, fontWeight: "700" },
  sub: { color: "#94a3b8", fontSize: 14, marginBottom: 8 },
  card: { backgroundColor: "#0f172a", borderRadius: 12, padding: 16, gap: 10 },
  cardTitle: { color: "#f8fafc", fontSize: 16, fontWeight: "600" },
  row: { flexDirection: "row", justifyContent: "space-between", alignItems: "center" },
  rowLabel: { color: "#94a3b8", fontSize: 13 },
  rowMono: { color: "#f8fafc", fontFamily: "monospace", fontSize: 13 },
  errorBox: { backgroundColor: "#3f1d1d", borderRadius: 8, padding: 12 },
  errorText: { color: "#fca5a5", fontSize: 12 },
  footnote: { color: "#64748b", fontSize: 11, marginTop: 8 },
});
