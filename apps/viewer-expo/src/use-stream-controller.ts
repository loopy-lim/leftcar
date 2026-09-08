import { useCallback, useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Alert } from "react-native";
import { replaceRestartedStreamState } from "./launch-stream";
import {
  observeAdaptiveResolution,
  recordAdaptiveResolutionResult,
  seedAdaptiveResolutionState,
  type AdaptiveQualityState,
  type AdaptiveResolutionAction,
  type AdaptiveResolutionState,
  type AdaptiveTarget,
} from "./adaptive-resolution";
import {
  hostQueuePressureUs,
  receiverRenderedFps,
} from "./receiver-telemetry";
import { requestWithReconnect } from "./catalog-helpers";
import { controlHost } from "./session";
import { shouldSwitchTransport } from "./transport-switch";
import {
  claimStreamRestore,
  classifyHostTermination,
  releaseStreamRestore,
  selectRecoverableStream,
  subscribeStreamTermination,
  type RestartRequest,
} from "./stream-termination";
import {
  getUsbState,
  resolveTransport,
  subscribeUsbState,
  type UsbAccessoryState,
} from "./usb";
import { formatErrorMessage, type StatusView } from "./control";
import { currentTranslation } from "./language-store";
import { interpolate } from "@leftcar/ui-tokens";
import type { ActiveStream, RestoredStream } from "./catalog-model-types";
import type { UdpStabilitySelection } from "./udp-stability";

