import { useCallback, useEffect, useMemo, useReducer, useRef } from "react";
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
  isPairingUnsupportedError,
  pairWithHost,
  pairWithHostApproval,
  pairWithHostByCode,
  parseHostEndpoint,
  parseQrPayload,
  resolvePairingHost,
  type QrPayload,
} from "../src/pairing";
import { formatErrorMessage } from "../src/control";
import { useAppTheme, type ThemeTokens } from "../src/theme";
import { useAppLanguage } from "../src/i18n";

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
  colors,
}: {
  code: string;
  onChangeCode: (val: string) => void;
  disabled: boolean;
  colors: ThemeTokens;
}) {
  const inputRef = useRef<TextInput>(null);
  const digits = Array.from({ length: 6 }, (_, i) => code[i] ?? "");

  const handleBoxPress = () => {
    inputRef.current?.focus();
  };

  return (
    <Pressable onPress={handleBoxPress} style={stylesLocal.otpContainer}>
      <TextInput
        ref={inputRef}
        style={stylesLocal.hiddenTextInput}
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
      <View style={stylesLocal.otpBoxesRow}>
        {digits.map((digit, index) => {
          const isCurrent = index === code.length && !disabled;
          const isFilled = Boolean(digit);
          return (
            <View
              key={index}
              style={[
                stylesLocal.otpBox,
                {
                  backgroundColor: colors.bgSubtle,
                  borderColor: isCurrent
                    ? colors.borderFocus
                    : isFilled
                      ? colors.borderStrong
                      : colors.borderSubtle,
                },
              ]}
            >
              <Text
                style={[
                  stylesLocal.otpDigit,
                  { color: isFilled ? colors.textPrimary : colors.textDim },
                ]}
              >
                {digit || (isCurrent ? "·" : "")}
              </Text>
            </View>
          );
        })}
      </View>
    </Pressable>
  );
}

function PairingModeCard({ mode, permission, requestPermission, code, busy, hasCodeTarget, canSubmitCode, colors, styles, onCodeChange, onSubmit, onQrScanned }: { mode: PairingMode; permission: { granted: boolean } | null | undefined; requestPermission: () => void; code: string; busy: boolean; hasCodeTarget: boolean; canSubmitCode: boolean; colors: ThemeTokens; styles: ReturnType<typeof createStyles>; onCodeChange: (value: string) => void; onSubmit: () => void; onQrScanned: (value: string) => void }) {
  const { t } = useAppLanguage();
  if (mode === "code") return <View style={styles.card}>
    <Text style={styles.cardTitle}>{t.viewer.pinTitle}</Text>
    <Text style={styles.cardDesc}>{hasCodeTarget ? t.viewer.pinDesc : t.viewer.pinNoTargetDesc}</Text>
    <OtpPinInput code={code} onChangeCode={onCodeChange} disabled={busy} colors={colors} />
    <Pressable style={({ pressed }) => [styles.primaryBtn, !canSubmitCode && styles.btnDisabled, pressed && canSubmitCode && styles.btnPressed]} onPress={onSubmit} disabled={!canSubmitCode}>
      {busy ? <ActivityIndicator color={colors.btnPrimaryText} size="small" /> : <Text style={styles.primaryBtnText}>{t.viewer.btnSubmitPin}</Text>}
    </Pressable>
  </View>;
  return <View style={styles.card}>
    <Text style={styles.cardTitle}>{t.viewer.tabQr}</Text>
    {!permission ? <View style={styles.cameraBox}><ActivityIndicator color={colors.textPrimary} /></View> : !permission.granted ? <View style={styles.cameraNotice}>
      <Ionicons name="camera-outline" size={28} color={colors.textPrimary} style={{ marginBottom: 4 }} /><Text style={styles.cameraNoticeTitle}>{t.viewer.cameraPermNeeded}</Text><Text style={styles.cameraNoticeText}>{t.viewer.cameraPermDesc}</Text><Pressable onPress={requestPermission} style={styles.permissionBtn}><Text style={styles.permissionBtnText}>{t.viewer.btnGrantPerm}</Text></Pressable>
    </View> : <View style={styles.scannerWrapper}><CameraView style={styles.camera} facing="back" barcodeScannerSettings={{ barcodeTypes: ["qr"] }} onBarcodeScanned={(result) => { const value = result.data?.trim(); if (value) onQrScanned(value); }}><View style={styles.scanOverlay}><View style={styles.scanFrame}><View style={[styles.cornerBracket, styles.cornerTopLeft]} /><View style={[styles.cornerBracket, styles.cornerTopRight]} /><View style={[styles.cornerBracket, styles.cornerBottomLeft]} /><View style={[styles.cornerBracket, styles.cornerBottomRight]} /></View><View style={styles.scanHintBox}><Text style={styles.scanHintText}>{t.viewer.qrScanHint}</Text></View></View></CameraView></View>}
  </View>;
}

