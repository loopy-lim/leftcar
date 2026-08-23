import { useCallback, useEffect, useRef, useState } from "react";
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
import {
  pairWithHost,
  pairWithHostByCode,
  parseQrPayload,
  type QrPayload,
} from "../src/pairing";
import { formatErrorMessage } from "../src/control";

type PairingMode = "qr" | "code";

function parseHostEndpoint(endpoint: string): { host: string; port: number } {
  const trimmed = endpoint.trim();
  if (!trimmed) return { host: "localhost", port: 7777 };
  const idx = trimmed.lastIndexOf(":");
  if (idx < 0) return { host: trimmed, port: 7777 };
  const host = trimmed.slice(0, idx).trim() || "localhost";
  const rawPort = Number(trimmed.slice(idx + 1).trim());
  const port = Number.isInteger(rawPort) && rawPort > 0 && rawPort < 65536 ? rawPort : 7777;
  return { host, port };
}

export default function Pairing() {
  const [mode, setMode] = useState<PairingMode>("qr");
  const [code, setCode] = useState("");
  const scannedPayloadRef = useRef<QrPayload | null>(null);
  const [busy, setBusy] = useState(false);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [permission, requestPermission] = useCameraPermissions();
  const host = controlHost();
  const scanningLockRef = useRef(false);

  useEffect(() => {
    setError(null);
  }, [code, mode]);

  const handlePairWithCode = useCallback(
    async (codeToPair: string) => {
      const trimmed = codeToPair.trim().replace(/\s+/g, "");
      if (trimmed.length !== 6) {
        setError("6자리 인증 코드를 정확히 입력해 주세요.");
        return;
      }
      setBusy(true);
      setError(null);
      setStatusMessage("호스트 인증 중…");
      try {
        const scannedPayload = scannedPayloadRef.current;
        if (scannedPayload) {
          await pairWithHost(scannedPayload, trimmed);
          await connectHost(scannedPayload.host, scannedPayload.port);
        } else {
          const endpoint = parseHostEndpoint(host || "localhost:7777");
          await pairWithHostByCode(endpoint.host, endpoint.port, trimmed);
          await connectHost(endpoint.host, endpoint.port);
        }
        router.replace("/catalog");
      } catch (e) {
        setError(formatErrorMessage(e));
      } finally {
        setBusy(false);
        setStatusMessage(null);
      }
    },
    [host],
  );

  const handleQrScanned = useCallback(
    async (scannedData: string) => {
      if (busy || scanningLockRef.current) return;
      scanningLockRef.current = true;
      try {
        const payload = parseQrPayload(scannedData);
        if (!payload) {
          setError("올바른 Leftcar 페어링 QR 코드가 아닙니다.");
          return;
        }
        setBusy(true);
        setError(null);
        if (payload.code) {
          setStatusMessage("QR 코드로 자동 페어링 중…");
          await pairWithHost(payload, payload.code);
          await connectHost(payload.host, payload.port);
          router.replace("/catalog");
        } else {
          scannedPayloadRef.current = payload;
          setMode("code");
          setStatusMessage("QR 코드가 인식되었습니다. 화면의 6자리 인증 번호를 입력하세요.");
        }
      } catch (e) {
        setError(formatErrorMessage(e));
      } finally {
        setBusy(false);
        setStatusMessage(null);
        setTimeout(() => {
          scanningLockRef.current = false;
        }, 1500);
      }
    },
    [busy],
  );

  return (
    <SafeAreaView style={styles.safeArea} edges={["left", "right", "bottom"]}>
      <ScrollView
        style={styles.root}
        contentContainerStyle={styles.content}
        showsVerticalScrollIndicator={false}
      >
        {/* Host Info Strip */}
        {host ? (
          <View style={styles.hostStrip}>
            <View style={styles.hostDot} />
            <Text style={styles.hostText} numberOfLines={1}>
              대상 호스트: <Text style={styles.hostAddr}>{host}</Text>
            </Text>
          </View>
        ) : null}

        {/* Segmented Mode Selector */}
        <View style={styles.modeTabsWrapper}>
          <View style={styles.modeTabs}>
            <Pressable
              style={[styles.modeTab, mode === "qr" && styles.modeTabActive]}
              onPress={() => setMode("qr")}
            >
              <Text style={[styles.modeTabText, mode === "qr" && styles.modeTabTextActive]}>
                📷 QR 스캔
              </Text>
            </Pressable>
            <Pressable
              style={[styles.modeTab, mode === "code" && styles.modeTabActive]}
              onPress={() => setMode("code")}
            >
              <Text style={[styles.modeTabText, mode === "code" && styles.modeTabTextActive]}>
                🔢 6자리 코드
              </Text>
            </Pressable>
          </View>
        </View>

        {/* Status Alert */}
        {statusMessage && (
          <View style={styles.statusCard}>
            <ActivityIndicator size="small" color="#2563EB" />
            <Text style={styles.statusText}>{statusMessage}</Text>
          </View>
        )}

        {/* Error Alert */}
        {error && (
          <View style={styles.errorCard}>
            <Text style={styles.errorText}>⚠️ {error}</Text>
          </View>
        )}

        {/* Mode View */}
        {mode === "qr" ? (
          <View style={styles.card}>
            <Text style={styles.cardTitle}>QR 코드 스캔</Text>
            <Text style={styles.cardDesc}>
              컴퓨터의 Leftcar Host 화면에 띄운 QR 코드를 비춰주세요.
            </Text>

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
                  <Text style={styles.permissionBtnText}>권한 허용</Text>
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
                      <Text style={styles.scanHintText}>QR 코드를 사각형에 맞추세요</Text>
                    </View>
                  </View>
                </CameraView>
              </View>
            )}
          </View>
        ) : (
          <View style={styles.card}>
            <Text style={styles.cardTitle}>6자리 인증 코드</Text>
            <Text style={styles.cardDesc}>
              호스트 화면의 QR 코드 아래에 적힌 6자리 번호를 입력하세요.
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
              onPress={() => handlePairWithCode(code)}
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

        {/* Security / Help Card */}
        <View style={styles.tipBox}>
          <Text style={styles.tipTitle}>💡 보안 및 안내</Text>
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
    paddingHorizontal: 16,
    paddingTop: 12,
    paddingBottom: 32,
    gap: 14,
  },
  hostStrip: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    borderRadius: 8,
    paddingHorizontal: 12,
    paddingVertical: 8,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  hostDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#2563EB",
    flexShrink: 0,
  },
  hostText: {
    color: "#64748B",
    fontSize: 12,
    flex: 1,
  },
  hostAddr: {
    color: "#0F172A",
    fontWeight: "600",
    fontFamily: "monospace",
  },
  modeTabsWrapper: {
    backgroundColor: "#FFFFFF",
    borderRadius: 10,
    borderWidth: 1,
    borderColor: "#E2E8F0",
    padding: 3,
  },
  modeTabs: {
    flexDirection: "row",
    gap: 4,
  },
  modeTab: {
    flex: 1,
    paddingVertical: 7,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 7,
  },
  modeTabActive: {
    backgroundColor: "#EFF6FF",
    borderWidth: 1,
    borderColor: "#BFDBFE",
  },
  modeTabText: {
    color: "#64748B",
    fontSize: 12,
    fontWeight: "600",
  },
  modeTabTextActive: {
    color: "#1D4ED8",
    fontWeight: "700",
  },
  statusCard: {
    backgroundColor: "#EFF6FF",
    borderWidth: 1,
    borderColor: "#BFDBFE",
    borderRadius: 8,
    padding: 10,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  statusText: {
    color: "#1D4ED8",
    fontSize: 12,
    fontWeight: "500",
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
    borderRadius: 14,
    padding: 16,
    gap: 12,
    boxShadow: "0 1px 3px rgba(0, 0, 0, 0.03)",
  },
  cardTitle: {
    fontSize: 14,
    fontWeight: "600",
    color: "#0F172A",
  },
  cardDesc: {
    fontSize: 12,
    color: "#64748B",
    lineHeight: 16,
  },
  scannerWrapper: {
    height: 220,
    borderRadius: 10,
    overflow: "hidden",
    backgroundColor: "#000000",
  },
  camera: {
    flex: 1,
  },
  cameraBox: {
    height: 180,
    alignItems: "center",
    justifyContent: "center",
  },
  scanOverlay: {
    flex: 1,
    backgroundColor: "rgba(0,0,0,0.3)",
    alignItems: "center",
    justifyContent: "center",
    gap: 10,
  },
  scanFrame: {
    width: 150,
    height: 150,
    borderWidth: 2,
    borderColor: "#FFFFFF",
    borderRadius: 10,
    backgroundColor: "transparent",
  },
  scanHintBox: {
    backgroundColor: "rgba(0,0,0,0.6)",
    paddingHorizontal: 8,
    paddingVertical: 3,
    borderRadius: 4,
  },
  scanHintText: {
    color: "#FFFFFF",
    fontSize: 10,
    fontWeight: "500",
  },
  cameraNotice: {
    padding: 20,
    alignItems: "center",
    justifyContent: "center",
    gap: 6,
  },
  cameraNoticeIcon: {
    fontSize: 26,
  },
  cameraNoticeTitle: {
    color: "#0F172A",
    fontSize: 13,
    fontWeight: "600",
  },
  cameraNoticeText: {
    color: "#64748B",
    fontSize: 11,
    textAlign: "center",
    lineHeight: 16,
  },
  permissionBtn: {
    backgroundColor: "#2563EB",
    borderRadius: 6,
    paddingHorizontal: 12,
    paddingVertical: 6,
    marginTop: 4,
  },
  permissionBtnText: {
    color: "#FFFFFF",
    fontSize: 11,
    fontWeight: "600",
  },
  codeInput: {
    backgroundColor: "#F8FAFC",
    borderWidth: 1,
    borderColor: "#CBD5E1",
    borderRadius: 8,
    color: "#0F172A",
    fontSize: 22,
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
    borderRadius: 10,
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
