import {
  type Dispatch,
  type SetStateAction,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Alert } from "react-native";
import { replaceRestartedStreamState } from "./launch-stream";
import {
  createAdaptiveResolutionState,
  observeAdaptiveResolution,
  recordAdaptiveResolutionResult,
  type AdaptiveQualityState,
  type AdaptiveResolutionAction,
  type AdaptiveResolutionState,
  type AdaptiveTarget,
} from "./adaptive-resolution";
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
import type { ActiveStream, RestoredStream } from "./catalog-model-types";
import type { UdpStabilitySelection } from "./udp-stability";

export function useStreamController(
  setError: Dispatch<SetStateAction<string | null>>,
  restoreStream: (active: ActiveStream) => Promise<RestoredStream>,
  reconfigureStream?: (
    active: ActiveStream,
    target: AdaptiveTarget,
    qualityState: AdaptiveQualityState,
  ) => Promise<RestoredStream>,
) {
  const [streams, setStreams] = useState<ActiveStream[]>([]);
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
        setError(
          `소유권이 사라진 화면 공유를 종료하지 못했습니다: ${formatErrorMessage(cause)}`,
        );
      });
    },
    [setError],
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
      setError(null);
      void queryClient.invalidateQueries({ queryKey: ["host-status", host] });
    },
    onError: (error, _request) => {
      // A failed bounded rebind keeps the existing logical stream visible so
      // the same Activity can retry on its current Surface; the caller owns
      // the explicit retry action and no automatic loop is created.
      updateStreams((previous) => previous);
      setError(`화면을 다시 연결하지 못했습니다: ${formatErrorMessage(error)}`);
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
        adaptiveStates.current.set(
          active.session,
          createAdaptiveResolutionState(active.sourceTarget),
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
      setError(null);
      void queryClient.invalidateQueries({ queryKey: ["host-status", host] });
    },
    onError: (error, active) => {
      setError(`전송 경로를 바꾸지 못했습니다: ${formatErrorMessage(error)}`);
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
          Alert.alert(
            "화면 공유가 종료되었어요",
            hostTermination === "feedbackTimeout"
              ? "컴퓨터와의 연결이 끊어져 화면 공유를 종료했습니다."
              : "컴퓨터에서 이 화면 공유를 종료했습니다.",
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
      adaptiveRebinds.current.add(active.session);
      const pendingQualityState: AdaptiveQualityState =
        action.kind === "downshift" ? "fallback" : "native";
      try {
        const restarted = await reconfigure(active, action.target, pendingQualityState);
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
        setError(null);
      } catch (error) {
        const result = recordAdaptiveResolutionResult(
          state,
          action,
          false,
          Date.now(),
        );
        adaptiveStates.current.set(active.session, result.state);
        setError(
          `해상도 전환에 실패했습니다. 현재 화면에서 다시 시도할 수 있습니다: ${formatErrorMessage(error)}`,
        );
      } finally {
        adaptiveRebinds.current.delete(active.session);
      }
    },
    [setError, updateStreams],
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
        createAdaptiveResolutionState(active.sourceTarget);
      const observed = observeAdaptiveResolution(state, {
        nowMs: Date.now(),
        receiverLossDelta: Math.max(0, loss - previousLoss),
        encodedFps: session.encodeOutputFps ?? session.fps,
        requestedFps: active.activeTarget.fps,
        queueAgeUs: session.pendingFrameOldestAgeUs ?? 0,
        latencyBudgetUs: 100_000,
        recoveryActive: recovery > previousRecovery,
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
  /** In-place state patch for one session (e.g. a user-driven resolution change). */
  const patchStream = useCallback((session: number, patch: (active: ActiveStream) => ActiveStream) => {
    updateStreams((previous) => previous.map((stream) => (stream.session === session ? patch(stream) : stream)));
  }, [updateStreams]);
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
          setError(`UDP 설정을 적용하지 못했습니다: ${formatErrorMessage(cause)}`);
          throw cause;
        } finally {
          releaseStreamRestore(heartbeatInFlight.current, active.session);
        }
        return reconnectAt(index + 1);
      };
      await reconnectAt(0);
      setError(null);
    },
    [endUnownedRestart, host, queryClient, restoreStream, setError, updateStreams],
  );

  return { addStream, applyUdpStability, patchStream, removeStream, streams, updateLocalCursor };
}