const stylesLocal = StyleSheet.create({
  otpContainer: {
    alignItems: "center",
    justifyContent: "center",
    marginVertical: 6,
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
    width: 44,
    height: 50,
    borderRadius: 8,
    borderWidth: 1.5,
    alignItems: "center",
    justifyContent: "center",
  },
  otpDigit: {
    fontSize: 20,
    fontWeight: "700",
    fontFamily: "monospace",
    fontVariant: ["tabular-nums"],
  },
});

export default function Pairing() {
  const { colors, isDark } = useAppTheme();
  const { t } = useAppLanguage();
  const styles = useMemo(() => createStyles(colors, isDark), [colors, isDark]);

  const params = useLocalSearchParams<{ endpoint?: string }>();
  const routeEndpoint = params.endpoint?.trim() || "";

  const [permission, requestPermission] = useCameraPermissions();

  const [state, dispatch] = useReducer(
    pairingViewReducer,
    initialPairingViewState,
  );
  const { mode, code, scannedPayload, scannedHost, busy, statusMessage, error } =
    state;

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
          patch: { error: t.viewer.invalidPinError },
        });
        return;
      }
      dispatch({
        type: "update",
        patch: {
          busy: true,
          error: null,
          statusMessage: t.viewer.pairingBusy,
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
          throw new Error(t.viewer.notConnectedError);
        }
        router.replace("/catalog");
      } catch (e) {
        dispatch({ type: "update", patch: { error: formatErrorMessage(e) } });
      } finally {
        dispatch({ type: "update", patch: { busy: false, statusMessage: null } });
      }
    },
    [hostEndpoint, scannedPayload, t],
  );

  // Auto-submit code when 6 digits are typed and target is ready
  useEffect(() => {
    if (code.length === 6 && canSubmitCode && !busy) {
      void handlePairWithCode(code);
    }
  }, [code, canSubmitCode, busy, handlePairWithCode]);

  const handleQrScanned = useCallback(
    async (scannedData: string) => {
      if (busy || scanningLockRef.current) return;
      scanningLockRef.current = true;
      dispatch({ type: "update", patch: { busy: true, error: null, statusMessage: null } });
      try {
        const payload = parseQrPayload(scannedData);
        if (!payload) {
          dispatch({
            type: "update",
            patch: { error: t.viewer.invalidQrError },
          });
          return;
        }
        // QR 스캔으로 페어링이 완결된다: 시크릿을 제시하고 Mac 화면의
        // [허용]을 기다린다. 카메라는 계속 켜져 있어 다른 QR로 재시도도
        // 바로 가능하다.
        dispatch({
          type: "update",
          patch: {
            scannedPayload: payload,
            scannedHost: formatHostEndpoint(payload.host, payload.port),
            statusMessage: t.viewer.qrApprovalWaitDesc,
          },
        });
        const result = await pairWithHostApproval(payload, {
          onPending: () =>
            dispatch({
              type: "update",
              patch: { statusMessage: t.viewer.qrApprovalWaitDesc },
            }),
        });
        if (result.kind === "approved") {
          await connectHost(payload.host, payload.port);
          router.replace("/catalog");
          return;
        }
        dispatch({
          type: "update",
          patch: {
            error: t.viewer.errPairingDeclined,
            scannedPayload: null,
            scannedHost: "",
          },
        });
      } catch (e) {
        if (isPairingUnsupportedError(e)) {
          // 구버전 호스트는 시크릿만으로 pair을 받지 않는다 — 6자리 입력으로
          // 전환한다(스캔한 대상은 그대로 유지).
          dispatch({ type: "update", patch: { mode: "code" } });
        } else {
          dispatch({
            type: "update",
            patch: { error: formatErrorMessage(e), scannedPayload: null, scannedHost: "" },
          });
        }
      } finally {
        dispatch({ type: "update", patch: { busy: false, statusMessage: null } });
        setTimeout(() => {
          scanningLockRef.current = false;
        }, 1500);
      }
    },
    [busy, t],
  );

  return (
    <SafeAreaView style={styles.safeArea} edges={["left", "right", "bottom"]}>
      <ScrollView
        style={styles.root}
        contentContainerStyle={styles.content}
        showsVerticalScrollIndicator={false}
      >
        {/* Connected Host Info (if known) */}
        {host ? (
          <View style={styles.hostStrip}>
            <View style={styles.hostDot} />
            <Text style={styles.hostText} numberOfLines={1}>
              {t.viewer.connectedHostLabel} <Text style={styles.hostAddr}>{host}</Text>
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
                  color={mode === "qr" ? colors.btnPrimaryText : colors.textMuted}
                />
                <Text style={[styles.modeTabText, mode === "qr" && styles.modeTabTextActive]}>
                  {t.viewer.tabQr}
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
                  color={mode === "code" ? colors.btnPrimaryText : colors.textMuted}
                />
                <Text style={[styles.modeTabText, mode === "code" && styles.modeTabTextActive]}>
                  {t.viewer.tabPin}
                </Text>
              </View>
            </Pressable>
          </View>
        </View>

        {/* Status Alert */}
        {statusMessage && (
          <View style={styles.statusCard}>
            <ActivityIndicator size="small" color={colors.textPrimary} />
            <Text style={styles.statusText}>{statusMessage}</Text>
          </View>
        )}

        {/* Error Alert */}
        {error && (
          <View style={styles.errorCard}>
            <Ionicons name="alert-circle" size={16} color={colors.textPrimary} />
            <Text style={styles.errorText}>{error}</Text>
          </View>
        )}

        <PairingModeCard mode={mode} permission={permission} requestPermission={requestPermission} code={code} busy={busy} hasCodeTarget={hasCodeTarget} canSubmitCode={canSubmitCode} colors={colors} styles={styles} onCodeChange={(value: string) => dispatch({ type: "update", patch: { code: value } })} onSubmit={() => void handlePairWithCode(code)} onQrScanned={handleQrScanned} />
      </ScrollView>
    </SafeAreaView>
  );
}

function createStyles(colors: ThemeTokens, isDark: boolean) {
  return StyleSheet.create({
    safeArea: {
      flex: 1,
      backgroundColor: colors.bgCanvas,
    },
    root: {
      flex: 1,
      backgroundColor: colors.bgCanvas,
    },
    content: {
      paddingHorizontal: 18,
      paddingTop: 14,
      paddingBottom: 32,
      gap: 14,
    },
    hostStrip: {
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 10,
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
      backgroundColor: colors.textPrimary,
      flexShrink: 0,
    },
    hostText: {
      color: colors.textMuted,
      fontSize: 12,
      flex: 1,
    },
    hostAddr: {
      color: colors.textPrimary,
      fontWeight: "700",
      fontFamily: "monospace",
      fontVariant: ["tabular-nums"],
    },
    modeTabsWrapper: {
      backgroundColor: colors.bgSurface,
      borderRadius: 10,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
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
      borderRadius: 7,
    },
    modeTabActive: {
      backgroundColor: colors.btnPrimaryBg,
    },
    modeTabText: {
      color: colors.textMuted,
      fontSize: 12,
      fontWeight: "600",
    },
    modeTabTextActive: {
      color: colors.btnPrimaryText,
      fontWeight: "700",
    },
    tabContentRow: {
      flexDirection: "row",
      alignItems: "center",
      gap: 6,
    },
    statusCard: {
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderCard,
      borderRadius: 10,
      padding: 12,
      flexDirection: "row",
      alignItems: "center",
      gap: 8,
    },
    statusText: {
      color: colors.textPrimary,
      fontSize: 12,
      fontWeight: "600",
      flex: 1,
    },
    errorCard: {
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderStrong,
      borderRadius: 10,
      padding: 12,
      flexDirection: "row",
      alignItems: "center",
      gap: 8,
    },
    errorText: {
      color: colors.textPrimary,
      fontSize: 12,
      lineHeight: 16,
      flex: 1,
      fontWeight: "500",
    },
    card: {
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      borderRadius: 14,
      padding: 16,
      gap: 12,
    },
    cardTitle: {
      fontSize: 14,
      fontWeight: "700",
      color: colors.textPrimary,
    },
    cardDesc: {
      fontSize: 12,
      color: colors.textSecondary,
      lineHeight: 17,
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
      backgroundColor: "rgba(0,0,0,0.4)",
      alignItems: "center",
      justifyContent: "center",
      gap: 10,
    },
    scanFrame: {
      width: 160,
      height: 160,
      position: "relative",
    },
    cornerBracket: {
      position: "absolute",
      width: 20,
      height: 20,
      borderColor: "#FFFFFF",
    },
    cornerTopLeft: {
      top: 0,
      left: 0,
      borderTopWidth: 3,
      borderLeftWidth: 3,
      borderTopLeftRadius: 4,
    },
    cornerTopRight: {
      top: 0,
      right: 0,
      borderTopWidth: 3,
      borderRightWidth: 3,
      borderTopRightRadius: 4,
    },
    cornerBottomLeft: {
      bottom: 0,
      left: 0,
      borderBottomWidth: 3,
      borderLeftWidth: 3,
      borderBottomLeftRadius: 4,
    },
    cornerBottomRight: {
      bottom: 0,
      right: 0,
      borderBottomWidth: 3,
      borderRightWidth: 3,
      borderBottomRightRadius: 4,
    },
    scanHintBox: {
      backgroundColor: "rgba(0,0,0,0.75)",
      paddingHorizontal: 10,
      paddingVertical: 4,
      borderRadius: 6,
    },
    scanHintText: {
      color: "#FFFFFF",
      fontSize: 10,
      fontWeight: "600",
    },
    cameraNotice: {
      padding: 20,
      alignItems: "center",
      justifyContent: "center",
      gap: 6,
    },
    cameraNoticeTitle: {
      color: colors.textPrimary,
      fontSize: 13,
      fontWeight: "700",
    },
    cameraNoticeText: {
      color: colors.textMuted,
      fontSize: 11,
      textAlign: "center",
      lineHeight: 16,
    },
    permissionBtn: {
      backgroundColor: colors.btnPrimaryBg,
      borderRadius: 6,
      paddingHorizontal: 12,
      paddingVertical: 6,
      marginTop: 4,
    },
    permissionBtnText: {
      color: colors.btnPrimaryText,
      fontSize: 11,
      fontWeight: "600",
    },
    otpHint: {
      color: colors.textMuted,
      fontSize: 11,
      textAlign: "center",
      lineHeight: 16,
      marginTop: 2,
    },
    primaryBtn: {
      backgroundColor: colors.btnPrimaryBg,
      borderRadius: 8,
      paddingVertical: 11,
      alignItems: "center",
      justifyContent: "center",
    },
    btnDisabled: {
      opacity: 0.4,
    },
    primaryBtnText: {
      color: colors.btnPrimaryText,
      fontSize: 13,
      fontWeight: "600",
    },
    btnPressed: {
      opacity: 0.8,
      transform: [{ scale: 0.98 }],
    },
  });
}
