import { useCallback, useEffect, useRef, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  KeyboardAvoidingView,
  Linking,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { CameraView, useCameraPermissions } from "expo-camera";
import { router } from "expo-router";
import { pairWithHost, parseQrPayload, type QrPayload } from "../src/pairing";

/**
 * Host pairing flow (design §페어링): scan the host's QR offer, then enter
 * the 6-digit verification code shown on the host screen. The issued token
 * lands in secure storage; the next control request picks it up.
 */

const CODE_LENGTH = 6;
const INVALID_QR_HINT_MS = 1_800;

export default function Pairing() {
  const [permission, requestPermission] = useCameraPermissions();
  const [payload, setPayload] = useState<QrPayload | null>(null);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [invalidQrHint, setInvalidQrHint] = useState(false);
  const [failCount, setFailCount] = useState(0);
  // Once a valid offer is captured, stop reacting to further camera frames.
  const scannedRef = useRef(false);
  const invalidHintTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Guards post-pair navigation: the async pair call can settle after the
  // screen was unmounted (the token is already stored either way).
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (invalidHintTimer.current) {
        clearTimeout(invalidHintTimer.current);
        invalidHintTimer.current = null;
      }
    };
  }, []);

  const handleBarcode = useCallback(({ data }: { data: string }) => {
    if (scannedRef.current) return;
    const parsed = parseQrPayload(data);
    if (parsed) {
      scannedRef.current = true;
      setPayload(parsed);
      setError(null);
      setCode("");
      setFailCount(0);
      return;
    }
    // Wrong QR: brief hint, keep scanning (debounced).
    if (invalidHintTimer.current) return;
    setInvalidQrHint(true);
    invalidHintTimer.current = setTimeout(() => {
      invalidHintTimer.current = null;
      setInvalidQrHint(false);
    }, INVALID_QR_HINT_MS);
  }, []);

  const submitCode = useCallback(async () => {
    if (!payload || code.length !== CODE_LENGTH || busy) return;
    setBusy(true);
    setError(null);
    try {
      await pairWithHost(payload, code);
      if (mountedRef.current) {
        Alert.alert("페어링 완료", "호스트와 연결되었습니다.", [
          { text: "확인", onPress: () => router.back() },
        ]);
      }
    } catch {
      const next = failCount + 1;
      setFailCount(next);
      setError(
        next >= 2
          ? "페어링 실패. 코드를 확인하세요. 오퍼가 만료되었을 수 있습니다. 호스트에서 QR을 다시 생성하세요."
          : "페어링 실패. 코드를 확인하세요",
      );
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }, [busy, code, failCount, payload]);

  const restartScan = useCallback(() => {
    scannedRef.current = false;
    if (invalidHintTimer.current) {
      clearTimeout(invalidHintTimer.current);
      invalidHintTimer.current = null;
    }
    setPayload(null);
    setCode("");
    setError(null);
    setInvalidQrHint(false);
    setFailCount(0);
  }, []);

  // ---- Stage 2: code entry ------------------------------------------------
  if (payload) {
    return (
      <KeyboardAvoidingView
        style={styles.root}
        behavior={Platform.OS === "ios" ? "padding" : undefined}
      >
        <ScrollView contentContainerStyle={styles.content}>
          <View style={styles.card}>
            <Text style={styles.title}>인증 코드 입력</Text>
            <Text style={styles.sub}>
              호스트 화면에 표시된 6자리 코드를 입력하세요. (QR에는 포함되지 않은 두 번째 인증
              수단입니다)
            </Text>

            <View style={styles.hostRow}>
              <View style={styles.hostIconBox}>
                <Text style={styles.hostIcon}>💻</Text>
              </View>
              <View>
                <Text style={styles.hostLabel}>연결 대상</Text>
                <Text style={styles.hostAddr}>
                  {payload.host}:{payload.port}
                </Text>
              </View>
            </View>

            <TextInput
              style={styles.codeInput}
              value={code}
              onChangeText={(text) => setCode(text.replace(/\D/g, "").slice(0, CODE_LENGTH))}
              keyboardType="number-pad"
              maxLength={CODE_LENGTH}
              placeholder="000000"
              placeholderTextColor="#334155"
              editable={!busy}
              autoFocus
            />

            {error ? (
              <View style={styles.errorCard}>
                <Text style={styles.errorIcon}>⚠️</Text>
                <Text style={styles.errorText}>{error}</Text>
              </View>
            ) : null}

            <Pressable
              style={[styles.primaryBtn, (code.length < CODE_LENGTH || busy) && styles.btnDisabled]}
              onPress={submitCode}
              disabled={busy || code.length < CODE_LENGTH}
            >
              {busy ? (
                <ActivityIndicator color="#ffffff" size="small" />
              ) : (
                <Text style={styles.primaryBtnText}>페어링</Text>
              )}
            </Pressable>

            <Pressable style={styles.rescanBtn} onPress={restartScan} disabled={busy}>
              <Text style={styles.rescanBtnText}>QR 다시 스캔하기</Text>
            </Pressable>
          </View>
        </ScrollView>
      </KeyboardAvoidingView>
    );
  }

  // ---- Stage 1: QR scan ----------------------------------------------------
  if (!permission) {
    return (
      <View style={styles.center}>
        <ActivityIndicator color="#6366f1" size="large" />
      </View>
    );
  }

  if (!permission.granted) {
    return (
      <View style={styles.center}>
        <Text style={styles.permissionIcon}>📷</Text>
        <Text style={styles.permissionTitle}>카메라 권한 필요</Text>
        <Text style={styles.permissionText}>
          QR 코드를 스캔하려면 카메라 권한이 필요합니다. 설정에서 허용해 주세요.
        </Text>
        <Pressable style={styles.primaryBtn} onPress={requestPermission}>
          <Text style={styles.primaryBtnText}>권한 요청하기</Text>
        </Pressable>
        <Pressable
          style={styles.rescanBtn}
          onPress={() => {
            void Linking.openSettings();
          }}
        >
          <Text style={styles.rescanBtnText}>설정 열기</Text>
        </Pressable>
      </View>
    );
  }

  return (
    <View style={styles.scannerRoot}>
      <CameraView
        style={StyleSheet.absoluteFill}
        barcodeScannerSettings={{ barcodeTypes: ["qr"] }}
        onBarcodeScanned={handleBarcode}
      />
      <View style={styles.scanOverlay} pointerEvents="box-none">
        <Text style={styles.scanHint}>호스트 화면의 QR 코드를 스캔하세요</Text>
        <View style={styles.scanFrame}>
          <View style={[styles.corner, styles.cornerTL]} />
          <View style={[styles.corner, styles.cornerTR]} />
          <View style={[styles.corner, styles.cornerBL]} />
          <View style={[styles.corner, styles.cornerBR]} />
        </View>
        {invalidQrHint ? (
          <View style={styles.invalidCard}>
            <Text style={styles.invalidText}>올바르지 않은 QR입니다</Text>
          </View>
        ) : null}
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
    backgroundColor: "#080b11",
  },
  content: {
    padding: 20,
    paddingBottom: 40,
  },
  card: {
    backgroundColor: "#0f172a",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.08)",
    borderRadius: 14,
    padding: 16,
    gap: 14,
  },
  title: {
    color: "#f8fafc",
    fontSize: 20,
    fontWeight: "800",
    letterSpacing: -0.4,
  },
  sub: {
    color: "#94a3b8",
    fontSize: 13,
    lineHeight: 18,
  },
  hostRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    backgroundColor: "#161f33",
    borderRadius: 10,
    padding: 12,
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
  hostLabel: {
    color: "#94a3b8",
    fontSize: 11,
  },
  hostAddr: {
    color: "#38bdf8",
    fontSize: 13,
    fontFamily: "monospace",
  },
  codeInput: {
    backgroundColor: "#161f33",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.1)",
    borderRadius: 10,
    color: "#f8fafc",
    fontSize: 32,
    fontFamily: "monospace",
    letterSpacing: 12,
    textAlign: "center",
    paddingVertical: 14,
  },
  errorCard: {
    backgroundColor: "rgba(239, 68, 68, 0.15)",
    borderWidth: 1,
    borderColor: "rgba(239, 68, 68, 0.35)",
    borderRadius: 12,
    padding: 12,
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
    lineHeight: 18,
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
  rescanBtn: {
    alignItems: "center",
    paddingVertical: 10,
  },
  rescanBtnText: {
    color: "#94a3b8",
    fontSize: 13,
    fontWeight: "600",
  },
  center: {
    flex: 1,
    backgroundColor: "#080b11",
    alignItems: "center",
    justifyContent: "center",
    padding: 32,
    gap: 12,
  },
  permissionIcon: {
    fontSize: 32,
  },
  permissionTitle: {
    color: "#f8fafc",
    fontSize: 17,
    fontWeight: "700",
  },
  permissionText: {
    color: "#94a3b8",
    fontSize: 13,
    textAlign: "center",
    lineHeight: 19,
  },
  scannerRoot: {
    flex: 1,
    backgroundColor: "#000000",
  },
  scanOverlay: {
    position: "absolute",
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    alignItems: "center",
    justifyContent: "center",
    gap: 24,
  },
  scanHint: {
    color: "#f8fafc",
    fontSize: 15,
    fontWeight: "700",
    textAlign: "center",
    paddingHorizontal: 24,
  },
  scanFrame: {
    width: 240,
    height: 240,
  },
  corner: {
    position: "absolute",
    width: 36,
    height: 36,
    borderColor: "#818cf8",
  },
  cornerTL: {
    top: 0,
    left: 0,
    borderTopWidth: 3,
    borderLeftWidth: 3,
    borderTopLeftRadius: 12,
  },
  cornerTR: {
    top: 0,
    right: 0,
    borderTopWidth: 3,
    borderRightWidth: 3,
    borderTopRightRadius: 12,
  },
  cornerBL: {
    bottom: 0,
    left: 0,
    borderBottomWidth: 3,
    borderLeftWidth: 3,
    borderBottomLeftRadius: 12,
  },
  cornerBR: {
    bottom: 0,
    right: 0,
    borderBottomWidth: 3,
    borderRightWidth: 3,
    borderBottomRightRadius: 12,
  },
  invalidCard: {
    backgroundColor: "rgba(239, 68, 68, 0.9)",
    paddingHorizontal: 16,
    paddingVertical: 8,
    borderRadius: 10,
  },
  invalidText: {
    color: "#ffffff",
    fontSize: 13,
    fontWeight: "700",
  },
});
