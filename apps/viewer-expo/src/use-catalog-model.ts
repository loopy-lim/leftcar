import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Alert, NativeModules } from "react-native";
import * as SecureStore from "expo-secure-store";
import { router } from "expo-router";
import { LocalizedError } from "./localized-error";
import { currentTranslation } from "./language-store";
import { interpolate } from "@leftcar/ui-tokens";
import {
  isStreamPrepareError,
  reconfigurePreparedStream,
  startPreparedStream,
  type StreamLauncher,
} from "./launch-stream";
import { handleUnauthorized } from "./connect-flow";
import {
  formatErrorMessage,
  isUnauthorizedError,
  preferredCaptureBackend,
  type CatalogView,
  type DisplayInfo,
} from "./control";
import {
  WINDOW_ASPECT_RATIO_PRESETS,
  type WindowAspectRatioPresetId,
} from "./window-aspect-ratio";
import {
  allocPorts,
  controlClient,
  controlHost,
  disconnectHost,
  reconnectHost,
} from "./session";
import {
  STREAM_PROFILES,
  is4KResolution,
} from "./stream-profile";
import {
  resolveInitialStreamTarget,
  streamingPriorityFromProfileId,
} from "./streaming-policy";
import {
  availableEncoderExperimentsForStreams,
  resolveEncoderExperimentForStream,
  type EncoderExperimentId,
} from "./encoder-experiment";
import {
  availableUdpStabilityOptions,
  resolveUdpStabilitySelection,
  type UdpStabilitySelection,
} from "./udp-stability";
import {
  catalogDisplayHost,
  catalogErrorMessage,
  isHubDisplay,
  requestWithReconnect,
} from "./catalog-helpers";
import { resolveStreamResolution } from "./stream-resolution";
import {
  streamTargetAfterResize,
} from "./display-resize";
import {
  admissionStreamsFrom,
  downgradedStreamTarget,
  planAdmission,
  requestedDecoderShape,
} from "./decoder-budget";
import type { ActiveStream, RestoredStream } from "./catalog-model-types";
import {
  deriveQualityState,
  fallbackTargetFor,
  type AdaptiveQualityState,
  type AdaptiveTarget,
} from "./adaptive-resolution";
import { useStreamController } from "./use-stream-controller";
import {
  DEFAULT_VIEWER_PREFERENCES,
  readViewerPreferences,
  resolveStreamMaximum,
  resolveViewerProfileId,
  writeViewerPreferences,
  type ViewerProfileSelection,
  type ViewerPreferences,
} from "./viewer-preferences";
import {
  deviceClipboardIo,
  loadClipboardShare,
  saveClipboardShare,
  startClipboardSync,
  type ClipboardSyncLoop,
} from "./clipboard-sync";

const launcher = NativeModules.StreamLauncher as StreamLauncher | undefined;

