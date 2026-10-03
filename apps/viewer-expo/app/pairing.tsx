import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import {
  ActivityIndicator,
  AppState,
  Pressable,
  ScrollView,
  TextInput,
  View,
} from "react-native";
import { CameraView, useCameraPermissions } from "expo-camera";
import { withUniwind } from "uniwind";
import { deviceClipboardIo } from "../src/clipboard-sync";
import * as Linking from "expo-linking";
import { resolveCameraState, type CameraState } from "../src/cameraPermission";
import { router, useLocalSearchParams, useFocusEffect } from "expo-router";
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
import { useAppLanguage, type TranslationSchema } from "../src/i18n";
import {
  SafeArea,
  Action,
  Field,
  Label,
  Notice,
  Surface,
} from "../src/ui/primitives";
import { cn } from "@leftcar/ui-tokens";

const StyledCameraView = withUniwind(CameraView);

type PairingMode = "qr" | "code";
interface PairingViewState {
  mode: PairingMode;
  code: string;
  scannedPayload: QrPayload | null;
  scannedHost: string;
  busy: boolean;
  scanPaused: boolean;
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
  scanPaused: false,
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
  t,
}: {
  code: string;
  onChangeCode: (value: string) => void;
  disabled: boolean;
  t: TranslationSchema;
}) {
  const input = useRef<TextInput>(null);
  const [focused, setFocused] = useState(false);
  const [pasteError, setPasteError] = useState<string | null>(null);
  const pasteRequest = useRef(0);
  useEffect(
    () => () => {
      pasteRequest.current += 1;
    },
    [],
  );
  useEffect(() => {
    if (disabled) pasteRequest.current += 1;
  }, [disabled]);
  const pasteCode = () => {
    if (disabled || !input.current) return;
    const request = ++pasteRequest.current;
    return deviceClipboardIo.readDeviceClipboard()
      .then((raw) => {
        if (request !== pasteRequest.current || !input.current) return;
        const value = raw.replace(/[^0-9]/g, "").slice(0, 6);
        if (value.length !== 6) {
          setPasteError(t.viewer.invalidPinError);
          return;
        }
        setPasteError(null);
        onChangeCode(value);
        input.current?.focus();
      })
      .catch((cause) => {
        if (request === pasteRequest.current)
          setPasteError(formatErrorMessage(cause));
      });
  };
  return (
    <View className="gap-3">
      <Pressable
        onPress={() => input.current?.focus()}
        accessible={false}
        className="relative min-h-14 w-full justify-center"
      >
        <Field
          ref={input}
          className="absolute inset-0 h-full w-full opacity-0"
          value={code}
          onChangeText={(value) => {
            setPasteError(null);
            onChangeCode(value.replace(/[^0-9]/g, "").slice(0, 6));
          }}
          keyboardType="number-pad"
          autoComplete="one-time-code"
          textContentType="oneTimeCode"
          editable={!disabled}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          caretHidden
          accessibilityLabel={t.viewer.pinTitle}
          accessibilityHint={t.viewer.pinDesc}
        />
        <View
          pointerEvents="none"
          className="flex-row gap-1.5"
          importantForAccessibility="no-hide-descendants"
          accessibilityElementsHidden
        >
          {Array.from({ length: 6 }, (_, index) => (
            <View
              key={index}
              className={cn(
                "h-14 min-w-0 flex-1 items-center justify-center rounded-md border bg-subtle",
                focused && index === Math.min(code.length, 5)
                  ? "border-ink"
                  : "border-outline",
              )}
            >
              <Label variant="heading" className="tabular-nums">
                {code[index] || "·"}
              </Label>
            </View>
          ))}
        </View>
      </Pressable>
      <Action
        variant="secondary"
        disabled={disabled}
        onPress={() => void pasteCode()}
        label={t.viewer.pinPaste}
      />
      {pasteError ? (
        <Notice tone="error">
          <Label>{pasteError}</Label>
        </Notice>
      ) : null}
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
  scanPaused,
  hasCodeTarget,
  canSubmitCode,
  onCodeChange,
  onSubmit,
  onQrScanned,
}: {
  mode: PairingMode;
  cameraState: CameraState;
  onRequestPermission: () => void;
  onOpenSettings: () => void;
  code: string;
  busy: boolean;
  scanPaused: boolean;
  hasCodeTarget: boolean;
  canSubmitCode: boolean;
  onCodeChange: (value: string) => void;
  onSubmit: () => void;
  onQrScanned: (value: string) => void;
}) {
  const { t } = useAppLanguage();
  if (mode === "code")
    return (
      <Surface className="gap-4">
        <Label variant="title">{t.viewer.pinTitle}</Label>
        <Label tone="muted">
          {hasCodeTarget ? t.viewer.pinDesc : t.viewer.pinNoTargetDesc}
        </Label>
        {hasCodeTarget ? (
          <>
            <OtpPinInput
              code={code}
              onChangeCode={onCodeChange}
              disabled={busy}
              t={t}
            />
            <Action
              onPress={onSubmit}
              disabled={!canSubmitCode}
              busy={busy}
              label={busy ? t.viewer.pairingBusy : t.viewer.btnSubmitPin}
            />
          </>
        ) : (
          <Action
            onPress={() => router.push("/host")}
            label={t.viewer.btnFindHost}
          />
        )}
      </Surface>
    );
  if (cameraState === "loading")
    return (
      <Surface className="items-center gap-3 py-8">
        <ActivityIndicator />
        <Label>{t.viewer.searching}</Label>
      </Surface>
    );
  if (cameraState !== "live")
    return (
      <Surface className="gap-3">
        <Label variant="title">{t.viewer.cameraPermNeeded}</Label>
        <Label tone="muted">{t.viewer.cameraPermDesc}</Label>
        <Action
          onPress={
            cameraState === "blocked" ? onOpenSettings : onRequestPermission
          }
          label={
            cameraState === "blocked"
              ? t.viewer.btnOpenAppSettings
              : t.viewer.btnGrantPerm
          }
        />
      </Surface>
    );
  return (
    <Surface className="gap-3">
      <View className="aspect-square w-full overflow-hidden rounded-lg bg-ink">
        <StyledCameraView
          className="flex-1"
          facing="back"
          barcodeScannerSettings={{ barcodeTypes: ["qr"] }}
          onBarcodeScanned={
            scanPaused
              ? undefined
              : (result) => {
                  const value = result.data?.trim();
                  if (value) onQrScanned(value);
                }
          }
        />
      </View>
      <Label tone="muted">{t.viewer.qrScanHint}</Label>
      <Label variant="caption" tone="muted">
        {t.viewer.qrScanIdleHint}
      </Label>
    </Surface>
  );
}

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
    scanPaused,
    statusMessage,
    notice,
    error,
  } = state;

  const host = scannedHost || resolvePairingHost(routeEndpoint, controlHost());
  // QR/PIN 모두 같은 attempt 수명을 쓴다. 언마운트·새 시도는 저장, 연결,
  // 화면 전환 중 어디에 있든 이전 작업을 취소하고 그 시도의 저장을 되돌린다.
  const pairingWorkflow = useMemo(() => new PairingWorkflow(), []);
  const viewLifetime = useRef({ active: true });
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

  useFocusEffect(
    useCallback(() => {
      const lifetime = { active: true };
      viewLifetime.current = lifetime;
      dispatch({ type: "update", patch: { busy: false, statusMessage: null } });
      return () => {
        lifetime.active = false;
        void pairingWorkflow.cancel();
      };
    }, [pairingWorkflow]),
  );
  const cancelPairing = useCallback(
    async (nextMode?: PairingMode) => {
      const lifetime = viewLifetime.current;
      await pairingWorkflow.cancel();
      if (!lifetime.active || viewLifetime.current !== lifetime) return;
      lastAutoSubmittedCodeRef.current = code;
      dispatch({
        type: "update",
        patch: {
          busy: false,
          statusMessage: null,
          error: null,
          notice: null,
          scanPaused: false,
          ...(nextMode ? { mode: nextMode } : {}),
        },
      });
    },
    [code, pairingWorkflow],
  );

  const handlePairWithCode = useCallback(
    async (codeToPair: string) => {
      if (!viewLifetime.current.active) return;
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
        if (
          !run.attempt.signal.aborted &&
          !(e instanceof Error && e.name === "AbortError")
        ) {
          dispatch({ type: "update", patch: { error: formatErrorMessage(e) } });
        }
      } finally {
        if (pairingWorkflow.finish(run)) {
          dispatch({
            type: "update",
            patch: { busy: false, statusMessage: null },
          });
        }
      }
    },
    [hostEndpoint, pairingWorkflow, scannedPayload, t],
  );

  // Auto-submit code when 6 digits are typed and target is ready
  useEffect(() => {
    if (
      mode === "code" &&
      code.length === 6 &&
      canSubmitCode &&
      !busy &&
      lastAutoSubmittedCodeRef.current !== code
    ) {
      lastAutoSubmittedCodeRef.current = code;
      void handlePairWithCode(code);
    }
  }, [mode, code, canSubmitCode, busy, handlePairWithCode]);

  const handleQrScanned = useCallback(
    async (scannedData: string) => {
      if (!viewLifetime.current.active || scanPaused) return;
      const payload = parseQrPayload(scannedData);
      if (!payload) {
        dispatch({
          type: "update",
          patch: { error: t.viewer.invalidQrError, scanPaused: true },
        });
        return;
      }
      const run = pairingWorkflow.beginQr(payload);
      // CameraView may emit the same visible barcode repeatedly. The approval
      // offer already in flight owns that event identity and continues.
      if (!run) return;
      dispatch({
        type: "update",
        patch: { busy: true, error: null, statusMessage: null },
      });
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
        if (run.attempt.signal.aborted) return;
        if (isPairingRejectedError(e)) {
          dispatch({
            type: "update",
            patch: {
              error: t.viewer.errPairingDeclined,
              scanPaused: true,
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
            patch: {
              error: formatErrorMessage(e),
              scanPaused: true,
              scannedPayload: null,
              scannedHost: "",
            },
          });
        }
      } finally {
        if (pairingWorkflow.finish(run)) {
          dispatch({
            type: "update",
            patch: { busy: false, statusMessage: null },
          });
        }
      }
    },
    [pairingWorkflow, scanPaused, t],
  );

  return {
    ...state,
    dispatch,
    host,
    hasCodeTarget,
    canSubmitCode,
    handlePairWithCode,
    handleQrScanned,
    cancelPairing,
  };
}