export function useStreamController(
  restoreStream: (active: ActiveStream) => Promise<RestoredStream>,
  reconfigureStream?: (
    active: ActiveStream,
    target: AdaptiveTarget,
    qualityState: AdaptiveQualityState,
  ) => Promise<RestoredStream>,
) {
  // 오류 문구는 발생 시점 언어를 따른다 — 훅 t를 넣으면 언어 전환마다
  // 장기 콜백 신원이 흔들리므로 모듈 저장소에서 직접 읽는다.
  const [streams, setStreams] = useState<ActiveStream[]>([]);
  // 스트림 오류는 이 훅이 소유하고 반환한다 — 부모 setter를 effect에서
  // 호출해 상태를 끌어올리는 대신, 표시 주체(카탈로그)가 반환값을 합성한다.
  const [streamError, setStreamError] = useState<string | null>(null);
  const heartbeatInFlight = useRef(new Set<number>());
  const lastRestartAt = useRef(new Map<number, number>());
  const notifiedTerminations = useRef(new Set<number>());
  const streamsRef = useRef<ActiveStream[]>([]);
  const adaptiveStates = useRef(new Map<number, AdaptiveResolutionState>());
  const adaptiveLoss = useRef(new Map<number, number>());
  const adaptiveRecovery = useRef(new Map<number, number>());
  const adaptiveFloorCollapse = useRef(new Map<number, number>());
  const adaptiveRebinds = useRef(new Set<number>());
  const reconfigureStreamRef = useRef(reconfigureStream);
  const updateStreams = useCallback(
    (transition: (previous: ActiveStream[]) => ActiveStream[]) => {
      const next = transition(streamsRef.current);
      streamsRef.current = next;
      setStreams(next);
    },
    [],
  );
  const endUnownedRestart = useCallback(
    (session: number) => {
      void requestWithReconnect("stopStream", { session }).catch((cause) => {
        setStreamError(interpolate(currentTranslation().viewer.errStopOrphaned, { detail: formatErrorMessage(cause) }));
      });
    },
    [],
  );
  const host = controlHost();
  const queryClient = useQueryClient();
  const statusQuery = useQuery({
    queryKey: ["host-status", host],
    queryFn: () => requestWithReconnect<StatusView>("getStatus"),
    // 1s samples feed the adaptive-resolution observer only while a stream
    // is live; the catalog screen falls back to the idle 2s cadence.
    refetchInterval: streams.length > 0 ? 1_000 : 2_000,
    staleTime: 1_000,
  });
  const statusView = statusQuery.data;

  const { mutate: restartStream } = useMutation({
    mutationFn: async (request: RestartRequest) => {
      const restarted = await restoreStream(request.active);
      return { ...request, restarted };
    },
    onSuccess: ({ active, restarted }) => {
      updateStreams((previous) =>
        replaceRestartedStreamState(
          previous,
          active.session,
          restarted,
          Date.now(),
          endUnownedRestart,
        ),
      );
      setStreamError(null);
      void queryClient.invalidateQueries({ queryKey: ["host-status", host] });
    },
    onError: (error, _request) => {
      // A failed bounded rebind keeps the existing logical stream visible so
      // the same Activity can retry on its current Surface; the caller owns
      // the explicit retry action and no automatic loop is created.
      updateStreams((previous) => previous);
      setStreamError(interpolate(currentTranslation().viewer.errRestoreFailed, { detail: formatErrorMessage(error) }));
    },
    onSettled: (_data, _error, request) => {
      releaseStreamRestore(heartbeatInFlight.current, request.active.session);
    },
  });

  const restartStreamRef = useRef(restartStream);
  useEffect(() => {
    restartStreamRef.current = restartStream;
  }, [restartStream]);

  useEffect(() => {
    reconfigureStreamRef.current = reconfigureStream;
  }, [reconfigureStream]);

  useEffect(() => {
    const activeIds = new Set(streams.map((stream) => stream.session));
    for (const active of streams) {
      if (!adaptiveStates.current.has(active.session)) {
        // Seed from what is actually running (responsive starts sit below the
        // source maximum), not from the maximum itself. Seeding only happens
        // when a session appears — never per sample, so hysteresis keeps
        // accumulating between real changes.
        adaptiveStates.current.set(
          active.session,
          seedAdaptiveResolutionState(
            active.sourceTarget,
            active.activeTarget,
            { nowMs: Date.now() },
          ),
        );
      }
    }
    for (const session of adaptiveStates.current.keys()) {
      if (!activeIds.has(session)) {
        adaptiveStates.current.delete(session);
        adaptiveLoss.current.delete(session);
        adaptiveRecovery.current.delete(session);
        adaptiveFloorCollapse.current.delete(session);
        adaptiveRebinds.current.delete(session);
      }
    }
  }, [streams]);

  useEffect(() => {
    const subscription = subscribeStreamTermination((event) => {
      const active = selectRecoverableStream(
        streamsRef.current,
        event,
        heartbeatInFlight.current,
      );
      if (!active || !claimStreamRestore(heartbeatInFlight.current, active.session)) {
        return;
      }
      restartStreamRef.current({ active, trigger: "nativeTermination" });
    });
    return () => subscription.remove();
  }, []);

  const { mutate: switchTransport } = useMutation({
    mutationFn: async (active: ActiveStream) => {
      const restarted = await restoreStream(active);
      return { active, restarted };
    },
    onSuccess: ({ active, restarted }) => {
      updateStreams((previous) =>
        replaceRestartedStreamState(
          previous,
          active.session,
          restarted,
          Date.now(),
          endUnownedRestart,
        ),
      );
      setStreamError(null);
      void queryClient.invalidateQueries({ queryKey: ["host-status", host] });
    },
    onError: (error, active) => {
      setStreamError(interpolate(currentTranslation().viewer.errTransportSwitchFailed, { detail: formatErrorMessage(error) }));
    },
    onSettled: (_data, _error, active) => {
      releaseStreamRestore(heartbeatInFlight.current, active.session);
    },
  });

  const switchTransportRef = useRef(switchTransport);
  useEffect(() => {
    switchTransportRef.current = switchTransport;
  }, [switchTransport]);

  useEffect(() => {
    if (!statusView) return;
    const sessionsById = new Map(
      statusView.sessions.map((session) => [session.session, session]),
    );
    const now = Date.now();
    for (const active of streams) {
      const session = sessionsById.get(active.session);
      const unhealthy =
        !session || ["error", "stopped", "unknown"].includes(session.state);
      const terminalMessage = session?.error ?? "";
      const hostTermination = classifyHostTermination(terminalMessage);
      if (hostTermination) {
        updateStreams((previous) =>
          previous.filter((item) => item.session !== active.session),
        );
        lastRestartAt.current.delete(active.session);
        if (
          hostTermination !== "viewerClosed" &&
          !notifiedTerminations.current.has(active.session)
        ) {
          notifiedTerminations.current.add(active.session);
          const copy = currentTranslation().viewer;
          Alert.alert(
            copy.streamEndedTitle,
            hostTermination === "feedbackTimeout"
              ? copy.streamEndedByDisconnect
              : copy.streamEndedByHost,
          );
        }
        continue;
      }
      if (
        !unhealthy ||
        now - active.startedAt < 5_000 ||
        now - (lastRestartAt.current.get(active.session) ?? 0) < 5_000 ||
        heartbeatInFlight.current.has(active.session)
      ) {
        continue;
      }

      if (!claimStreamRestore(heartbeatInFlight.current, active.session)) {
        continue;
      }
      lastRestartAt.current.set(active.session, now);
      restartStream({ active, trigger: "hostStatus" });
    }
  }, [restartStream, statusView, streams, updateStreams]);

  const runAdaptiveRebind = useCallback(
    async (
      active: ActiveStream,
      state: AdaptiveResolutionState,
      action: Exclude<AdaptiveResolutionAction, { kind: "keep" }>,
    ) => {
      const reconfigure = reconfigureStreamRef.current;
      if (!reconfigure || adaptiveRebinds.current.has(active.session)) return;
      // A pending restart/transport switch owns the session; an adaptive
      // reconfigure issued underneath it would race the replacement.
      if (heartbeatInFlight.current.has(active.session)) return;
      adaptiveRebinds.current.add(active.session);
      const pendingQualityState: AdaptiveQualityState =
        action.kind === "downshift" ? "fallback" : "native";
      try {
          const restarted = await reconfigure(active, action.target, pendingQualityState);
        // The session may have been replaced/removed while the reconfigure
        // was in flight; only apply results to the stream that still exists.
        if (!streamsRef.current.some((item) => item.session === active.session)) {
          return;
        }
        const acceptedTarget = {
          width: restarted.width ?? action.target.width,
          height: restarted.height ?? action.target.height,
          fps: restarted.fps ?? action.target.fps,
        };
        const result = recordAdaptiveResolutionResult(
          state,
          action,
          true,
          Date.now(),
          acceptedTarget,
        );
        adaptiveStates.current.set(active.session, result.state);
        updateStreams((previous) => previous.map((item) => {
          if (item.session !== active.session) return item;
          return {
            ...item,
            ...restarted,
            width: acceptedTarget.width,
            height: acceptedTarget.height,
            fps: acceptedTarget.fps,
            activeTarget: acceptedTarget,
            qualityState: result.state.qualityState,
          };
        }));
        setStreamError(null);
      } catch (error) {
        const result = recordAdaptiveResolutionResult(
          state,
          action,
          false,
          Date.now(),
        );
        adaptiveStates.current.set(active.session, result.state);
        setStreamError(interpolate(currentTranslation().viewer.errAdaptiveResize, { detail: formatErrorMessage(error) }));
      } finally {
        adaptiveRebinds.current.delete(active.session);
      }
    },
    [updateStreams],
  );

  useEffect(() => {
    if (!statusView || !reconfigureStreamRef.current) return;
    const sessionsById = new Map(
      statusView.sessions.map((session) => [session.session, session]),
    );
    for (const active of streamsRef.current) {
      const session = sessionsById.get(active.session);
      if (!session) continue;
      const loss =
        (session.receiverFrameGaps ?? 0) +
        (session.receiverInputDrops ?? 0) +
        (session.receiverIncompleteAus ?? 0) +
        (session.receiverStaleInputDrops ?? session.receiverStaleFrames ?? 0);
      const recovery =
        (session.recoveryKeyframes ?? 0) +
        (session.receiverPairedIdrEpisodes ?? 0) +
        (session.receiverSuppressedRecoveryRequests ?? 0);
      const floorCollapse = session.bitrateFloorCollapseCount ?? 0;
      const previousFloorCollapse = adaptiveFloorCollapse.current.get(active.session);
      const previousLoss = adaptiveLoss.current.get(active.session);
      const previousRecovery = adaptiveRecovery.current.get(active.session);
      adaptiveLoss.current.set(active.session, loss);
      adaptiveRecovery.current.set(active.session, recovery);
      adaptiveFloorCollapse.current.set(active.session, floorCollapse);
      if (previousLoss === undefined || previousRecovery === undefined || previousFloorCollapse === undefined) continue;
      const state = adaptiveStates.current.get(active.session) ??
        seedAdaptiveResolutionState(active.sourceTarget, active.activeTarget);
      // 수신기 renderedFps는 신선한 피드백에서만 관측값으로 전달한다 —
      // 없거나 오래된 값은 0으로 만들지 않고 생략한다 (구 Host 호환).
      const renderedFps = receiverRenderedFps(session);
      const observed = observeAdaptiveResolution(state, {
        nowMs: Date.now(),
        receiverLossDelta: Math.max(0, loss - previousLoss),
        encodedFps: session.encodeOutputFps ?? session.fps,
        renderedFps,
        requestedFps: active.activeTarget.fps,
        // Host 큐 압력은 인코딩 대기열, 분할 인코딩 큐, 분할 캡처 큐 중
        // 가장 오래된 값으로 본다.
        queueAgeUs: hostQueuePressureUs(session),
        latencyBudgetUs: 100_000,
        // 실시간 active-burst 필드는 아직 존재하지 않는다. 누적 카운터
        // 델타는 관측 증거(recoveryObserved)로만 전달하고, 진행 중
        // 폭발(recoveryActive)로 거짓 표기해 측정을 계속 무효화하지
        // 않는다.
        recoveryActive: false,
        recoveryObserved: recovery > previousRecovery,
        floorCollapseDelta: Math.max(0, floorCollapse - previousFloorCollapse),
        rebindInFlight: adaptiveRebinds.current.has(active.session),
      });
      adaptiveStates.current.set(active.session, observed.state);
      if (observed.action.kind !== "keep") {
        void runAdaptiveRebind(active, observed.state, observed.action);
      }
    }
  }, [runAdaptiveRebind, statusView]);

  const [usbState, setUsbState] = useState<UsbAccessoryState>({
    attached: false,
    controlPort: 0,
  });

  useEffect(() => {
    let disposed = false;
    void getUsbState().then((state) => {
      if (!disposed) setUsbState(state);
    });
    const subscription = subscribeUsbState(setUsbState);
    return () => {
      disposed = true;
      subscription.remove();
    };
  }, []);

  useEffect(() => {
    const target = resolveTransport(usbState, "auto");
    const timer = setTimeout(() => {
      for (const active of streamsRef.current) {
        if (
          target === active.mediaTransport ||
          !shouldSwitchTransport(
            active.mediaTransport,
            usbState,
            active.encoderExperiment,
          )
        ) {
          continue;
        }
        if (!claimStreamRestore(heartbeatInFlight.current, active.session)) {
          continue;
        }
        switchTransportRef.current(active);
      }
    }, 1_000);
    return () => clearTimeout(timer);
  }, [usbState]);

  const addStream = useCallback((stream: ActiveStream) => {
    updateStreams((previous) => [...previous, stream]);
  }, [updateStreams]);
  const removeStream = useCallback((session: number) => {
    updateStreams((previous) => previous.filter((stream) => stream.session !== session));
  }, [updateStreams]);
  const updateLocalCursor = useCallback((localCursor: boolean) => {
    updateStreams((previous) => previous.map((stream) => ({ ...stream, localCursor })));
  }, [updateStreams]);
  const updateLocalAudio = useCallback((localAudio: boolean) => {
    updateStreams((previous) => previous.map((stream) => ({ ...stream, localAudio })));
  }, [updateStreams]);
  /** In-place state patch for one session (e.g. a user-driven resolution change). */
  const patchStream = useCallback((session: number, patch: (active: ActiveStream) => ActiveStream) => {
    updateStreams((previous) => previous.map((stream) => (stream.session === session ? patch(stream) : stream)));
  }, [updateStreams]);
  /**
   * Re-seed adaptive state after an explicit target change (manual resize or
   * restore). Called only on real changes — never per sample — so window
   * hysteresis is not silently reset while the user is not touching anything.
   */
  const syncAdaptiveTarget = useCallback(
    (session: number, sourceTarget: AdaptiveTarget, activeTarget: AdaptiveTarget) => {
      adaptiveStates.current.set(
        session,
        seedAdaptiveResolutionState(sourceTarget, activeTarget, { nowMs: Date.now() }),
      );
    },
    [],
  );
  const applyUdpStability = useCallback(
    async (udpStability: UdpStabilitySelection) => {
      const activeStreams = [...streamsRef.current];
      const reconnectAt = async (index: number): Promise<void> => {
        const active = activeStreams[index];
        if (!active) return;
        const configured = { ...active, udpStability };
        if (!claimStreamRestore(heartbeatInFlight.current, active.session)) {
          return reconnectAt(index + 1);
        }
        try {
          const restarted = await restoreStream(configured);
          updateStreams((previous) =>
            replaceRestartedStreamState(
              previous,
              active.session,
              restarted,
              Date.now(),
              endUnownedRestart,
            ),
          );
          void queryClient.invalidateQueries({ queryKey: ["host-status", host] });
        } catch (cause) {
          setStreamError(interpolate(currentTranslation().viewer.errUdpApply, { detail: formatErrorMessage(cause) }));
          throw cause;
        } finally {
          releaseStreamRestore(heartbeatInFlight.current, active.session);
        }
        return reconnectAt(index + 1);
      };
      await reconnectAt(0);
      setStreamError(null);
    },
    [endUnownedRestart, host, queryClient, restoreStream, updateStreams],
  );

  return {
    streamError,
    addStream,
    applyUdpStability,
    patchStream,
    removeStream,
    streams,
    syncAdaptiveTarget,
    updateLocalCursor,
    updateLocalAudio,
  };
}
