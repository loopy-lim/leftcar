import { useCallback, useEffect, useMemo, useReducer, useRef, useState, type ReactNode } from "react";
import {
  ActivityIndicator,
  AppState,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
  useWindowDimensions,
} from "react-native";
import { Ionicons } from "@expo/vector-icons";
import { CameraView, useCameraPermissions } from "expo-camera";
import * as Linking from "expo-linking";
import { applyPanelDensity, panelDensityScale } from "../src/panel-density";
import { resolveCameraState, type CameraState } from "../src/cameraPermission";
import { SafeAreaView } from "react-native-safe-area-context";
import { router, useLocalSearchParams } from "expo-router";
import { controlHost } from "../src/session";
import {
  PairingWorkflow,
  runPinPairingWorkflow,
  runQrPairingWorkflow,
} from "../src/connect-flow";
import {
  formatHostEndpoint,
  canSubmitPairingCode,
  isPairingRejectedError,
  isPairingUnsupportedError,
  parseHostEndpoint,
  parseQrPayload,
  resolvePairingHost,
  type QrPayload,
} from "../src/pairing";
import { formatErrorMessage } from "../src/control";
import { useAppTheme, type ThemeTokens } from "../src/theme";
import { useAppLanguage, type TranslationSchema } from "../src/i18n";

type PairingMode = "qr" | "code";

/** 카메라가 살아 있어도 QR이 안 뜨는(호스트 창이 닫힌) 상황의 실패 지점 힌트 지연. */
const QR_SCAN_IDLE_HINT_MS = 8_000;

interface PairingViewState {
  mode: PairingMode;
  code: string;
  scannedPayload: QrPayload | null;
  scannedHost: string;
  busy: boolean;
  statusMessage: string | null;
  /** 모드 전환 등 화면 변화의 이유를 알리는 1회성 안내. 입력·탭 전환으로 지운다. */
  notice: string | null;
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
  notice: null,
  error: null,
};

/**
 * 401로 진입한 경우(연결 승인 만료) 안내 창이 6자리 연결 코드를 가리켰으므로
 * 코드 입력을 기본으로 연다. 그 외 진입(버튼)은 QR 스캔이 기본이다.
 */