export default function Pairing() {
  const { t } = useAppLanguage();

  const params = useLocalSearchParams<{ endpoint?: string }>();
  const routeEndpoint = params.endpoint?.trim() || "";
  const [permission, requestPermission, getPermission] = useCameraPermissions();
  const cameraState = resolveCameraState(permission);
  const {
    mode,
    code,
    busy,
    scanPaused,
    statusMessage,
    notice,
    error,
    dispatch,
    host,
    hasCodeTarget,
    canSubmitCode,
    handlePairWithCode,
    handleQrScanned,
    cancelPairing,
  } = usePairingWorkflow(routeEndpoint, t);

  const [cameraError, setCameraError] = useState<string | null>(null);
  const [cameraFocused, setCameraFocused] = useState(true);
  const cameraFocus = useRef(true);
  const cameraRequest = useRef(0);
  useFocusEffect(
    useCallback(() => {
      cameraFocus.current = true;
      setCameraFocused(true);
      return () => {
        cameraFocus.current = false;
        cameraRequest.current += 1;
        setCameraFocused(false);
      };
    }, []),
  );
  const runCameraAction = useCallback(
    async (action: () => Promise<unknown>) => {
      if (!cameraFocus.current) return;
      const request = ++cameraRequest.current;
      setCameraError(null);
      try {
        await action();
      } catch (failure) {
        if (cameraFocus.current && cameraRequest.current === request)
          setCameraError(formatErrorMessage(failure));
      }
    },
    [],
  );
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
    <SafeArea className="flex-1 bg-canvas" edges={["left", "right", "bottom"]}>
      <ScrollView
        className="flex-1"
        contentContainerClassName="mx-auto w-full max-w-xl gap-4 px-4 pb-8 pt-4"
        keyboardShouldPersistTaps="handled"
      >
        {host ? (
          <Surface variant="inset" className="gap-1">
            <Label variant="caption" tone="muted">
              {t.viewer.pairingTarget}
            </Label>
            <Label variant="code" selectable>
              {host}
            </Label>
          </Surface>
        ) : null}
        <View className="flex-row gap-2">
          <Action
            variant="secondary"
            className="flex-1"
            accessibilityRole="tab"
            accessibilityState={{ selected: mode === "qr" }}
            onPress={() => void cancelPairing("qr")}
            label={t.viewer.tabQr}
          />
          <Action
            variant="secondary"
            className="flex-1"
            accessibilityRole="tab"
            accessibilityState={{ selected: mode === "code" }}
            onPress={() => void cancelPairing("code")}
            label={t.viewer.tabPin}
          />
        </View>
        {notice ? (
          <Notice>
            <Label>{notice}</Label>
          </Notice>
        ) : null}
        {statusMessage ? (
          <Notice>
            <View className="flex-row items-center gap-3">
              <ActivityIndicator />
              <Label className="flex-1" accessibilityLiveRegion="polite">
                {statusMessage}
              </Label>
            </View>
            <Action
              variant="secondary"
              onPress={() => void cancelPairing()}
              label={t.common.cancel}
            />
          </Notice>
        ) : null}
        {error ? (
          <Notice tone="error">
            <Label>{error}</Label>
            {mode === "qr" ? (
              <Action
                variant="secondary"
                onPress={() =>
                  dispatch({
                    type: "update",
                    patch: { error: null, scanPaused: false },
                  })
                }
                label={t.common.retry}
              />
            ) : null}
          </Notice>
        ) : null}
        {cameraError ? (
          <Notice tone="error">
            <Label>{cameraError}</Label>
            <Action
              variant="secondary"
              onPress={refreshPermission}
              label={t.common.retry}
            />
          </Notice>
        ) : null}
        {cameraFocused ? (
          <PairingModeCard
            mode={mode}
            cameraState={cameraState}
            onRequestPermission={() => void runCameraAction(requestPermission)}
            onOpenSettings={openAppSettings}
            code={code}
            busy={busy}
            scanPaused={scanPaused}
            hasCodeTarget={hasCodeTarget}
            canSubmitCode={canSubmitCode}
            onCodeChange={(value) =>
              dispatch({ type: "update", patch: { code: value } })
            }
            onSubmit={() => void handlePairWithCode(code)}
            onQrScanned={handleQrScanned}
          />
        ) : null}
        <Surface variant="inset" className="gap-2">
          <Label>{t.viewer.pairingTipTitle}</Label>
          <Label variant="caption" tone="muted">
            {t.viewer.pairingTipCode}
          </Label>
          <Label variant="caption" tone="muted">
            {t.viewer.pairingTipNetwork}
          </Label>
        </Surface>
      </ScrollView>
    </SafeArea>
  );
}
