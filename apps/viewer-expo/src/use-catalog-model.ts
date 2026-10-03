import { useExtensionDisplay } from "./use-extension-display";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { NativeModules } from "react-native";
import { router } from "expo-router";
import { LocalizedError } from "./localized-error";
import { currentTranslation } from "./language-store";
import { interpolate } from "@leftcar/ui-tokens";
import {
  isStreamPrepareError,
  readViewerDisplayMetrics,
  resolveReconfigureExperiment,
  reconfigurePreparedStream,
  startPreparedStream,
  type StreamControlRequest,
  type StreamLauncher,
} from "./launch-stream";
import { isAbsentControlSession } from "./control-error";
import { handleUnauthorized } from "./connect-flow";
import {
  formatErrorMessage,
  isUnauthorizedError,
  preferredCaptureBackend,
  type CatalogView,
  type DisplayInfo,
} from "./control";
import {
  allocPorts,
  controlClient,
  controlHost,
  controlTarget,
  disconnectHost,
  reconnectHost,
  requestContextForError,
  captureRequestContext,
  isRequestContextCurrent,
} from "./session";
import { STREAM_PROFILES, is4KResolution } from "./stream-profile";
import {
  capTargetToPanelShortSide,
  panelShortSideCap,
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
  catalogMediaHost,
  catalogErrorMessage,
  isExternalRouteAddress,
  isHubDisplay,
  requestWithReconnect,
  requestForCurrentSelection,
} from "./catalog-helpers";
import { mergeAdvertisedRoutes } from "./recent-hosts";
import { resolveStreamResolution } from "./stream-resolution";
import { streamTargetAfterResize } from "./display-resize";
import {
  deviceDecoderReservations,
  requestedDecoderShape,
} from "./decoder-budget";
import {
  ReservedStream,
  retryAbandonedDecoderCleanup,
} from "./reserved-stream";
import { streamSessionStore } from "./stream-session-store";
import type { ActiveStream, RestoredStream } from "./catalog-model-types";
import {
  deriveQualityState,
  fallbackTargetFor,
  type AdaptiveQualityState,
  type AdaptiveTarget,
} from "./adaptive-resolution";
import { useStreamController } from "./use-stream-controller";
import {
  resolveStreamMaximum,
  resolveViewerProfileId,
  type ViewerProfileSelection,
} from "./viewer-preferences";
import { useCatalogPreferences } from "./use-catalog-preferences";
import {
  runStreamLifetimeOperation,
  sameStreamLifetime,
} from "./stream-lifetime-operation";
import { NativeSettingsController, type NativeSettingsKey } from "./native-settings";

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
  useEffect(() => {
    // Until this device-wide probe settles the pool admits one instance only.
    // Keep launch reservation synchronous so unmount/selection cleanup cannot
    // run between a capability await and publication of native ownership.
    if (launcher) void deviceDecoderReservations.refreshCapability(launcher);
  }, []);
  const {
    preferences,
    updateViewerPreferences: setPreferences,
    viewerPreferenceControlsDisabled,
    clipboardPreferenceControlDisabled,
    clipboardShare,
    updateClipboardPreference,
    udpStability,
    updateUdpPreference,
    udpPreferenceControlsDisabled,
    preferencePersistenceIssue,
    preferenceLoading,
    preferenceSaving,
    retryPersistence,
  } = useCatalogPreferences();
  const [encoderExperiment, setEncoderExperiment] =
    useState<EncoderExperimentId>("auto");
  const [udpSettingsDirty, setUdpSettingsDirty] = useState(false);
  const udpPreferenceRevision = useRef(0);
  const host = controlHost();
  const nativeSettings = useMemo(() => new NativeSettingsController(), [host]);
  const nativeSettingsStatus = useSyncExternalStore(
    nativeSettings.subscribe, nativeSettings.getSnapshot, nativeSettings.getSnapshot,
  );
  useEffect(() => {
    nativeSettings.activate();
    return () => nativeSettings.dispose();
  }, [nativeSettings]);
  const nativeSettingsApplying = nativeSettingsStatus.pending.length > 0;
  const udpReconnecting = nativeSettingsStatus.pending.includes("udp");
  const handleRetryNativeSettings = useCallback((key: NativeSettingsKey) => {
    void nativeSettings.retry(key);
  }, [nativeSettings]);
  const audioRequest = useRef(0);
  const audioLifetimes = useRef(
    new Map<string, { stream: ActiveStream; successfulRequest: number }>(),
  );
  const presentationRequest = useRef(0);
  const presentationLifetimes = useRef(
    new Map<
      number,
      {
        stream: ActiveStream;
        successfulRequest: number;
      }
    >(),
  );
  useEffect(
    () => () => {
      audioRequest.current += 1;
      audioLifetimes.current.clear();
      presentationRequest.current += 1;
      presentationLifetimes.current.clear();
    },
    [host],
  );
  // 시작 크기 우선순위는 별도 다이얼 없이 선택한 품질 프로필에서 파생한다
  // (기존 streamingPriority 저장값은 마이그레이션 호환용으로만 남는다).
  const streamingPriority = streamingPriorityFromProfileId(
    preferences.profileId,
  );
  const catalogQuery = useQuery({
    queryKey: ["catalog", host],
    queryFn: () => requestWithReconnect<CatalogView>("getCatalog"),
    staleTime: 30_000,
  });
  const { refetch: refetchCatalog } = catalogQuery;

  // 디코더 어드미션(M4/R8)이 쓰는 라이브 스트림 스냅숏. reconfigure 콜백은
  // useStreamController보다 먼저 정의되므로 ref로 최신 목록을 운반한다.
  const sessionStore = streamSessionStore(host);
  const ownedReservations = sessionStore.pending;
  useEffect(() => () => sessionStore.detachCatalog(), [sessionStore]);

  const handleToggleClipboardShare = useCallback(
    (enabled: boolean) => {
      if (clipboardPreferenceControlDisabled) return;
      updateClipboardPreference(() => enabled);
    },
    [clipboardPreferenceControlDisabled, updateClipboardPreference],
  );

  useEffect(() => {
    if (catalogQuery.error && isUnauthorizedError(catalogQuery.error)) {
      const context = requestContextForError(catalogQuery.error);
      const controller = new AbortController();
      // 연결 해제 전 모듈 게터에서 대상 주소를 꺼려 effect 의존성 없이도
      // 항상 최신 엔드포인트가 페어링 화면으로 전달된다.
      void handleUnauthorized({
        context,
        signal: controller.signal,
        navigate: { endpoint: controlHost(), replace: true },
      }).catch((cause) => {
        if (
          !controller.signal.aborted &&
          context &&
          isRequestContextCurrent(context)
        ) {
          setError(formatErrorMessage(cause));
        }
      });
      return () => controller.abort();
    }
  }, [catalogQuery.error]);

  const displays = (catalogQuery.data?.displays ?? []).filter(
    (display) => !isHubDisplay(display.name),
  );
  const loading = catalogQuery.isLoading;
  const refreshing = catalogQuery.isRefetching;
  const effectiveCaptureBackend = preferredCaptureBackend(
    catalogQuery.data,
    "",
  );
  const mediaHost = catalogMediaHost(
    host,
    catalogQuery.data?.mediaHost,
    captureRequestContext()?.client.remoteAddress,
    catalogQuery.data?.publicMediaEndpoint,
  );
  // 외부 경로(테일넷·공인 주소)면 시작 타깃을 1080p로 낮춘다 — 설정 없는
  // 자동 완화이고, 위로 올라가는 것은 적응 정책의 몫이다.
  const externalMediaRoute = isExternalRouteAddress(mediaHost);

  // 호스트가 광고하는 다른 경로(테일넷·LAN)를 같은 호스트 엔트리에 병합한다 —
  // 집에서 한 번 연결하면 어느 네트워크에서든 접속 후보가 생긴다. 실제 병합은
  // 공용 헬퍼가 담당하고, 여기는 카탈로그 화면 백스톱이다.
  const advertisedRoutes = useMemo(
    () => ({
      tailscaleHost: catalogQuery.data?.tailscaleHost,
      mediaHost: catalogQuery.data?.mediaHost,
    }),
    [catalogQuery.data?.tailscaleHost, catalogQuery.data?.mediaHost],
  );
  useEffect(() => {
    const target = controlTarget();
    if (!target) return;
    void mergeAdvertisedRoutes(target, advertisedRoutes).catch(() => {
      // 별칭 저장 실패는 조용히 넘긴다 — 다음 카탈로그에서 다시 시도한다.
    });
  }, [advertisedRoutes]);
  const selectedProfile =
    STREAM_PROFILES.find((profile) => profile.id === preferences.profileId) ??
    STREAM_PROFILES.find((profile) => profile.id === "balanced") ??
    STREAM_PROFILES[0];

  const advertisedEncoderExperiments = catalogQuery.data?.encoderExperiments;
  const fittedDisplayTargets = displays.map((display) => {
    const profileId = resolveViewerProfileId(preferences.profileId, display);
    const profile =
      STREAM_PROFILES.find((candidate) => candidate.id === profileId) ??
      selectedProfile;
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
      const reservation = active.reservation;
      if (!reservation) throw new Error("Missing decoder reservation");
      // 복구는 카탈로그가 언마운트된 뒤에도 호출된다. 전송 끊김은
      // controlHost()를 비우지만 원래 선택 대상(activeContext)은 유지된다 —
      // 선택 변경이 아니므로 렌더 시점 host가 비었으면 유지된 대상으로
      // 핀한다. 그렇지 않으면 모든 요청이 "Host selection changed"로
      // 거절되어 재연결 자체가 불가능해진다.
      const retained = captureRequestContext();
      const retainedHost = retained
        ? `${retained.target.host}:${retained.target.port}`
        : "";
      const request = requestForCurrentSelection(host || retainedHost);
      const demand = reservation.pool.plan(
        {
          split:
            active.encoderExperiment === "splitVertical" ||
            (active.encoderExperiment === "auto" &&
              is4KResolution(
                active.activeTarget.width,
                active.activeTarget.height,
              )),
          target: active.activeTarget,
        },
        reservation.lease,
      );
      if (!demand) throw new LocalizedError("errDecoderCapacity");
      const restored = await reservation.run(
        demand,
        async (reservedLauncher) => {
          // Recovery outlives the catalog's QueryObserver and React surface.
          const currentCatalog = await request<CatalogView>("getCatalog");
          if (!currentCatalog || currentCatalog.captureBackends.length === 0) {
            throw new LocalizedError("errBackendQuery");
          }
          const captureBackend = preferredCaptureBackend(
            currentCatalog,
            active.captureBackend,
          );
          // Re-resolve from the just-fetched catalog: the media host can differ
          // from the cached `mediaHost` computed at render time.
          const refreshedMediaHost = catalogMediaHost(
            host || retainedHost,
            currentCatalog.mediaHost,
            captureRequestContext()?.client.remoteAddress,
            currentCatalog.publicMediaEndpoint,
          );
          await request("stopStream", { session: active.session }).catch((error: unknown) => {
            // A restarted Host may have already forgotten this session. Every
            // other failure retains uncertainty/authorization/cancellation;
            // starting another session would bypass the recovery fence.
            if (!isAbsentControlSession(error)) throw error;
          });
          const control = controlClient() ?? (await reconnectHost());
          const viewerMetrics = await readViewerDisplayMetrics(launcher);
          const restarted = await startPreparedStream({
            control,
            request,
            launcher: reservedLauncher,
            decoderReservation: demand,
            host: refreshedMediaHost,
            advertisedEncoderExperiments: currentCatalog.encoderExperiments,
            advertisedUdpStabilityCapabilities:
              currentCatalog.udpStabilityCapabilities,
            args: {
              sourceIndex: active.sourceIndex,
              sourceId: active.sourceId,
              viewerPort: active.port,
              width: demand.target.width,
              height: demand.target.height,
              fps: demand.target.fps,
              captureBackend,
              mediaTransport: "auto",
              encoderExperiment: active.encoderExperiment,
              contentMode: active.contentMode,
              udpStability: active.udpStability,
              viewerDisplay: viewerMetrics,
              showFps: active.showFps ?? preferences.showFps,
              localCursor: active.localCursor ?? preferences.localCursor,
              localAudio: active.localAudio ?? preferences.localAudio,
              opusAudio: active.opusAudio ?? preferences.opusAudio,
              balancedPresentation:
                active.balancedPresentation ?? preferences.balancedPresentation,
              presentationSmooth:
                active.presentationSmooth ?? preferences.presentationSmooth,
            },
          });
          return {
            ...restarted,
            captureBackend,
            width: restarted.width ?? demand.target.width,
            height: restarted.height ?? demand.target.height,
            fps: restarted.fps ?? demand.target.fps,
            qualityState: active.qualityState,
          };
        },
        request,
      );
      return restored;
    },
    [
      host,
      preferences.showFps,
      preferences.localCursor,
      preferences.localAudio,
      preferences.opusAudio,
      preferences.balancedPresentation,
      preferences.presentationSmooth,
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
      source?: { index: number; sourceId?: string },
      operationRequest?: StreamControlRequest,
    ): Promise<RestoredStream> => {
      if (!launcher) {
        throw new LocalizedError("errResizeLauncher");
      }
      const reservation = active.reservation;
      if (!reservation) throw new Error("Missing decoder reservation");
      // 재구성(reconfigure)도 전송 끊김 후 유지된 대상으로 핀한다 — 위
      // 복구 경로와 같은 이유다(전송 손실은 선택 변경이 아니다).
      const retainedForReconfigure = captureRequestContext();
      const request =
        operationRequest ??
        requestForCurrentSelection(
          host ||
            (retainedForReconfigure
              ? `${retainedForReconfigure.target.host}:${retainedForReconfigure.target.port}`
              : ""),
        );
      const requested = {
        split:
          resolveReconfigureExperiment(active, target, {
            reconfigureEncoderExperiment:
              catalogQuery.data?.reconfigureEncoderExperiment === true,
            advertisedEncoderExperiments: catalogQuery.data?.encoderExperiments,
          }) === "splitVertical",
        target,
      };
      const demand = reservation.pool.plan(requested, reservation.lease);
      if (!demand) throw new LocalizedError("errDecoderCapacity");
      const reconfigured = await reservation.run(
        demand,
        async (reservedLauncher) => {
          const control = controlClient() ?? (await reconnectHost());
          return reconfigurePreparedStream({
            control,
            request,
            launcher: reservedLauncher,
            host: mediaHost,
            active,
            target: demand.target,
            decoderReservation: demand,
            qualityState,
            // capability가 있을 때만 인코더 모드 전환(Auto↔Split)을 요청한다.
            reconfigureEncoderExperiment:
              catalogQuery.data?.reconfigureEncoderExperiment === true,
            ...(source
              ? { sourceIndex: source.index, sourceId: source.sourceId }
              : {}),
            reconfigureSource: catalogQuery.data?.reconfigureSource === true,
            advertisedEncoderExperiments: catalogQuery.data?.encoderExperiments,
          });
        },
        request,
      );
      return {
        ...reconfigured,
        captureBackend: active.captureBackend,
      };
    },
    [catalogQuery.data, host, mediaHost],
  );

  const {
    addStream,
    applyUdpStability,
    patchStream,
    streamError,
    streams,
    syncAdaptiveTarget,
    updateLocalCursor,
  } = useStreamController(restoreActiveStream, reconfigureActiveStream);
  useEffect(() => {
    for (const [key, lifetime] of audioLifetimes.current) {
      if (
        !streams.some((stream) => sameStreamLifetime(stream, lifetime.stream))
      )
        audioLifetimes.current.delete(key);
    }
    for (const [session, lifetime] of presentationLifetimes.current) {
      if (
        !streams.some((stream) => sameStreamLifetime(stream, lifetime.stream))
      ) {
        presentationLifetimes.current.delete(session);
      }
    }
  }, [streams]);
  const replaceStreamState = useCallback(
    (expected: ActiveStream, next: ActiveStream) => {
      patchStream(expected.session, (current) =>
        sameStreamLifetime(current, expected) ? next : current,
      );
    },
    [patchStream],
  );

  const handleRefresh = useCallback(() => {
    setError(null);
    void retryAbandonedDecoderCleanup().catch((cause) =>
      setError(formatErrorMessage(cause)),
    );
    void refetchCatalog();
  }, [refetchCatalog]);

  const handleSelectProfile = useCallback(
    (id: ViewerProfileSelection) => {
      if (viewerPreferenceControlsDisabled) return;
      setPreferences((current) => ({ ...current, profileId: id }));
    },
    [setPreferences, viewerPreferenceControlsDisabled],
  );

  const handleToggleFps = useCallback(
    (showFps: boolean) => {
      if (viewerPreferenceControlsDisabled) return;
      setPreferences((current) => ({ ...current, showFps }));
    },
    [setPreferences, viewerPreferenceControlsDisabled],
  );

  const handleToggleCursor = useCallback(
    (localCursor: boolean) => {
      if (viewerPreferenceControlsDisabled) return;
      setPreferences((current) => ({ ...current, localCursor }));
      updateLocalCursor(localCursor);
      const method = launcher?.setCursorStream;
      void nativeSettings.apply("cursor", {
        host,
        getSnapshot: sessionStore.getSnapshot,
        tasks: streams.map(active => ({
          active,
          apply: () => method
            ? method(`src-${active.port}`, localCursor)
            : Promise.reject(new LocalizedError("launchFeatureError")),
        })),
      });
    },
    [
      host,
      nativeSettings,
      sessionStore,
      setPreferences,
      streams,
      updateLocalCursor,
      viewerPreferenceControlsDisabled,
    ],
  );

  const handleToggleBalancedPresentation = useCallback(
    (balancedPresentation: boolean) => {
      if (viewerPreferenceControlsDisabled) return;
      setPreferences((current) => ({ ...current, balancedPresentation }));
      const request = ++presentationRequest.current;
      const method = launcher?.setBalancedPresentation;
      const tasks = streams.map(stream => {
        let lifetime = presentationLifetimes.current.get(stream.session);
        if (!lifetime || !sameStreamLifetime(lifetime.stream, stream)) {
          lifetime = { stream, successfulRequest: 0 };
          presentationLifetimes.current.set(stream.session, lifetime);
        }
        const operation = lifetime;
        return {
          active: stream,
          apply: () => method
            ? method(`src-${stream.port}`, balancedPresentation)
            : Promise.reject(new LocalizedError("launchFeatureError")),
          commit: () => {
            // Issuing a newer request does not cancel this native operation. Only
            // a newer successful request (or a retired lifetime) supersedes it.
            if (
              presentationLifetimes.current.get(stream.session) !== operation ||
              request < operation.successfulRequest
            )
              return;
            patchStream(stream.session, (current) => {
              if (!sameStreamLifetime(current, stream)) return current;
              operation.successfulRequest = request;
              return { ...current, balancedPresentation };
            });
          },
        };
      });
      void nativeSettings.apply("balanced", { host, getSnapshot: sessionStore.getSnapshot, tasks });
    },
    [host, nativeSettings, patchStream, sessionStore, setPreferences, streams, viewerPreferenceControlsDisabled],
  );

  const applyMediaSetting = useCallback(
    (key: "localAudio" | "opusAudio" | "presentationSmooth", enabled: boolean) => {
      if (viewerPreferenceControlsDisabled) return;
      setPreferences((current) => ({ ...current, [key]: enabled }));
      const method =
        key === "localAudio"
          ? launcher?.setAudioStream
          : key === "opusAudio" ? launcher?.setOpusAudio : launcher?.setPresentationSmooth;
      const request = ++audioRequest.current;
      const tasks = streams.map(stream => {
        const lifetimeKey = `${stream.session}:${key}`;
        let operation = audioLifetimes.current.get(lifetimeKey);
        if (!operation || !sameStreamLifetime(operation.stream, stream)) {
          operation = { stream, successfulRequest: 0 };
          audioLifetimes.current.set(lifetimeKey, operation);
        }
        const captured = operation;
        return {
          active: stream,
          apply: () => method
            ? method(`src-${stream.port}`, enabled)
            : Promise.reject(new LocalizedError("launchFeatureError")),
          commit: () => {
            if (
              audioLifetimes.current.get(lifetimeKey) !== captured ||
              request < captured.successfulRequest
            )
              return;
            patchStream(stream.session, (current) => {
              if (!sameStreamLifetime(current, stream)) return current;
              captured.successfulRequest = request;
              return { ...current, [key]: enabled };
            });
          },
        };
      });
      const settingKey = key === "localAudio" ? "audio" : key === "opusAudio" ? "opus" : "smooth";
      void nativeSettings.apply(settingKey, { host, getSnapshot: sessionStore.getSnapshot, tasks });
    },
    [host, nativeSettings, patchStream, sessionStore, setPreferences, streams, viewerPreferenceControlsDisabled],
  );
  const handleToggleAudio = useCallback(
    (enabled: boolean) => applyMediaSetting("localAudio", enabled),
    [applyMediaSetting],
  );
  const handleToggleOpusAudio = useCallback(
    (enabled: boolean) => applyMediaSetting("opusAudio", enabled),
    [applyMediaSetting],
  );
  const handleTogglePresentationSmooth = useCallback(
    (enabled: boolean) => applyMediaSetting("presentationSmooth", enabled),
    [applyMediaSetting],
  );

  const handleSelectEncoderExperiment = useCallback(
    (id: EncoderExperimentId) => {
      setEncoderExperiment(id);
    },
    [],
  );

  const handleSelectUdpStability = useCallback(
    (selection: UdpStabilitySelection) => {
      if (udpPreferenceControlsDisabled) return;
      udpPreferenceRevision.current += 1;
      updateUdpPreference(() => selection);
      if (streams.length > 0) setUdpSettingsDirty(true);
    },
    [streams.length, udpPreferenceControlsDisabled, updateUdpPreference],
  );

  const handleApplyUdpStability = useCallback(() => {
    const desired = { ...effectiveUdpStability };
    const revision = udpPreferenceRevision.current;
    void nativeSettings.apply("udp", {
      host,
      getSnapshot: sessionStore.getSnapshot,
      tasks: streams.map(active => ({
        active,
        apply: (isCurrent: () => boolean) => applyUdpStability(desired, { activeStreams: [active], isCurrent })
          .catch(cause => { throw new LocalizedError("errUdpApply", { detail: formatErrorMessage(cause) }); }),
      })),
      onApplied: () => {
        // Retrying an older failed choice must not mark a later choice applied.
        if (udpPreferenceRevision.current === revision) setUdpSettingsDirty(false);
      },
    });
  }, [applyUdpStability, effectiveUdpStability, host, nativeSettings, sessionStore, streams]);

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
        const profileId = resolveViewerProfileId(
          preferences.profileId,
          display,
        );
        const displayProfile =
          STREAM_PROFILES.find((profile) => profile.id === profileId) ??
          selectedProfile;
        // 소스/사용자 최대(적응 정책의 업시프트 목표)와 새 스트림의 시작
        // 목표(responsive 1440 / clarity 최대)를 분리한다. AUTO는 실제
        // clarity 스트리밍 목표(resolveStreamingTarget)를 최대로 쓰고,
        // 수동 프로필은 기존 프로필 상한을 그대로 유지한다. 논리 데스크톱
        // 크기는 자동 품질 전환으로 바꾸지 않는다. 클라이언트 패널 메트릭이
        // 정상이면 시작·최대 모두 패널 단변으로 캡해 1:1(HiDPI)로 전송하고,
        // 메트릭이 없거나 비정상이면 기존 고정 앵커로 폴백한다.
        const viewerMetrics = await readViewerDisplayMetrics(launcher);
        const panelCap = panelShortSideCap(
          viewerMetrics
            ? {
                width: viewerMetrics.physicalWidth,
                height: viewerMetrics.physicalHeight,
              }
            : undefined,
        );
        const maximumTarget = capTargetToPanelShortSide(
          resolveStreamMaximum(display, preferences.profileId),
          panelCap,
        );
        const initialTarget = resolveInitialStreamTarget(
          display,
          streamingPriority,
          maximumTarget,
          { externalRoute: externalMediaRoute, panelShortSide: panelCap },
        );
        const requestedTarget = {
          width: initialTarget.width,
          height: initialTarget.height,
          fps: initialTarget.fps,
        };
        const demand = deviceDecoderReservations.plan({
          target: requestedTarget,
          split: requestedDecoderShape({
            encoderExperiment,
            width: requestedTarget.width,
            height: requestedTarget.height,
            advertisedEncoderExperiments,
          }),
        });
        if (!demand) throw new LocalizedError("errDecoderCapacity");
        const launchTarget = demand.target;
        // Reservation is synchronous, before the first native/network await.
        const reservation = new ReservedStream(
          port,
          demand,
          launcher,
          client.request.bind(client),
          deviceDecoderReservations,
          requestForCurrentSelection(host),
        );
        ownedReservations.add(reservation);
        try {
          const { width, height, fps } = launchTarget;
          const sourceTarget = {
            width: maximumTarget.width,
            height: maximumTarget.height,
            fps: maximumTarget.fps,
          };
          const started = await reservation.run(demand, (reservedLauncher) =>
            startPreparedStream({
              control: client,
              request: reservation.controlRequest,
              launcher: reservedLauncher,
              decoderReservation: demand,
              host: mediaHost,
              advertisedEncoderExperiments,
              advertisedUdpStabilityCapabilities:
                catalogQuery.data?.udpStabilityCapabilities,
              args: {
                sourceIndex: display.index,
                sourceId: display.sourceId,
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
                viewerDisplay: viewerMetrics,
                // 호스트가 기억한 마지막 창 크기 — 새 창이 같은 크기로 열린다.
                windowWidthPx: catalogQuery.data?.windowSize?.widthPx ?? 0,
                windowHeightPx: catalogQuery.data?.windowSize?.heightPx ?? 0,
                showFps: preferences.showFps,
                localCursor: preferences.localCursor,
                localAudio: preferences.localAudio,
                opusAudio: preferences.opusAudio,
                balancedPresentation: preferences.balancedPresentation,
                presentationSmooth: preferences.presentationSmooth,
              },
            }),
          );
          const acceptedTarget = {
            width: started.width ?? width,
            height: started.height ?? height,
            fps: started.fps ?? fps,
          };
          addStream({
            reservation,
            port,
            session: started.session,
            sourceIndex: display.index,
            sourceId: display.sourceId,
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
            opusAudio: started.opusAudio ?? false,
            balancedPresentation: started.balancedPresentation ?? false,
            presentationSmooth: started.presentationSmooth ?? true,
            viewerIps: started.viewerIps,
            mediaTransport: started.mediaTransport,
            mediaKey: started.mediaKey,
            startedAt: Date.now(),
          });
        } catch (cause) {
          try {
            await reservation.close();
          } catch (cleanupError) {
            // No ActiveStream/Stop action exists for a failed launch. Transfer
            // its counted lease to the cleanup queue used by visible Refresh.
            reservation.retainForCleanupRetry();
            throw cleanupError;
          } finally {
            ownedReservations.delete(reservation);
          }
          throw cause;
        }
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
      catalogQuery.data?.windowSize,
      effectiveCaptureBackend,
      encoderExperiment,
      effectiveUdpStability,
      host,
      mediaHost,
      externalMediaRoute,
      preferences.profileId,
      preferences.showFps,
      preferences.localCursor,
      preferences.localAudio,
      preferences.opusAudio,
      preferences.balancedPresentation,
      preferences.presentationSmooth,
      selectedProfile,
      ownedReservations,
      streamingPriority,
    ],
  );

  const stopStream = useCallback(
    async (active: ActiveStream) => {
      try {
        return await runStreamLifetimeOperation({
          host,
          active,
          getSnapshot: sessionStore.getSnapshot,
          work: async (request) => {
            if (active.reservation) await active.reservation.close();
            else await request("stopStream", { session: active.session });
          },
          commit: () => {
            sessionStore.update((previous) =>
              previous.filter(
                (current) => !sameStreamLifetime(current, active),
              ),
            );
            if (active.reservation)
              ownedReservations.delete(active.reservation);
          },
        });
      } catch (cause) {
        setError(formatErrorMessage(cause));
        return false;
      }
    },
    [host, sessionStore, ownedReservations],
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
        return await runStreamLifetimeOperation({
          host,
          active,
          getSnapshot: sessionStore.getSnapshot,
          work: (request) =>
            reconfigureActiveStream(
              active,
              target,
              "native",
              undefined,
              request,
            ),
          commit: (accepted, current) => {
            const next = streamTargetAfterResize(current, target, accepted);
            syncAdaptiveTarget(
              current.session,
              next.sourceTarget,
              next.activeTarget,
            );
            replaceStreamState(current, next);
          },
        });
      } catch (cause) {
        setError(
          interpolate(currentTranslation().viewer.errResizeFailed, {
            detail: formatErrorMessage(cause),
          }),
        );
        return false;
      } finally {
        setResizingSession(null);
      }
    },
    [
      host,
      sessionStore,
      reconfigureActiveStream,
      replaceStreamState,
      syncAdaptiveTarget,
    ],
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
      if (
        display.sourceId && active.sourceId
          ? display.sourceId === active.sourceId
          : !display.sourceId &&
            !active.sourceId &&
            display.index === active.sourceIndex
      )
        return true;
      setSwitchingSession(active.session);
      try {
        return await runStreamLifetimeOperation({
          host,
          active,
          getSnapshot: sessionStore.getSnapshot,
          work: async (request) => {
            // 새 디스플레이를 새 창을 여는 것과 같은 규칙으로 맞춘다: 스트리밍
            // 목표는 디스플레이+프로필에서, 적응 상태는 새 디스플레이의 최대
            // 목표에 다시 심는다(openDisplay의 시딩과 동일).
            const profileId = resolveViewerProfileId(
              preferences.profileId,
              display,
            );
            const metrics = await readViewerDisplayMetrics(launcher);
            const panelCap = panelShortSideCap(
              metrics
                ? {
                    width: metrics.physicalWidth,
                    height: metrics.physicalHeight,
                  }
                : undefined,
            );
            const maximumTarget = capTargetToPanelShortSide(
              resolveStreamMaximum(display, preferences.profileId),
              panelCap,
            );
            const initialTarget = resolveInitialStreamTarget(
              display,
              streamingPriority,
              maximumTarget,
              { externalRoute: externalMediaRoute, panelShortSide: panelCap },
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
              { index: display.index, sourceId: display.sourceId },
              request,
            );
            const sourceTarget = {
              width: maximumTarget.width,
              height: maximumTarget.height,
              fps: maximumTarget.fps,
            };
            return { reconfigured, sourceTarget, target };
          },
          commit: ({ reconfigured, sourceTarget, target }, current) => {
            const acceptedTarget = {
              width: reconfigured.width ?? target.width,
              height: reconfigured.height ?? target.height,
              fps: reconfigured.fps ?? target.fps,
            };
            replaceStreamState(current, {
              ...streamTargetAfterResize(current, target, reconfigured),
              sourceIndex: reconfigured.sourceIndex ?? display.index,
              sourceId: display.sourceId,
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
            syncAdaptiveTarget(current.session, sourceTarget, acceptedTarget);
          },
        });
      } catch (cause) {
        setError(formatErrorMessage(cause));
        return false;
      } finally {
        setSwitchingSession(null);
      }
    },
    [
      externalMediaRoute,
      host,
      sessionStore,
      preferences.profileId,
      reconfigureActiveStream,
      replaceStreamState,
      streamingPriority,
      syncAdaptiveTarget,
    ],
  );

  /** 클라이언트 패널 단변(물리) — 카드 라벨·시작 크기 계산에 쓰인다. 메트릭이
   * 없거나 비정상이면 undefined(고정 앵커 폴백). */
  const [panelShortSide, setPanelShortSide] = useState<number | undefined>(
    undefined,
  );
  useEffect(() => {
    let cancelled = false;
    void readViewerDisplayMetrics(launcher).then((metrics) => {
      if (cancelled) return;
      setPanelShortSide(
        panelShortSideCap(
          metrics
            ? { width: metrics.physicalWidth, height: metrics.physicalHeight }
            : undefined,
        ),
      );
    });
    return () => {
      cancelled = true;
    };
  }, []);
  const extension = useExtensionDisplay({
    host,
    catalog: catalogQuery.data,
    launcher,
    streams,
    refetchCatalog,
    openDisplay,
    stopStream,
  });

  const visibleError =
    error ||
    extension.extensionError ||
    streamError ||
    (catalogQuery.error ? catalogErrorMessage(catalogQuery.error) : null) ||
    (catalogQuery.data && displays.length === 0
      ? new LocalizedError("errSourceAccess").format()
      : null);

  return {
    displays,
    ...extension,
    inputAllowed: catalogQuery.data?.inputAllowed,
    panelShortSide,
    effectiveNextEncoderExperiment,
    effectiveUdpStability,
    handleApplyUdpStability,
    handleRefresh,
    handleResizeSession,
    handleSelectEncoderExperiment,
    handleSelectProfile,
    handleSelectUdpStability,
    handleSwitchSessionSource,
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
    viewerPreferenceControlsDisabled,
    clipboardPreferenceControlDisabled,
    udpPreferenceControlsDisabled,
    preferencePersistenceIssue,
    preferenceLoading,
    preferenceSaving,
    retryPersistence,
    nativeSettingsApplying,
    nativeSettingsFailures: nativeSettingsStatus.failures,
    handleRetryNativeSettings,
    visibleError,
    handleToggleFps,
    handleToggleCursor,
    handleToggleAudio,
    handleToggleBalancedPresentation,
    handleTogglePresentationSmooth,
    handleToggleClipboardShare,
    clipboardShare,
    profileId: preferences.profileId,
    streamingPriority,
    externalMediaRoute,
    showFps: preferences.showFps,
    localCursor: preferences.localCursor,
    localAudio: preferences.localAudio,
    opusAudio: preferences.opusAudio ?? false,
    handleToggleOpusAudio,
    balancedPresentation: preferences.balancedPresentation,
    presentationSmooth: preferences.presentationSmooth,
    resizingSession,
    switchingSession,
  };
}
