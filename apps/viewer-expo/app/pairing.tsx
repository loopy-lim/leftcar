import { useCallback, useEffect, useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { CameraView, useCameraPermissions } from "expo-camera";
import { SafeAreaView } from "react-native-safe-area-context";
import { router } from "expo-router";
import { connectHost, controlHost } from "../src/session";
import { pairWithHost } from "../src/pairing";

type PairingMode = "qr" | "code";

export default function Pairing() {
  const [mode, setMode] = useState<PairingMode>("qr");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [permission, requestPermission] = useCameraPermissions();
  const host = controlHost();

  useEffect(() => {
    setError(null);
  }, [code, mode]);

  const handlePair = useCallback(
    async (codeToPair: string) => {
      const trimmed = codeToPair.trim().replace(/\s+/g, "");
      if (trimmed.length !== 6) {
        setError("6자리 인증 코드를 정확히 입력하세요.");
        return;
      }
      setBusy(true);
      setError(null);
      try {
        const client = await connectHost(host || "localhost:7777");
        await pairWithHost(client, trimmed);
        router.replace("/catalog");
      } catch (e) {
        setError(String(e instanceof Error ? e.message : e));
      } finally {
        setBusy(false);
      }
    },
    [host],
  );

  const handleQrScanned = useCallback(
    (scannedCode: string) => {
      if (busy) return;
      void handlePair(scannedCode);
    },
    [busy, handlePair],
  );

  return (
    <SafeAreaView style={styles.safeArea} edges={["left", "right", "bottom"]}>
      <ScrollView style={styles.root} contentContainerStyle={styles.content}>
        {/* Header Title */}
        <View style={styles.header}>
          <Text style={styles.title}>호스트 기기 페어링</Text>
          <Text style={styles.sub}>
            컴퓨터의 Leftcar Host 화면에 표시된 QR 코드나 6자리 코드로 안전하게 연결합니다.
          </Text>
          {host ? (
            <View style={styles.hostChip}>
              <View style={styles.hostDot} />
              <Text style={styles.hostAddr} numberOfLines={1}>대상: {host}</Text>
            </View>
          ) : null}
        </View>

        {/* Mode Selector */}
        <View style={styles.modeTabs}>
          <Pressable
            style={[styles.modeTab, mode === "qr" && styles.modeTabActive]}
            onPress={() => setMode("qr")}
          >
            <Text style={[styles.modeTabText, mode === "qr" && styles.modeTabTextActive]}>
              📷 QR 코드 스캔
            </Text>
          </Pressable>
          <Pressable
            style={[styles.modeTab, mode === "code" && styles.modeTabActive]}
            onPress={() => setMode("code")}
          >
            <Text style={[styles.modeTabText, mode === "code" && styles.modeTabTextActive]}>
              🔢 6자리 코드 입력
            </Text>
          </Pressable>
        </View>

        {/* Error Card */}
        {error && (
          <View style={styles.errorCard}>
            <Text style={styles.errorText}>⚠️ {error}</Text>
          </View>
        )}

        {/* QR Scan View */}
        {mode === "qr" ? (
          <View style={styles.card}>
            {!permission ? (
              <View style={styles.cameraBox}>
                <ActivityIndicator color="#2563EB" />
              </View>
            ) : !permission.granted ? (
              <View style={styles.cameraNotice}>
                <Text style={styles.cameraNoticeIcon}>📷</Text>
                <Text style={styles.cameraNoticeTitle}>카메라 권한 필요</Text>
                <Text style={styles.cameraNoticeText}>
                  컴퓨터 화면의 QR 코드를 스캔하려면 카메라 권한이 필요합니다.
                </Text>
                <Pressable onPress={requestPermission} style={styles.permissionBtn}>
                  <Text style={styles.permissionBtnText}>권한 허용하기</Text>
                </Pressable>
              </View>
            ) : (
              <View style={styles.scannerWrapper}>
                <CameraView
                  style={styles.camera}
                  facing="back"
                  barcodeScannerSettings={{ barcodeTypes: ["qr"] }}
                  onBarcodeScanned={(result) => {
                    const value = result.data?.trim();
                    if (value) handleQrScanned(value);
                  }}
                >
                  <View style={styles.scanOverlay}>
                    <View style={styles.scanFrame} />
                    <View style={styles.scanHintBox}>
                      <Text style={styles.scanHintText}>QR 코드를 사각형 안에 맞춰주세요</Text>
                    </View>
                  </View>
                </CameraView>
              </View>
            )}
          </View>
        ) : (
          /* Code Input View */
          <View style={styles.card}>
            <Text style={styles.cardTitle}>6자리 인증 코드 입력</Text>
            <Text style={styles.fieldHint}>
              컴퓨터 화면의 QR 코드 아래에 표시된 6자리 번호를 입력하세요.
            </Text>

            <TextInput
              style={styles.codeInput}
              placeholder="000000"
              placeholderTextColor="#94A3B8"
              keyboardType="number-pad"
              maxLength={6}
              value={code}
              onChangeText={setCode}
              editable={!busy}
            />

            <Pressable
              style={[styles.primaryBtn, (code.length !== 6 || busy) && styles.btnDisabled]}
              onPress={() => handlePair(code)}
              disabled={code.length !== 6 || busy}
            >
              {busy ? (
                <ActivityIndicator color="#FFFFFF" size="small" />
              ) : (
                <Text style={styles.primaryBtnText}>페어링 완료하기</Text>
              )}
            </Pressable>
          </View>
        )}

        {/* Help Tip */}
        <View style={styles.tipBox}>
          <Text style={styles.tipTitle}>💡 페어링 안내</Text>
          <Text style={styles.tipText}>
            • 한 번 페어링된 기기는 다음 연결 시 자동으로 승인됩니다.
          </Text>
          <Text style={styles.tipText}>
            • 보안을 위해 생성된 QR 코드는 2분 후 만료됩니다.
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
  hostChip: {
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
  hostDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#2563EB",
    flexShrink: 0,
  },
  hostAddr: {
    color: "#1E40AF",
    fontSize: 11,
    fontWeight: "600",
    fontFamily: "monospace",
    flex: 1,
  },
  modeTabs: {
    flexDirection: "row",
    backgroundColor: "#F1F5F9",
    borderRadius: 8,
    padding: 3,
    borderWidth: 1,
    borderColor: "#E2E8F0",
  },
  modeTab: {
    flex: 1,
    paddingVertical: 8,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 6,
  },
  modeTabActive: {
    backgroundColor: "#FFFFFF",
    shadowColor: "#000000",
    shadowOffset: { width: 0, height: 1 },
    shadowOpacity: 0.06,
    shadowRadius: 2,
    elevation: 1,
  },
  modeTabText: {
    color: "#64748B",
    fontSize: 12,
    fontWeight: "600",
  },
  modeTabTextActive: {
    color: "#0F172A",
    fontWeight: "700",
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
  cardTitle: {
    color: "#0F172A",
    fontSize: 14,
    fontWeight: "600",
  },
  fieldHint: {
    color: "#64748B",
    fontSize: 12,
  },
  scannerWrapper: {
    height: 240,
    borderRadius: 10,
    overflow: "hidden",
    backgroundColor: "#000000",
  },
  camera: {
    flex: 1,
  },
  cameraBox: {
    height: 200,
    alignItems: "center",
    justifyContent: "center",
  },
  scanOverlay: {
    flex: 1,
    backgroundColor: "rgba(0,0,0,0.3)",
    alignItems: "center",
    justifyContent: "center",
    gap: 12,
  },
  scanFrame: {
    width: 160,
    height: 160,
    borderWidth: 2,
    borderColor: "#FFFFFF",
    borderRadius: 12,
    backgroundColor: "transparent",
  },
  scanHintBox: {
    backgroundColor: "rgba(0,0,0,0.6)",
    paddingHorizontal: 10,
    paddingVertical: 4,
    borderRadius: 6,
  },
  scanHintText: {
    color: "#FFFFFF",
    fontSize: 11,
    fontWeight: "500",
  },
  cameraNotice: {
    padding: 20,
    alignItems: "center",
    justifyContent: "center",
    gap: 8,
  },
  cameraNoticeIcon: {
    fontSize: 28,
  },
  cameraNoticeTitle: {
    color: "#0F172A",
    fontSize: 14,
    fontWeight: "600",
  },
  cameraNoticeText: {
    color: "#64748B",
    fontSize: 12,
    textAlign: "center",
    lineHeight: 17,
  },
  permissionBtn: {
    backgroundColor: "#2563EB",
    borderRadius: 6,
    paddingHorizontal: 12,
    paddingVertical: 8,
    marginTop: 4,
  },
  permissionBtnText: {
    color: "#FFFFFF",
    fontSize: 12,
    fontWeight: "600",
  },
  codeInput: {
    backgroundColor: "#F8FAFC",
    borderWidth: 1,
    borderColor: "#CBD5E1",
    borderRadius: 8,
    color: "#0F172A",
    fontSize: 24,
    fontWeight: "700",
    fontFamily: "monospace",
    letterSpacing: 6,
    textAlign: "center",
    paddingVertical: 10,
    paddingHorizontal: 8,
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
