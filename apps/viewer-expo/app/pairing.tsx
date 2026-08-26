import { useCallback, useEffect, useReducer, useRef } from "react";
import {
  ActivityIndicator,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { Ionicons } from "@expo/vector-icons";
import { CameraView, useCameraPermissions } from "expo-camera";
import { SafeAreaView } from "react-native-safe-area-context";
import { router, useLocalSearchParams } from "expo-router";
import { connectHost, controlHost } from "../src/session";
import {
  formatHostEndpoint,
  canSubmitPairingCode,
  pairWithHost,
  pairWithHostByCode,
  parseHostEndpoint,
  parseQrPayload,
  resolvePairingHost,
  type QrPayload,
} from "../src/pairing";
import { formatErrorMessage } from "../src/control";

type PairingMode = "qr" | "code";

interface PairingViewState {
  mode: PairingMode;
  code: string;
  scannedPayload: QrPayload | null;
  scannedHost: string;
  busy: boolean;
  statusMessage: string | null;
  error: string | null;
}

type PairingViewAction = {
  type: "update";
  patch: Partial<PairingViewState>;
};

const initialPairingViewState: PairingViewState = {
  mode: "qr",
  code: "",
  scannedPayload: null,
  scannedHost: "",
  busy: false,
  statusMessage: null,
  error: null,
};

function pairingViewReducer(
  state: PairingViewState,
  action: PairingViewAction,
): PairingViewState {
  return action.type === "update" ? { ...state, ...action.patch } : state;
}

function OtpPinInput({
  code,
  onChangeCode,
  disabled,
}: {
  code: string;
  onChangeCode: (val: string) => void;
  disabled: boolean;
}) {
  const inputRef = useRef<TextInput>(null);
  const digits = Array.from({ length: 6 }, (_, i) => code[i] ?? "");

  const handleBoxPress = () => {
    inputRef.current?.focus();
  };

  return (
    <Pressable onPress={handleBoxPress} style={styles.otpContainer}>
      <TextInput
        ref={inputRef}
        style={styles.hiddenTextInput}
        value={code}
        onChangeText={(text) => {
          const cleaned = text.replace(/[^0-9]/g, "").slice(0, 6);
          onChangeCode(cleaned);
        }}
        keyboardType="number-pad"
        maxLength={6}
        editable={!disabled}
        autoFocus={false}
        caretHidden
      />
      <View style={styles.otpBoxesRow}>
        {digits.map((digit, index) => {
          const isCurrent = index === code.length && !disabled;
          const isFilled = Boolean(digit);
          return (
            <View
              key={index}
              style={[
                styles.otpBox,
                isFilled && styles.otpBoxFilled,
                isCurrent && styles.otpBoxCurrent,
              ]}
            >
              <Text style={styles.otpDigit}>{digit}</Text>
            </View>
          );
        })}
      </View>
    </Pressable>
  );
}

export default function Pairing() {
  const params = useLocalSearchParams<{ endpoint?: string | string[] }>();
  const routeEndpoint = Array.isArray(params.endpoint) ? params.endpoint[0] : params.endpoint;
  const initialHost = resolvePairingHost(routeEndpoint, controlHost());
  const [state, dispatch] = useReducer(
    pairingViewReducer,
    {
      ...initialPairingViewState,
      mode: parseHostEndpoint(initialHost) ? "code" : "qr",
    },
  );
  const { mode, code, scannedPayload, scannedHost, busy, statusMessage, error } = state;
  const [permission, requestPermission] = useCameraPermissions();
  const host = scannedHost || resolvePairingHost(routeEndpoint, controlHost());
  const scanningLockRef = useRef(false);
  const hostEndpoint = parseHostEndpoint(host);
  const hasCodeTarget = Boolean(hostEndpoint);
  const canSubmitCode = canSubmitPairingCode(code, hasCodeTarget, busy);

  useEffect(() => {
    dispatch({ type: "update", patch: { error: null } });
  }, [code, mode]);

  const handlePairWithCode = useCallback(
    async (codeToPair: string) => {
      const trimmed = codeToPair.trim().replace(/\s+/g, "");
      if (trimmed.length !== 6) {
        dispatch({
          type: "update",
          patch: { error: "6자리 인증 코드를 정확히 입력해 주세요." },
        });
        return;
      }
      dispatch({
        type: "update",
        patch: {
          busy: true,
          error: null,
          statusMessage: "컴퓨터에서 연결을 확인하는 중…",
        },
      });
      try {
        if (scannedPayload) {
          await pairWithHost(scannedPayload, trimmed);
          await connectHost(scannedPayload.host, scannedPayload.port);
        } else if (hostEndpoint) {
          await pairWithHostByCode(hostEndpoint.host, hostEndpoint.port, trimmed);
          await connectHost(hostEndpoint.host, hostEndpoint.port);
        } else {
          throw new Error("연결할 컴퓨터 주소가 없습니다.");
        }
        router.replace("/catalog");
      } catch (e) {
        dispatch({ type: "update", patch: { error: formatErrorMessage(e) } });
      } finally {
        dispatch({ type: "update", patch: { busy: false, statusMessage: null } });
      }
    },
    [hostEndpoint, scannedPayload],
  );

  const handleQrScanned = useCallback(
    async (scannedData: string) => {
      if (busy || scanningLockRef.current) return;
      scanningLockRef.current = true;
      try {
        const payload = parseQrPayload(scannedData);
        if (!payload) {
          dispatch({
            type: "update",
            patch: { error: "Leftcar에서 만든 연결 QR 코드가 아닙니다." },
          });
          return;
        }
        dispatch({
          type: "update",
          patch: {
            busy: true,
            error: null,
            scannedPayload: payload,
            scannedHost: formatHostEndpoint(payload.host, payload.port),
            mode: "code",
            statusMessage: "QR 코드를 확인했습니다. 컴퓨터 화면의 6자리 번호를 입력해 주세요.",
          },
        });
      } catch (e) {
        dispatch({ type: "update", patch: { error: formatErrorMessage(e) } });
      } finally {
        dispatch({ type: "update", patch: { busy: false, statusMessage: null } });
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
              연결 대상: <Text style={styles.hostAddr}>{host}</Text>
            </Text>
          </View>
        ) : null}

        {/* Segmented Mode Selector */}
        <View style={styles.modeTabsWrapper}>
          <View style={styles.modeTabs}>
            <Pressable
              style={[styles.modeTab, mode === "qr" && styles.modeTabActive]}
              onPress={() => dispatch({ type: "update", patch: { mode: "qr" } })}
            >
              <View style={styles.tabContentRow}>
                <Ionicons
                  name="qr-code-outline"
                  size={14}
                  color={mode === "qr" ? "#FFFFFF" : "#71717A"}
                />
                <Text style={[styles.modeTabText, mode === "qr" && styles.modeTabTextActive]}>
                  QR 코드 스캔
                </Text>
              </View>
            </Pressable>
            <Pressable
              style={[styles.modeTab, mode === "code" && styles.modeTabActive]}
              onPress={() => dispatch({ type: "update", patch: { mode: "code" } })}
            >
              <View style={styles.tabContentRow}>
                <Ionicons
                  name="keypad-outline"
                  size={14}
                  color={mode === "code" ? "#FFFFFF" : "#71717A"}
                />
                <Text style={[styles.modeTabText, mode === "code" && styles.modeTabTextActive]}>
                  6자리 PIN 입력
                </Text>
              </View>
            </Pressable>
          </View>
        </View>

        {/* Status Alert */}
        {statusMessage && (
          <View style={styles.statusCard}>
            <ActivityIndicator size="small" color="#09090B" />
            <Text style={styles.statusText}>{statusMessage}</Text>
          </View>
        )}

        {/* Error Alert */}
        {error && (
          <View style={styles.errorCard}>
            <Ionicons name="alert-circle" size={16} color="#09090B" />
            <Text style={styles.errorText}>{error}</Text>
          </View>
        )}

        {/* Mode View */}
        {mode === "qr" ? (
          <View style={styles.card}>
            <Text style={styles.cardTitle}>QR 코드 스캔</Text>
            <Text style={styles.cardDesc}>
              컴퓨터의 Leftcar Host Studio 화면에 띄운 QR 코드를 비춰 주세요.
            </Text>

            {!permission ? (
              <View style={styles.cameraBox}>
                <ActivityIndicator color="#09090B" />
              </View>
            ) : !permission.granted ? (
              <View style={styles.cameraNotice}>
                <Ionicons name="camera-outline" size={28} color="#09090B" style={{ marginBottom: 4 }} />
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
            <Text style={styles.cardTitle}>6자리 인증 PIN 번호</Text>
            <Text style={styles.cardDesc}>
              {hasCodeTarget
                ? "컴퓨터 화면에 표시된 6자리 번호를 입력하세요."
                : "먼저 연결할 컴퓨터 주소를 선택하거나 입력하세요."}
            </Text>

            <OtpPinInput
              code={code}
              onChangeCode={(value) => dispatch({ type: "update", patch: { code: value } })}
              disabled={busy}
            />

            {code.length === 6 && !hasCodeTarget ? (
              <Text style={styles.otpHint}>
                연결할 컴퓨터 주소가 있어야 연결 승인 버튼이 활성화됩니다.
              </Text>
            ) : null}

            <Pressable
              style={[
                styles.primaryBtn,
                !canSubmitCode && styles.btnDisabled,
              ]}
              onPress={() => handlePairWithCode(code)}
              disabled={!canSubmitCode}
            >
              {busy ? (
                <ActivityIndicator color="#FFFFFF" size="small" />
              ) : (
                <Text style={styles.primaryBtnText}>연결 승인하기</Text>
              )}
            </Pressable>
          </View>
        )}

        {/* Security / Help Card */}
        <View style={styles.tipBox}>
          <View style={styles.tipTitleRow}>
            <Ionicons name="shield-checkmark-outline" size={15} color="#09090B" />
            <Text style={styles.tipTitle}>안전한 기기 페어링</Text>
          </View>
          <Text style={styles.tipText}>
            • Host 주소와 화면에 표시된 6자리 번호를 확인해야 연결이 허용됩니다.
          </Text>
          <Text style={styles.tipText}>
            • 6자리 연결 코드는 Host에서 새 연결을 만들 때마다 갱신됩니다.
          </Text>
          <Text style={styles.tipText}>
            • 신뢰하는 동일한 Wi-Fi 네트워크에서만 사용하세요.
          </Text>
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
  hostStrip: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#E4E4E7",
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
    backgroundColor: "#09090B",
    flexShrink: 0,
  },
  hostText: {
    color: "#71717A",
    fontSize: 12,
    flex: 1,
  },
  hostAddr: {
    color: "#09090B",
    fontWeight: "700",
    fontFamily: "monospace",
    fontVariant: ["tabular-nums"],
  },
  modeTabsWrapper: {
    backgroundColor: "#FFFFFF",
    borderRadius: 8,
    borderWidth: 1,
    borderColor: "#E4E4E7",
    padding: 3,
  },
  modeTabs: {
    flexDirection: "row",
    gap: 3,
  },
  modeTab: {
    flex: 1,
    paddingVertical: 7,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 6,
  },
  modeTabActive: {
    backgroundColor: "#09090B",
  },
  modeTabText: {
    color: "#71717A",
    fontSize: 12,
    fontWeight: "600",
  },
  modeTabTextActive: {
    color: "#FFFFFF",
    fontWeight: "700",
  },
  tabContentRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  statusCard: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#D4D4D8",
    borderRadius: 8,
    padding: 12,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  statusText: {
    color: "#09090B",
    fontSize: 12,
    fontWeight: "600",
    flex: 1,
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
  card: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    borderRadius: 12,
    padding: 16,
    gap: 12,
  },
  cardTitle: {
    fontSize: 14,
    fontWeight: "700",
    color: "#09090B",
  },
  cardDesc: {
    fontSize: 12,
    color: "#52525B",
    lineHeight: 17,
  },
  scannerWrapper: {
    height: 220,
    borderRadius: 8,
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
    backgroundColor: "rgba(0,0,0,0.35)",
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
    backgroundColor: "rgba(0,0,0,0.7)",
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
  cameraNoticeTitle: {
    color: "#09090B",
    fontSize: 13,
    fontWeight: "700",
  },
  cameraNoticeText: {
    color: "#71717A",
    fontSize: 11,
    textAlign: "center",
    lineHeight: 16,
  },
  permissionBtn: {
    backgroundColor: "#09090B",
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

  /* OTP PIN Split Boxes */
  otpContainer: {
    alignItems: "center",
    justifyContent: "center",
    marginVertical: 4,
    position: "relative",
  },
  hiddenTextInput: {
    position: "absolute",
    width: 1,
    height: 1,
    opacity: 0,
  },
  otpBoxesRow: {
    flexDirection: "row",
    gap: 8,
  },
  otpBox: {
    width: 42,
    height: 48,
    borderRadius: 8,
    borderWidth: 1.5,
    borderColor: "#E4E4E7",
    backgroundColor: "#FAFAFA",
    alignItems: "center",
    justifyContent: "center",
  },
  otpBoxFilled: {
    borderColor: "#09090B",
    backgroundColor: "#FFFFFF",
  },
  otpBoxCurrent: {
    borderColor: "#09090B",
    backgroundColor: "#FFFFFF",
  },
  otpDigit: {
    fontSize: 20,
    fontWeight: "700",
    color: "#09090B",
    fontFamily: "monospace",
    fontVariant: ["tabular-nums"],
  },
  otpHint: {
    color: "#71717A",
    fontSize: 11,
    textAlign: "center",
    lineHeight: 16,
    marginTop: 2,
  },

  primaryBtn: {
    backgroundColor: "#09090B",
    borderRadius: 8,
    paddingVertical: 11,
    alignItems: "center",
    justifyContent: "center",
  },
  btnDisabled: {
    opacity: 0.4,
  },
  primaryBtnText: {
    color: "#FFFFFF",
    fontSize: 13,
    fontWeight: "600",
  },
  tipBox: {
    backgroundColor: "#F4F4F5",
    borderRadius: 10,
    borderWidth: 1,
    borderColor: "#E4E4E7",
    padding: 14,
    gap: 5,
  },
  tipTitleRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  tipTitle: {
    color: "#09090B",
    fontSize: 12,
    fontWeight: "700",
  },
  tipText: {
    color: "#71717A",
    fontSize: 11,
    lineHeight: 16,
  },
});