function initPairingViewState(routeEndpoint: string): PairingViewState {
  return routeEndpoint
    ? { ...initialPairingViewState, mode: "code" }
    : initialPairingViewState;
}

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
  label,
  hint,
  colors,
}: {
  code: string;
  onChangeCode: (val: string) => void;
  disabled: boolean;
  label: string;
  hint: string;
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
        accessibilityLabel={label}
        accessibilityHint={hint}
      />
      {/* 숫자 상자는 장식이다 — 값은 숨은 TextInput이 음성으로 읽어 주므로
          스크린 리더에서는 문자열 조각으로 읽히지 않게 숨긴다. */}
      <View
        style={stylesLocal.otpBoxesRow}
        importantForAccessibility="no-hide-descendants"
        accessibilityElementsHidden
      >
        {digits.map((digit, index) => {
          const isCurrent = index === code.length && !disabled;
          const isFilled = Boolean(digit);
          let borderColor: string = colors.borderSubtle;
          if (isCurrent) {
            borderColor = colors.borderFocus;
          } else if (isFilled) {
            borderColor = colors.borderStrong;
          }
          return (
            <View
              key={index}
              style={[
                stylesLocal.otpBox,
                {
                  backgroundColor: colors.bgSubtle,
                  borderColor,
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

interface PairingModeCardProps {
  mode: PairingMode;
  cameraState: CameraState;
  onRequestPermission: () => void;
  onOpenSettings: () => void;
  code: string;
  busy: boolean;
  hasCodeTarget: boolean;
  canSubmitCode: boolean;
  colors: ThemeTokens;
  styles: ReturnType<typeof createStyles>;
  onCodeChange: (value: string) => void;
  onSubmit: () => void;
  onQrScanned: (value: string) => void;
}

function QrCameraLoading({ colors, styles }: { colors: ThemeTokens; styles: ReturnType<typeof createStyles> }) {
  return (
    <View style={styles.cameraBox}>
      <ActivityIndicator color={colors.textPrimary} />
    </View>
  );
}

function QrCameraPermissionNotice({
  colors,
  styles,
  t,
  blocked,
  onPrimary,
}: {
  colors: ThemeTokens;
  styles: ReturnType<typeof createStyles>;
  t: TranslationSchema;
  /** 권한이 영구 거부된 경우 — 재요청으로는 풀리지 않으므로 설정 앱으로 안내한다. */
  blocked: boolean;
  onPrimary: () => void;
}) {
  return (
    <View style={styles.cameraNotice}>
      <Ionicons
        name="camera-outline"
        size={28}
        color={colors.textPrimary}
        style={{ marginBottom: 4 }}
      />
      <Text style={styles.cameraNoticeTitle}>{t.viewer.cameraPermNeeded}</Text>
      <Text style={styles.cameraNoticeText}>{t.viewer.cameraPermDesc}</Text>
      <Pressable onPress={onPrimary} style={styles.permissionBtn}>
        <Text style={styles.permissionBtnText}>
          {blocked ? t.viewer.btnOpenAppSettings : t.viewer.btnGrantPerm}
        </Text>
      </Pressable>
    </View>
  );
}

function QrScanner({
  styles,
  t,
  showIdleHint,
  onQrScanned,
}: {
  styles: ReturnType<typeof createStyles>;
  t: TranslationSchema;
  showIdleHint: boolean;
  onQrScanned: (value: string) => void;
}) {
  return (
    <View style={styles.scannerWrapper}>
      <CameraView
        style={styles.camera}
        facing="back"
        barcodeScannerSettings={{ barcodeTypes: ["qr"] }}
        onBarcodeScanned={(result) => {
          const value = result.data?.trim();
          if (value) onQrScanned(value);
        }}
      >
        <View style={styles.scanOverlay}>
          <View style={styles.scanFrame}>
            <View style={[styles.cornerBracket, styles.cornerTopLeft]} />
            <View style={[styles.cornerBracket, styles.cornerTopRight]} />
            <View style={[styles.cornerBracket, styles.cornerBottomLeft]} />
            <View style={[styles.cornerBracket, styles.cornerBottomRight]} />
          </View>
          <View style={styles.scanHintBox}>
            <Text style={styles.scanHintText}>{t.viewer.qrScanHint}</Text>
            {showIdleHint ? (
              <Text style={styles.scanHintText}>{t.viewer.qrScanIdleHint}</Text>
            ) : null}
          </View>
        </View>
      </CameraView>
    </View>
  );
}

function PairingModeCard({
  mode,
  cameraState,
  onRequestPermission,
  onOpenSettings,
  code,
  busy,
  hasCodeTarget,
  canSubmitCode,
  colors,
  styles,
  onCodeChange,
  onSubmit,
  onQrScanned,
}: PairingModeCardProps) {
  const { t } = useAppLanguage();
  // 카메라가 켜져 있는데 QR을 못 읽는 상황은 대부분 Mac 쪽 연결 창이 닫힌
  // 것이다. 실패 지점에서 한 번, 다음 행동을 알려 준다.
  const [scanIdle, setScanIdle] = useState(false);
  const cameraLive = mode === "qr" && cameraState === "live";
  useEffect(() => {
    if (!cameraLive || busy) {
      setScanIdle(false);
      return;
    }
    const timer = setTimeout(() => setScanIdle(true), QR_SCAN_IDLE_HINT_MS);
    return () => clearTimeout(timer);
  }, [busy, cameraLive]);

  if (mode === "code") {
    return (
      <View style={styles.card}>
        <Text style={styles.cardTitle}>{t.viewer.pinTitle}</Text>
        <Text style={styles.cardDesc}>
          {hasCodeTarget ? t.viewer.pinDesc : t.viewer.pinNoTargetDesc}
        </Text>
        <OtpPinInput
          code={code}
          onChangeCode={onCodeChange}
          disabled={busy}
          label={t.viewer.pinTitle}
          hint={t.viewer.pinDesc}
          colors={colors}
        />
        <Pressable
          style={({ pressed }) => [
            styles.primaryBtn,
            !canSubmitCode && styles.btnDisabled,
            pressed && canSubmitCode && styles.btnPressed,
          ]}
          onPress={onSubmit}
          disabled={!canSubmitCode}
        >
          {busy ? (
            <ActivityIndicator color={colors.btnPrimaryText} size="small" />
          ) : (
            <Text style={styles.primaryBtnText}>{t.viewer.btnSubmitPin}</Text>
          )}
        </Pressable>
      </View>
    );
  }
  let cameraBody: ReactNode;
  if (cameraState === "loading") {
    cameraBody = <QrCameraLoading colors={colors} styles={styles} />;
  } else if (cameraState === "blocked") {
    cameraBody = (
      <QrCameraPermissionNotice
        colors={colors}
        styles={styles}
        t={t}
        blocked
        onPrimary={onOpenSettings}
      />
    );
  } else if (cameraState === "request") {
    cameraBody = (
      <QrCameraPermissionNotice
        colors={colors}
        styles={styles}
        t={t}
        blocked={false}
        onPrimary={onRequestPermission}
      />
    );
  } else {
    cameraBody = <QrScanner styles={styles} t={t} showIdleHint={scanIdle} onQrScanned={onQrScanned} />;
  }
  return (
    <View style={styles.card}>
      {cameraBody}
    </View>
  );
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

function usePairingWorkflow(routeEndpoint: string, t: TranslationSchema) {
  const [state, dispatch] = useReducer(
    pairingViewReducer,
    routeEndpoint,
    initPairingViewState,
  );
  const {
    mode,
    code,
    scannedPayload,
    scannedHost,
    busy,
    statusMessage,
    notice,
    error,
  } = state;

  const host = scannedHost || resolvePairingHost(routeEndpoint, controlHost());
  // QR/PIN 모두 같은 attempt 수명을 쓴다. 언마운트·새 시도는 저장, 연결,
  // 화면 전환 중 어디에 있든 이전 작업을 취소하고 그 시도의 저장을 되돌린다.
  const pairingWorkflow = useMemo(() => new PairingWorkflow(), []);
  // 자동 제출은 같은 코드를 두 번 제출하지 않는다 — 실패 후 busy가 풀려도
  // 사용자가 코드를 고칠 때까지 재시도 루프가 돌지 않는다.
  const lastAutoSubmittedCodeRef = useRef<string | null>(null);
  const hostEndpoint = parseHostEndpoint(host);
  const hasCodeTarget = Boolean(hostEndpoint);
  const canSubmitCode = canSubmitPairingCode(code, hasCodeTarget, busy);

  useEffect(() => {
    dispatch({ type: "update", patch: { error: null } });
  }, [code, mode]);

  useEffect(() => {
    dispatch({ type: "update", patch: { notice: null } });
  }, [code]);

  useEffect(
    () => () => {
      void pairingWorkflow.cancel();
    },
    [pairingWorkflow],
  );

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
      const run = pairingWorkflow.beginPin();
      try {
        const target = scannedPayload ?? hostEndpoint;
        if (!target) throw new Error(t.viewer.notConnectedError);
        await runPinPairingWorkflow({
          run,
          target,
          code: trimmed,
          navigate: () => router.replace("/catalog"),
        });
      } catch (e) {
        if (!(e instanceof Error && e.name === "AbortError")) {
          dispatch({ type: "update", patch: { error: formatErrorMessage(e) } });
        }
      } finally {
        if (pairingWorkflow.finish(run)) {
          dispatch({ type: "update", patch: { busy: false, statusMessage: null } });
        }
      }
    },
    [hostEndpoint, pairingWorkflow, scannedPayload, t],
  );

  // Auto-submit code when 6 digits are typed and target is ready
  useEffect(() => {
    if (
      code.length === 6 &&
      canSubmitCode &&
      !busy &&
      lastAutoSubmittedCodeRef.current !== code
    ) {
      lastAutoSubmittedCodeRef.current = code;
      void handlePairWithCode(code);
    }
  }, [code, canSubmitCode, busy, handlePairWithCode]);

  const handleQrScanned = useCallback(
    async (scannedData: string) => {
      const payload = parseQrPayload(scannedData);
      if (!payload) {
        dispatch({ type: "update", patch: { error: t.viewer.invalidQrError } });
        return;
      }
      const run = pairingWorkflow.beginQr(payload);
      // CameraView may emit the same visible barcode repeatedly. The approval
      // offer already in flight owns that event identity and continues.
      if (!run) return;
      dispatch({ type: "update", patch: { busy: true, error: null, statusMessage: null } });
      try {
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
        await runQrPairingWorkflow({
          run,
          payload,
          onPending: () => {
            if (!run.attempt.signal.aborted) {
              dispatch({
                type: "update",
                patch: { statusMessage: t.viewer.qrApprovalWaitDesc },
              });
            }
          },
          navigate: () => router.replace("/catalog"),
        });
        return;
      } catch (e) {
        if (e instanceof Error && e.name === "AbortError") {
          // 화면 이탈·새 스캔으로 취소된 폴링 — 조용히 끝낸다.
          return;
        }
        if (isPairingRejectedError(e)) {
          dispatch({
            type: "update",
            patch: {
              error: t.viewer.errPairingDeclined,
              scannedPayload: null,
              scannedHost: "",
            },
          });
        } else if (isPairingUnsupportedError(e)) {
          // 구버전 호스트는 시크릿만으로 pair을 받지 않는다 — 6자리 입력으로
          // 전환한다(스캔한 대상은 그대로 유지). 모드가 바뀐 이유를 실패
          // 지점에서 한 번 알려 준다.
          dispatch({
            type: "update",
            patch: { mode: "code", notice: t.viewer.pinFallbackNotice },
          });
        } else {
          dispatch({
            type: "update",
            patch: { error: formatErrorMessage(e), scannedPayload: null, scannedHost: "" },
          });
        }
      } finally {
        if (pairingWorkflow.finish(run)) {
          dispatch({ type: "update", patch: { busy: false, statusMessage: null } });
        }
      }
    },
    [pairingWorkflow, t],
  );

  return {
    ...state,
    dispatch,
    host,
    hasCodeTarget,
    canSubmitCode,
    handlePairWithCode,
    handleQrScanned,
  };
}

export default function Pairing() {
  const { colors, isDark } = useAppTheme();
  const { t } = useAppLanguage();
  const { width } = useWindowDimensions();
  const density = panelDensityScale(width);
  const styles = useMemo(
    () => applyPanelDensity(createStyles(colors, isDark), density),
    [colors, isDark, density],
  );

  const params = useLocalSearchParams<{ endpoint?: string }>();
  const routeEndpoint = params.endpoint?.trim() || "";
  const [permission, requestPermission, getPermission] = useCameraPermissions();
  const cameraState = resolveCameraState(permission);
  const {
    mode,
    code,
    busy,
    statusMessage,
    notice,
    error,
    dispatch,
    host,
    hasCodeTarget,
    canSubmitCode,
    handlePairWithCode,
    handleQrScanned,
  } = usePairingWorkflow(routeEndpoint, t);

  const [cameraError, setCameraError] = useState<string | null>(null);
  const cameraRequest = useRef(0);
  useEffect(() => () => { cameraRequest.current += 1; }, []);
  const runCameraAction = useCallback(async (action: () => Promise<unknown>) => {
    const request = ++cameraRequest.current;
    setCameraError(null);
    try { await action(); }
    catch (failure) {
      if (cameraRequest.current === request) setCameraError(formatErrorMessage(failure));
    }
  }, []);
  const refreshPermission = useCallback(() => {
    void runCameraAction(getPermission);
  }, [getPermission, runCameraAction]);
  useEffect(() => {
    const subscription = AppState.addEventListener("change", (status) => {
      if (status === "active") refreshPermission();
    });
    return () => subscription.remove();
  }, [refreshPermission]);
  const openAppSettings = useCallback(() => {
    void runCameraAction(Linking.openSettings);
  }, [runCameraAction]);

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
              onPress={() => dispatch({ type: "update", patch: { mode: "qr", notice: null } })}
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
              onPress={() => dispatch({ type: "update", patch: { mode: "code", notice: null } })}
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

        {/* Mode-switch Notice (e.g. QR → pairing code fallback) */}
        {notice && (
          <View style={styles.noticeCard}>
            <Ionicons name="information-circle" size={16} color={colors.textPrimary} />
            <Text style={styles.statusText}>{notice}</Text>
          </View>
        )}

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

        {cameraError && (
          <View style={styles.errorCard} accessibilityRole="alert">
            <Text style={styles.errorText}>{cameraError}</Text>
            <Pressable onPress={refreshPermission}><Text>{t.common.retry}</Text></Pressable>
          </View>
        )}
        <PairingModeCard mode={mode} cameraState={cameraState} onRequestPermission={() => { void runCameraAction(requestPermission); }} onOpenSettings={openAppSettings} code={code} busy={busy} hasCodeTarget={hasCodeTarget} canSubmitCode={canSubmitCode} colors={colors} styles={styles} onCodeChange={(value: string) => dispatch({ type: "update", patch: { code: value } })} onSubmit={() => void handlePairWithCode(code)} onQrScanned={handleQrScanned} />
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
    noticeCard: {
      backgroundColor: colors.bgSurface,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
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