export function useCatalogModel() {
  // 오류 문구는 발생 시점 언어를 따른다 — 훅 t를 넣으면 언어 전환마다
  // 장기 콜백 신원이 흔들리므로 모듈 저장소에서 직접 읽는다.
  const [error, setError] = useState<string | null>(null);
  const [launchingIndex, setLaunchingIndex] = useState<number | null>(null);
  const [resizingSession, setResizingSession] = useState<number | null>(null);
  // 소스 전환(R7 cheap display switch) 중인 세션 — 창을 재열지 않고 같은
  // reconfigure 경로로 다른 디스플레이로 옮길 때 진행 표시에 쓴다.
  const [switchingSession, setSwitchingSession] = useState<number | null>(null);
  // 스트림 세션별 XR 창 비율 선택 — 멀티 스트림에서 카드가 각자 활성
  // 상태를 표시할 수 있게 한다.
  const [windowRatios, setWindowRatios] = useState<
    Record<number, WindowAspectRatioPresetId>
  >({});
  // null = 프로브 불가(구버전 네이티브) — 기존처럼 비율 행을 보여 준다.
  const [aspectSupported, setAspectSupported] = useState<boolean | null>(null);

  useEffect(() => {
    if (!launcher?.isXrWindowRatioSupported) {
      setAspectSupported(null);
      return;
    }
    let active = true;
    launcher
      .isXrWindowRatioSupported()
      .then((supported) => {
        if (active) setAspectSupported(Boolean(supported));
      })
      .catch(() => {
        if (active) setAspectSupported(false);
      });
    return () => {
      active = false;
    };
  }, []);
  const [preferences, setPreferences] = useState<ViewerPreferences>(
    DEFAULT_VIEWER_PREFERENCES,
  );
  const [preferencesLoaded, setPreferencesLoaded] = useState(false);
  const [encoderExperiment, setEncoderExperiment] =
    useState<EncoderExperimentId>("auto");
  const [udpStability, setUdpStability] = useState<UdpStabilitySelection>({
    profile: "auto",
  });
  const [udpSettingsDirty, setUdpSettingsDirty] = useState(false);
  const [udpReconnecting, setUdpReconnecting] = useState(false);
  const host = controlHost();
  // 시작 크기 우선순위는 별도 다이얼 없이 선택한 품질 프로필에서 파생한다
  // (기존 streamingPriority 저장값은 마이그레이션 호환용으로만 남는다).
  const streamingPriority = streamingPriorityFromProfileId(preferences.profileId);
  const catalogQuery = useQuery({
    queryKey: ["catalog", host],
    queryFn: () => requestWithReconnect<CatalogView>("getCatalog"),
    staleTime: 30_000,
  });
  const { refetch: refetchCatalog } = catalogQuery;

  useEffect(() => {
    let active = true;
    void readViewerPreferences(SecureStore)
      .then((stored) => {
        if (active) setPreferences(stored);
      })
      .finally(() => {
        if (active) setPreferencesLoaded(true);
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!preferencesLoaded) return;
    void writeViewerPreferences(SecureStore, preferences).catch(() => undefined);
  }, [preferences, preferencesLoaded]);

  // 클립보드 공유 토글(U5): `leftcar.clipboardShare`(기본 꺼짐)에 저장하고,
  // 켜져 있으면 제어 세션과 함께 폴링 루프를 돌린다. 호스트 게이트도 기본
  // 꺼짐이므로 이중 잠금이다(docs/07 §20).
  const clipboardSyncRef = useRef<ClipboardSyncLoop | null>(null);
  const [clipboardShare, setClipboardShareState] = useState(false);
  // 디코더 어드미션(M4/R8)이 쓰는 라이브 스트림 스냅숏. reconfigure 콜백은
  // useStreamController보다 먼저 정의되므로 ref로 최신 목록을 운반한다.
  const liveStreamsRef = useRef<ActiveStream[]>([]);

  useEffect(() => {
    let active = true;
    void loadClipboardShare(SecureStore).then((enabled) => {
      if (active) setClipboardShareState(enabled);
    });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!clipboardSyncRef.current) {
      clipboardSyncRef.current = startClipboardSync({
        getClient: () => controlClient(),
        ...deviceClipboardIo,
      });
    }
    clipboardSyncRef.current.setEnabled(clipboardShare);
    return () => {
      clipboardSyncRef.current?.stop();
      clipboardSyncRef.current = null;
    };
  }, [clipboardShare]);

  const handleToggleClipboardShare = useCallback((enabled: boolean) => {
    setClipboardShareState(enabled);
    void saveClipboardShare(SecureStore, enabled).catch(() => undefined);
  }, []);

  useEffect(() => {
    if (catalogQuery.error && isUnauthorizedError(catalogQuery.error)) {
      // 연결 해제 전 모듈 게터에서 대상 주소를 꺼려 effect 의존성 없이도
      // 항상 최신 엔드포인트가 페어링 화면으로 전달된다.
      void handleUnauthorized({
        navigate: { endpoint: controlHost(), replace: true },
      });
    }
  }, [catalogQuery.error]);

  const displays = (catalogQuery.data?.displays ?? []).filter(
    (display) => !isHubDisplay(display.name),
  );
  const loading = catalogQuery.isLoading;
  const refreshing = catalogQuery.isRefetching;
  const effectiveCaptureBackend = preferredCaptureBackend(catalogQuery.data, "");
  const mediaHost =
    catalogQuery.data?.mediaHost?.trim() || catalogDisplayHost(host);
  const selectedProfile =
    STREAM_PROFILES.find((profile) => profile.id === preferences.profileId) ??
    STREAM_PROFILES.find((profile) => profile.id === "balanced") ??
    STREAM_PROFILES[0];

  const advertisedEncoderExperiments = catalogQuery.data?.encoderExperiments;
  const fittedDisplayTargets = displays.map((display) => {
    const profileId = resolveViewerProfileId(preferences.profileId, display);
    const profile = STREAM_PROFILES.find((candidate) => candidate.id === profileId)
      ?? selectedProfile;
    return resolveStreamResolution(display, profile);
  });
  const hasActual4KTarget = fittedDisplayTargets.some((target) =>
    is4KResolution(target.width, target.height),
  );
  const selectedEncoderExperiments = availableEncoderExperimentsForStreams(
    advertisedEncoderExperiments,
    fittedDisplayTargets,
  );
  const effectiveNextEncoderExperiment = resolveEncoderExperimentForStream(
    encoderExperiment,
    advertisedEncoderExperiments,
    hasActual4KTarget ? 3840 : 0,
    hasActual4KTarget ? 2160 : 0,
  );
  const advertisedUdpStabilityCapabilities =
    catalogQuery.data?.udpStabilityCapabilities;
  const udpStabilityOptions = useMemo(
    () => availableUdpStabilityOptions(advertisedUdpStabilityCapabilities),
    [advertisedUdpStabilityCapabilities],
  );
  const effectiveUdpStability = useMemo<UdpStabilitySelection>(
    () =>
      resolveUdpStabilitySelection(
        udpStability,
        advertisedUdpStabilityCapabilities,
      ) ?? {
        profile: udpStabilityOptions?.profiles[0] ?? "auto",
      },
    [advertisedUdpStabilityCapabilities, udpStability, udpStabilityOptions],
  );

  useEffect(() => {
    setEncoderExperiment((current) =>
      resolveEncoderExperimentForStream(
        current,
        advertisedEncoderExperiments,
        hasActual4KTarget ? 3840 : 0,
        hasActual4KTarget ? 2160 : 0,
      ),
    );
  }, [advertisedEncoderExperiments, hasActual4KTarget]);

  const restoreActiveStream = useCallback(
    async (active: ActiveStream): Promise<RestoredStream> => {
      if (!launcher) {
        throw new LocalizedError("errRestartLauncher");
      }
      const refreshed = await refetchCatalog();
      const currentCatalog = refreshed.data ?? catalogQuery.data;
      if (!currentCatalog || currentCatalog.captureBackends.length === 0) {
        throw new LocalizedError("errBackendQuery");
      }
      const captureBackend = preferredCaptureBackend(
        currentCatalog,
        active.captureBackend,
      );
      // Re-resolve from the just-fetched catalog: the media host can differ
      // from the cached `mediaHost` computed at render time.
      const refreshedMediaHost = currentCatalog.mediaHost?.trim()
        ? catalogDisplayHost(currentCatalog.mediaHost.trim())
        : catalogDisplayHost(host);
      await requestWithReconnect("stopStream", { session: active.session }).catch(
        () => undefined,
      );
      const control = controlClient() ?? (await reconnectHost());
      const restarted = await startPreparedStream({
        control,
        request: requestWithReconnect,
        launcher,
        host: refreshedMediaHost,
        advertisedEncoderExperiments: currentCatalog.encoderExperiments,
        advertisedUdpStabilityCapabilities:
          currentCatalog.udpStabilityCapabilities,
        args: {
          sourceIndex: active.sourceIndex,
          viewerPort: active.port,
          width: active.activeTarget.width,
          height: active.activeTarget.height,
          fps: active.activeTarget.fps,
          captureBackend,
          mediaTransport: "auto",
          encoderExperiment: active.encoderExperiment,
          contentMode: active.contentMode,
          udpStability: active.udpStability,
          showFps: active.showFps ?? preferences.showFps,
          localCursor: active.localCursor ?? preferences.localCursor,
          localAudio: active.localAudio ?? preferences.localAudio,
        },
      });
      return {
        ...restarted,
        captureBackend,
        width: restarted.width ?? active.activeTarget.width,
        height: restarted.height ?? active.activeTarget.height,
        fps: restarted.fps ?? active.activeTarget.fps,
        qualityState: active.qualityState,
      };
    },
    [
      catalogQuery.data,
      host,
      preferences.showFps,
      preferences.localCursor,
      preferences.localAudio,
      refetchCatalog,
    ],
  );

  const reconfigureActiveStream = useCallback(
    async (
      active: ActiveStream,
      target: AdaptiveTarget,
      qualityState: AdaptiveQualityState,
      // 소스 전환(R7): 지정하면 reconfigure 요청에 sourceIndex를 실어 같은
      // 세션 창을 다른 호스트 디스플레이로 옮긴다. 능력 플래그가 없는
      // 구버전 호스트에는 절대 보내지 않는다(launch-stream에서 게이트).
      source?: { index: number },
    ): Promise<RestoredStream> => {
      if (!launcher) {
        throw new LocalizedError("errResizeLauncher");
      }
      const control = controlClient() ?? (await reconnectHost());
      const reconfigured = await reconfigurePreparedStream({
        control,
        launcher,
        host: mediaHost,
        active,
        target,
        qualityState,
        // capability가 있을 때만 인코더 모드 전환(Auto↔Split)을 요청한다.
        reconfigureEncoderExperiment:
          catalogQuery.data?.reconfigureEncoderExperiment === true,
        ...(source ? { sourceIndex: source.index } : {}),
        reconfigureSource: catalogQuery.data?.reconfigureSource === true,
        advertisedEncoderExperiments: catalogQuery.data?.encoderExperiments,
        // split 승격도 디코더 예산(M4/R8) 안에서만 — 자기 슬롯을 반납한
        // 뒤 두 슬롯이 들어갈 때만 승격한다.
        decoderBudget: {
          currentStreams: admissionStreamsFrom(liveStreamsRef.current),
        },
      });
      return {
        ...reconfigured,
        captureBackend: active.captureBackend,
      };
    },
    [catalogQuery.data, mediaHost],
  );

  const { addStream, applyUdpStability, patchStream, removeStream, streamError, streams, syncAdaptiveTarget, updateLocalCursor, updateLocalAudio } =
    useStreamController(restoreActiveStream, reconfigureActiveStream);
  useEffect(() => {
    liveStreamsRef.current = streams;
  }, [streams]);
  const replaceStreamState = useCallback(
    (next: ActiveStream) => {
      patchStream(next.session, () => next);
    },
    [patchStream],
  );

  const handleRefresh = useCallback(() => {
    setError(null);
    void refetchCatalog();
  }, [refetchCatalog]);

  const handleSelectProfile = useCallback((id: ViewerProfileSelection) => {
    setPreferences((current) => ({ ...current, profileId: id }));
  }, []);

  const handleToggleFps = useCallback((showFps: boolean) => {
    setPreferences((current) => ({ ...current, showFps }));
  }, []);

  const handleToggleCursor = useCallback((localCursor: boolean) => {
    setPreferences((current) => ({ ...current, localCursor }));
    updateLocalCursor(localCursor);
    if (launcher?.setCursorStream) {
      void Promise.all(
        streams.map((stream) => launcher.setCursorStream?.(`src-${stream.port}`, localCursor)),
      ).catch(() => setError(currentTranslation().viewer.errCursorUpdate));
    }
  }, [setError, streams, updateLocalCursor]);

  const handleToggleAudio = useCallback((localAudio: boolean) => {
    setPreferences((current) => ({ ...current, localAudio }));
    updateLocalAudio(localAudio);
    if (launcher?.setAudioStream) {
      void Promise.all(
        streams.map((stream) => launcher.setAudioStream?.(`src-${stream.port}`, localAudio)),
      ).catch(() => setError(currentTranslation().viewer.errAudioUpdate));
    }
  }, [setError, streams, updateLocalAudio]);

  const handleSelectEncoderExperiment = useCallback(
    (id: EncoderExperimentId) => {
      setEncoderExperiment(id);
    },
    [],
  );

  /**
   * XR 창 비율 프리셋 선택. 네이티브 setWindowAspectRatio가 활성
   * StreamActivity에 비율을 전달하고, 컴퓨터 화면 해상도는 그대로 둔다.
   * 선택은 세션별로 기록한다. 비-XR 기기에서 네이티브 호출이 실패하면
   * 조용히 이전 선택으로 되돌리고, 프로브가 지원 불가를 알리면 카드가
   * 비율 행 자체를 숨긴다 — 눌렸다가 튕기는 버튼을 남기지 않는다.
   */
  const handleSelectWindowAspectRatio = useCallback(
    (presetId: WindowAspectRatioPresetId, stream: ActiveStream) => {
      const preset = WINDOW_ASPECT_RATIO_PRESETS.find((c) => c.id === presetId);
      if (!preset) return;
      if (!launcher?.setWindowAspectRatio) {
        setWindowRatios((current) => ({ ...current, [stream.session]: presetId }));
        return;
      }
      const previous = windowRatios[stream.session] ?? null;
      setWindowRatios((current) => ({ ...current, [stream.session]: presetId }));
      launcher
        .setWindowAspectRatio(`src-${stream.port}`, preset.ratio)
        .catch(() =>
          setWindowRatios((current) => {
            const next = { ...current };
            if (previous === null) delete next[stream.session];
            else next[stream.session] = previous;
            return next;
          }),
        );
    },
    [windowRatios],
  );

  const handleSelectUdpStability = useCallback(
    (selection: UdpStabilitySelection) => {
      setUdpStability(selection);
      if (streams.length > 0) setUdpSettingsDirty(true);
    },
    [streams.length],
  );

  const handleApplyUdpStability = useCallback(() => {
    setUdpReconnecting(true);
    void applyUdpStability(effectiveUdpStability)
      .then(() => setUdpSettingsDirty(false))
      .finally(() => setUdpReconnecting(false));
  }, [applyUdpStability, effectiveUdpStability]);

  const openDisplay = useCallback(
    async (display: DisplayInfo) => {
      const client = controlClient();
      if (!client) {
        setError(currentTranslation().viewer.connectionLostError);
        return;
      }
      if (!launcher) {
        setError(currentTranslation().viewer.launchFeatureError);
        return;
      }
      setLaunchingIndex(display.index);
      setError(null);
      // Each attempt reserves two consecutive ports (see allocPorts): the
      // second port belongs to a splitVertical stream's right tile, so a
      // window must never hand base+1 to the next openDisplay.
      const launch = async (port: number) => {
        const profileId = resolveViewerProfileId(preferences.profileId, display);
        const displayProfile =
          STREAM_PROFILES.find((profile) => profile.id === profileId) ??
          selectedProfile;
        // 소스/사용자 최대(적응 정책의 업시프트 목표)와 새 스트림의 시작
        // 목표(responsive 1440 / clarity 최대)를 분리한다. AUTO는 실제
        // clarity 스트리밍 목표(resolveStreamingTarget)를 최대로 쓰고,
        // 수동 프로필은 기존 프로필 상한을 그대로 유지한다. 논리 데스크톱
        // 크기는 자동 품질 전환으로 바꾸지 않는다.
        const maximumTarget = resolveStreamMaximum(display, preferences.profileId);
        const initialTarget = resolveInitialStreamTarget(
          display,
          streamingPriority,
          maximumTarget,
        );
        const requestedTarget = {
          width: initialTarget.width,
          height: initialTarget.height,
          fps: initialTarget.fps,
        };
        // 디코더 어드미션(M4/R8): 네이티브 prepare 전에 라이브 창 점유
        // 인스턴스를 합산해 요청 형태(split은 2, 일반은 1)를 검사한다.
        // 스트림 목록이 단일 출처(ledger)라 종료된 스트림의 슬롯은 자동으로
        // 반납된다. 두 openDisplay가 서로의 addStream 등록 전에 검사를
        // 통과하는 작은 경쟁은 v1에서 허용한다.
        let launchTarget = requestedTarget;
        const admission = planAdmission(
          admissionStreamsFrom(streams),
          {
            target: requestedTarget,
            split: requestedDecoderShape({
              encoderExperiment,
              width: requestedTarget.width,
              height: requestedTarget.height,
              advertisedEncoderExperiments,
            }),
          },
        );
        if (!admission.allowed) {
          throw new LocalizedError("errDecoderCapacity");
        }
        if (admission.action === "downgradeResolution") {
          // split 거절 → 가장 큰 단일 해상도로(4K 요청이므로 사다리에 다음
          // 단계가 항상 있고, 4K 미만 목표는 prepare 경로에서 분할로
          // 승격되지 않는다). 일반 창 만석은 이제 block이므로 이 분기에
          // 도달하는 강등은 split 거절뿐이다.
          const lowered = downgradedStreamTarget(requestedTarget);
          if (lowered) launchTarget = lowered;
        }
        const { width, height, fps } = launchTarget;
        const sourceTarget = {
          width: maximumTarget.width,
          height: maximumTarget.height,
          fps: maximumTarget.fps,
        };
        const started = await startPreparedStream({
          control: client,
          request: requestWithReconnect,
          launcher,
          host: mediaHost,
          advertisedEncoderExperiments,
          advertisedUdpStabilityCapabilities:
            catalogQuery.data?.udpStabilityCapabilities,
          args: {
            sourceIndex: display.index,
            viewerPort: port,
            width,
            height,
            fps,
            captureBackend: effectiveCaptureBackend,
            mediaTransport: "auto",
            encoderExperiment,
            displayName: display.name,
            contentMode: displayProfile.contentMode,
            udpStability: effectiveUdpStability,
            showFps: preferences.showFps,
            localCursor: preferences.localCursor,
            localAudio: preferences.localAudio,
          },
        });
        const acceptedTarget = {
          width: started.width ?? width,
          height: started.height ?? height,
          fps: started.fps ?? fps,
        };
        addStream({
          port,
          session: started.session,
          sourceIndex: display.index,
          sourceName: display.name,
          width: acceptedTarget.width,
          height: acceptedTarget.height,
          fps: acceptedTarget.fps,
          sourceTarget,
          activeTarget: acceptedTarget,
          fallbackTarget: fallbackTargetFor(sourceTarget),
          qualityState: deriveQualityState(
            acceptedTarget,
            sourceTarget,
            started.qualityState,
          ),
          captureBackend: effectiveCaptureBackend,
          contentMode: displayProfile.contentMode,
          encoderExperiment: started.encoderExperiment,
          udpStability: started.udpStability,
          showFps: preferences.showFps,
          localCursor: preferences.localCursor,
          localAudio: preferences.localAudio,
          viewerIps: started.viewerIps,
          mediaTransport: started.mediaTransport,
          mediaKey: started.mediaKey,
          startedAt: Date.now(),
        });
      };
      try {
        try {
          await launch(allocPorts(2));
        } catch (cause) {
          if (!isStreamPrepareError(cause)) throw cause;
          // A viewer-side bind failure surfaces before the Host startStream
          // request exists, so no host session needs cleanup here. The
          // failed port pair is already abandoned by the allocator, so one
          // fresh allocation is guaranteed to skip past the conflict (e.g.
          // a stale split neighbor still holding base+1).
          await launch(allocPorts(2));
        }
      } catch (cause) {
        setError(formatErrorMessage(cause));
      } finally {
        setLaunchingIndex(null);
      }
    },
    [
      addStream,
      advertisedEncoderExperiments,
      catalogQuery.data?.udpStabilityCapabilities,
      effectiveCaptureBackend,
      encoderExperiment,
      effectiveUdpStability,
      mediaHost,
      preferences.profileId,
      preferences.showFps,
      preferences.localCursor,
      preferences.localAudio,
      selectedProfile,
      streams,
      streamingPriority,
    ],
  );

  const stopStream = useCallback(
    async (active: ActiveStream) => {
      try {
        await requestWithReconnect("stopStream", { session: active.session });
      } catch {
        // best effort
      }
      removeStream(active.session);
    },
    [removeStream],
  );

  /**
   * Explicit resolution change: reconfigure the session through the existing
   * reconfigure path and re-seed the adaptive state to the accepted target.
   */
  const handleResizeSession = useCallback(
    async (
      active: ActiveStream,
      width: number,
      height: number,
      fps: number,
    ): Promise<boolean> => {
      setResizingSession(active.session);
      try {
        const target = { width, height, fps };
        const accepted = await reconfigureActiveStream(active, target, "native");
        const next = streamTargetAfterResize(active, target, accepted);
        // 명시적 크기 변경 후 적응 상태를 새 목표로 다시 심는다 — 매 샘플마다가
        // 아니라 실제 변경 때만 호출되므로 히스테리시스가 보존된다.
        syncAdaptiveTarget(active.session, next.sourceTarget, next.activeTarget);
        replaceStreamState(next);
        return true;
      } catch (cause) {
        setError(interpolate(currentTranslation().viewer.errResizeFailed, { detail: formatErrorMessage(cause) }));
        return false;
      } finally {
        setResizingSession(null);
      }
    },
    [reconfigureActiveStream, replaceStreamState, syncAdaptiveTarget],
  );

  /**
   * Cheap display switch (R7): move an active stream window to a different
   * host display on the SAME host without tearing down the window. The
   * prepared receiver is reused (bound before the reconfigure, so it captures
   * the replacement backend's LCH1 challenge), the media port never changes,
   * and the window state is re-seeded against the new display exactly like a
   * fresh open would be. The host only restarts its capture backend under the
   * same session id.
   */
  const handleSwitchSessionSource = useCallback(
    async (active: ActiveStream, display: DisplayInfo): Promise<boolean> => {
      if (!launcher) {
        setError(currentTranslation().viewer.launchFeatureError);
        return false;
      }
      if (display.index === active.sourceIndex) return true;
      setSwitchingSession(active.session);
      try {
        // 새 디스플레이를 새 창을 여는 것과 같은 규칙으로 맞춘다: 스트리밍
        // 목표는 디스플레이+프로필에서, 적응 상태는 새 디스플레이의 최대
        // 목표에 다시 심는다(openDisplay의 시딩과 동일).
        const profileId = resolveViewerProfileId(preferences.profileId, display);
        const maximumTarget = resolveStreamMaximum(display, preferences.profileId);
        const initialTarget = resolveInitialStreamTarget(
          display,
          streamingPriority,
          maximumTarget,
        );
        const target = {
          width: initialTarget.width,
          height: initialTarget.height,
          fps: initialTarget.fps,
        };
        const reconfigured = await reconfigureActiveStream(
          active,
          target,
          "native",
          { index: display.index },
        );
        const sourceTarget = {
          width: maximumTarget.width,
          height: maximumTarget.height,
          fps: maximumTarget.fps,
        };
        const acceptedTarget = {
          width: reconfigured.width ?? target.width,
          height: reconfigured.height ?? target.height,
          fps: reconfigured.fps ?? target.fps,
        };
        replaceStreamState({
          ...streamTargetAfterResize(active, target, reconfigured),
          sourceIndex: reconfigured.sourceIndex ?? display.index,
          sourceName: reconfigured.sourceName ?? display.name,
          width: acceptedTarget.width,
          height: acceptedTarget.height,
          fps: acceptedTarget.fps,
          sourceTarget,
          activeTarget: acceptedTarget,
          fallbackTarget: fallbackTargetFor(sourceTarget),
          qualityState: deriveQualityState(
            acceptedTarget,
            sourceTarget,
            reconfigured.qualityState,
          ),
        });
        syncAdaptiveTarget(active.session, sourceTarget, acceptedTarget);
        return true;
      } catch (cause) {
        setError(formatErrorMessage(cause));
        return false;
      } finally {
        setSwitchingSession(null);
      }
    },
    [
      preferences.profileId,
      reconfigureActiveStream,
      replaceStreamState,
      streamingPriority,
      syncAdaptiveTarget,
    ],
  );

  const visibleError =
    error ||
    streamError ||
    (catalogQuery.error ? catalogErrorMessage(catalogQuery.error) : null);

  return {
    displays,
    effectiveNextEncoderExperiment,
    effectiveUdpStability,
    handleApplyUdpStability,
    handleRefresh,
    handleResizeSession,
    handleSelectEncoderExperiment,
    handleSelectProfile,
    handleSelectUdpStability,
    handleSelectWindowAspectRatio,
    handleSwitchSessionSource,
    windowRatios,
    aspectSupported,
    host,
    launchingIndex,
    loading,
    openDisplay,
    refreshing,
    selectedEncoderExperiments,
    selectedProfile,
    stopStream,
    streams,
    udpReconnecting,
    udpSettingsDirty,
    udpStabilityOptions,
    visibleError,
    handleToggleFps,
    handleToggleCursor,
    handleToggleAudio,
    handleToggleClipboardShare,
    clipboardShare,
    profileId: preferences.profileId,
    streamingPriority,
    showFps: preferences.showFps,
    localCursor: preferences.localCursor,
    localAudio: preferences.localAudio,
    resizingSession,
    switchingSession,
  };
}
