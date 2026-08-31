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
import { requestWithReconnect } from "./catalog-helpers";
import { controlHost } from "./session";
import { shouldSwitchTransport } from "./transport-switch";
import {
  claimStreamRestore,
  reduceRestartFailure,
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
import type { StatusView } from "./control";
import type { ActiveStream, RestoredStream } from "./catalog-model-types";
import type { UdpStabilitySelection } from "./udp-stability";

export function useStreamController(
  setError: Dispatch<SetStateAction<string | null>>,
  restoreStream: (active: ActiveStream) => Promise<RestoredStream>,
) {
  const [streams, setStreams] = useState<ActiveStream[]>([]);
  const heartbeatInFlight = useRef(new Set<number>());
  const lastRestartAt = useRef(new Map<number, number>());
  const notifiedTerminations = useRef(new Set<number>());
  const streamsRef = useRef<ActiveStream[]>([]);
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
          `소유권이 사라진 화면 공유를 종료하지 못했습니다: ${String(
            cause instanceof Error ? cause.message : cause,
          )}`,
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
    refetchInterval: 2_000,
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
    onError: (error, request) => {
      updateStreams((previous) =>
        reduceRestartFailure(previous, request),
      );
      setError(
        `화면을 다시 연결하지 못했습니다: ${String(
          error instanceof Error ? error.message : error,
        )}`,
      );
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
      setError(
        `전송 경로를 바꾸지 못했습니다: ${String(
          error instanceof Error ? error.message : error,
        )}`,
      );
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
      const hostTerminated =
        terminalMessage === "viewer closed stream" ||
        terminalMessage.includes("feedback timeout") ||
        terminalMessage.includes("host operator stopped");
      if (hostTerminated) {
        updateStreams((previous) =>
          previous.filter((item) => item.session !== active.session),
        );
        lastRestartAt.current.delete(active.session);
        if (!notifiedTerminations.current.has(active.session)) {
          notifiedTerminations.current.add(active.session);
          Alert.alert(
            "화면 공유가 종료되었어요",
            terminalMessage.includes("feedback timeout")
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
          !shouldSwitchTransport(active.mediaTransport, usbState)
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
          setError(
            `UDP 설정을 적용하지 못했습니다: ${String(
              cause instanceof Error ? cause.message : cause,
            )}`,
          );
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

  return { addStream, applyUdpStability, removeStream, streams };
}
