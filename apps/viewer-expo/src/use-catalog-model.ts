import { useCallback, useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Alert, NativeModules } from "react-native";
import * as SecureStore from "expo-secure-store";
import { router } from "expo-router";
import { clearToken } from "./pairing";
import {
  reconfigurePreparedStream,
  startPreparedStream,
  type StreamLauncher,
} from "./launch-stream";
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
  allocPort,
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
  type StreamingPriority,
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
  fitProfileToDisplay,
  isHubDisplay,
  requestWithReconnect,
} from "./catalog-helpers";
import {
  streamTargetAfterResize,
} from "./display-resize";
import type { ActiveStream, RestoredStream } from "./catalog-model-types";
import {
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

const launcher = NativeModules.StreamLauncher as StreamLauncher | undefined;

export function useCatalogModel() {
  const [error, setError] = useState<string | null>(null);
  const [launchingIndex, setLaunchingIndex] = useState<number | null>(null);
  const [resizingSession, setResizingSession] = useState<number | null>(null);
  const [windowRatio, setWindowRatio] =
    useState<WindowAspectRatioPresetId | null>(null);
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

  useEffect(() => {
    if (catalogQuery.error && isUnauthorizedError(catalogQuery.error)) {
      void (async () => {
        // 연결 해제 전 모듈 게터에서 대상 주소를 꺼려 effect 의존성 없이도
        // 항상 최신 엔드포인트가 페어링 화면으로 전달된다.
        const endpoint = controlHost();
        await clearToken();
        disconnectHost();
        Alert.alert(
          "연결 승인이 필요해요",
          "컴퓨터의 연결 승인이 만료되었거나 삭제되었습니다. 다시 승인해 주세요.",
        );
        router.replace({
          pathname: "/pairing",
          params: { endpoint },
        });
      })();
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
    return fitProfileToDisplay(display, profile);
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
        throw new Error("화면을 다시 연결할 기능을 시작할 수 없습니다");
      }
      const refreshed = await refetchCatalog();
      const currentCatalog = refreshed.data ?? catalogQuery.data;
      if (!currentCatalog || currentCatalog.captureBackends.length === 0) {
        throw new Error("현재 컴퓨터의 화면 공유 backend를 조회하지 못했습니다");
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
    [catalogQuery.data, host, preferences.showFps, preferences.localCursor, refetchCatalog],
  );

  const reconfigureActiveStream = useCallback(
    async (
      active: ActiveStream,
      target: AdaptiveTarget,
      qualityState: AdaptiveQualityState,
    ): Promise<RestoredStream> => {
      if (!launcher) {
        throw new Error("화면 해상도를 다시 연결할 기능을 시작할 수 없습니다");
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
        advertisedEncoderExperiments: catalogQuery.data?.encoderExperiments,
      });
      return {
        ...reconfigured,
        captureBackend: active.captureBackend,
      };
    },
    [catalogQuery.data, mediaHost],
  );

  const { addStream, applyUdpStability, patchStream, removeStream, streams, syncAdaptiveTarget, updateLocalCursor } =
    useStreamController(setError, restoreActiveStream, reconfigureActiveStream);
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
      ).catch(() => setError("열린 화면의 커서 설정을 바꾸지 못했습니다."));
    }
  }, [setError, streams, updateLocalCursor]);

  const handleSelectStreamingPriority = useCallback(
    (priority: StreamingPriority) => {
      setPreferences((current) => ({ ...current, streamingPriority: priority }));
    },
    [],
  );

  const handleSelectEncoderExperiment = useCallback(
    (id: EncoderExperimentId) => {
      setEncoderExperiment(id);
    },
    [],
  );

  /**
   * XR 창 비율 프리셋 선택. 네이티브 setWindowAspectRatio가 활성
   * StreamActivity에 비율을 전달하고, 컴퓨터 화면 해상도는 그대로 둔다.
   * XR이 아닌 기기에서는 네이티브 호출이 실패하므로 조용히 무시하고 선택을
   * 되돌린다 — 카드는 어떤 기기에서도 비율 행을 노출한다.
   */
  const handleSelectWindowAspectRatio = useCallback(
    (presetId: WindowAspectRatioPresetId) => {
      const preset = WINDOW_ASPECT_RATIO_PRESETS.find((c) => c.id === presetId);
      if (!preset) return;
      const active = streams[0];
      if (!launcher?.setWindowAspectRatio || !active) {
        setWindowRatio(presetId);
        return;
      }
      const previous = windowRatio;
      setWindowRatio(presetId);
      launcher
        .setWindowAspectRatio(`src-${active.port}`, preset.ratio)
        .catch(() => setWindowRatio(previous));
    },
    [streams, windowRatio],
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
        setError("컴퓨터와의 연결이 끊어졌습니다. 다시 연결해 주세요.");
        return;
      }
      if (!launcher) {
        setError("화면을 여는 기능을 시작할 수 없습니다. 앱을 다시 실행해 주세요.");
        return;
      }
      setLaunchingIndex(display.index);
      setError(null);
      try {
        const port = allocPort();
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
          preferences.streamingPriority,
          maximumTarget,
        );
        const { width, height, fps } = initialTarget;
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
          qualityState:
            started.qualityState ??
            (acceptedTarget.width === sourceTarget.width &&
              acceptedTarget.height === sourceTarget.height)
              ? "native"
              : "fallback",
          captureBackend: effectiveCaptureBackend,
          contentMode: displayProfile.contentMode,
          encoderExperiment: started.encoderExperiment,
          udpStability: started.udpStability,
          showFps: preferences.showFps,
          localCursor: preferences.localCursor,
          viewerIps: started.viewerIps,
          mediaTransport: started.mediaTransport,
          startedAt: Date.now(),
        });
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
      preferences.streamingPriority,
      selectedProfile,
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
        setError(`화면 해상도 전환에 실패했습니다: ${formatErrorMessage(cause)}`);
        return false;
      } finally {
        setResizingSession(null);
      }
    },
    [reconfigureActiveStream, replaceStreamState, syncAdaptiveTarget],
  );

  const visibleError = error
    ? error
    : catalogQuery.error
      ? catalogErrorMessage(catalogQuery.error)
      : null;

  return {
    displays,
    effectiveNextEncoderExperiment,
    effectiveUdpStability,
    handleApplyUdpStability,
    handleRefresh,
    handleResizeSession,
    handleSelectEncoderExperiment,
    handleSelectProfile,
    handleSelectStreamingPriority,
    handleSelectUdpStability,
    handleSelectWindowAspectRatio,
    windowRatio,
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
    profileId: preferences.profileId,
    streamingPriority: preferences.streamingPriority,
    showFps: preferences.showFps,
    localCursor: preferences.localCursor,
    resizingSession,
  };
}
