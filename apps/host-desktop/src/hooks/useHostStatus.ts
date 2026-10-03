import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import { trayStatus, type HostSnapshotView } from "../hostState";
import type { SessionRow } from "../sessionTypes";
import {
  createTerminationNotice,
  isTerminalSession,
  type TerminationNotice,
} from "../streamTermination";

export type HostErrorKind =
  | "remote-desktop-permission"
  | "screen-permission"
  | "network"
  | "service"
  | "generic";

export interface HostErrorView {
  message: string;
  kind: HostErrorKind;
}

export function hostErrorView(
  cause: unknown,
  t: TranslationSchema,
): HostErrorView {
  const message = String(
    cause instanceof Error ? cause.message : cause,
  ).toLowerCase();
  if (message.includes("remote desktop")) {
    return {
      message: t.host.screenPermissionError,
      kind: "remote-desktop-permission",
    };
  }
  if (message.includes("permission") || message.includes("not authorized")) {
    return { message: t.host.screenPermissionError, kind: "screen-permission" };
  }
  if (message.includes("no lan interface")) {
    return { message: t.host.networkNotFoundError, kind: "network" };
  }
  if (message.includes("invoke") || message.includes("initialization")) {
    return { message: t.host.appServiceInitError, kind: "service" };
  }
  return { message: t.host.connectionCheckError, kind: "generic" };
}

interface StatusView {
  sessions: SessionRow[];
}

/** 뷰어 잠금 배너 탭으로 접수된 입력 허용 요청(Tauri list_input_requests). */
export interface InputRequestRow {
  session: number;
  device: string | null;
  ageMs: number;
}

export function useHostStatus(t: TranslationSchema) {
  const [banner, setBanner] = useState("Leftcar");
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [inputRequests, setInputRequests] = useState<InputRequestRow[]>([]);
  const [terminationNotice, setTerminationNotice] =
    useState<TerminationNotice | null>(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<HostErrorView | null>(null);
  const [inputPermission, setInputPermission] = useState(false);
  const [screenPermission, setScreenPermission] = useState(true);
  const [platform, setPlatform] =
    useState<HostSnapshotView["platform"]>("macos");
  const [controlPort, setControlPort] = useState(7777);
  const [lanIp, setLanIp] = useState<string | null>(null);
  const [lastUpdated, setLastUpdated] = useState<Date>(new Date());
  const priorActiveSessions = useRef(new Map<number, SessionRow>());
  const seenTerminations = useRef(new Set<string>());
  const hasStatusSnapshot = useRef(false);
  const refreshRevision = useRef(0);
  const mounted = useRef(false);

  const refresh = useCallback(async () => {
    const revision = ++refreshRevision.current;
    try {
      const [
        status,
        permission,
        screenGranted,
        hostPlatform,
        actualControlPort,
        actualLanIp,
        requests,
      ] = await Promise.all([
        invoke<StatusView>("get_status"),
        invoke<boolean>("get_input_permission"),
        invoke<boolean>("get_screen_permission"),
        invoke<HostSnapshotView["platform"]>("get_host_platform"),
        invoke<number>("get_control_port"),
        invoke<string | null>("get_lan_ip").catch(() => null),
        invoke<InputRequestRow[]>("list_input_requests").catch(
          () => [] as InputRequestRow[],
        ),
      ]);
      if (!mounted.current || revision !== refreshRevision.current) return;
      const statusSessions = status.sessions || [];
      const activeSessions = statusSessions.filter(
        (session) => !isTerminalSession(session),
      );
      let nextTerminationNotice: TerminationNotice | null = null;

      for (const terminalSession of statusSessions.filter(isTerminalSession)) {
        const notice = createTerminationNotice(terminalSession);
        if (!seenTerminations.current.has(notice.key)) {
          seenTerminations.current.add(notice.key);
          nextTerminationNotice = notice;
        }
      }

      if (hasStatusSnapshot.current) {
        const reportedSessionIds = new Set(
          statusSessions.map((session) => session.session),
        );
        for (const priorSession of priorActiveSessions.current.values()) {
          if (reportedSessionIds.has(priorSession.session)) continue;
          const notice = createTerminationNotice({
            ...priorSession,
            state: "stopped",
            error: null,
          });
          if (!seenTerminations.current.has(notice.key)) {
            seenTerminations.current.add(notice.key);
            nextTerminationNotice = notice;
          }
        }
      }

      priorActiveSessions.current = new Map(
        activeSessions.map((session) => [session.session, session]),
      );
      hasStatusSnapshot.current = true;
      setReady(true);
      setSessions(activeSessions);
      setInputRequests(requests);
      if (nextTerminationNotice) setTerminationNotice(nextTerminationNotice);
      setBanner(
        trayStatus({
          platform: hostPlatform,
          pairingState: "connected",
          activeStreamCount: activeSessions.length,
        } satisfies HostSnapshotView),
      );
      setError(null);
      setInputPermission(permission);
      setScreenPermission(screenGranted);
      setPlatform(hostPlatform);
      setControlPort(actualControlPort);
      setLanIp(actualLanIp);
      setLastUpdated(new Date());
    } catch (cause) {
      if (mounted.current && revision === refreshRevision.current)
        setError(hostErrorView(cause, t));
    }
  }, [t]);

  useEffect(() => {
    mounted.current = true;
    const refreshWhenVisible = () => {
      if (!document.hidden) void refresh();
    };
    void refresh();
    const timer = setInterval(refreshWhenVisible, 2_000);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    window.addEventListener("focus", refreshWhenVisible);
    return () => {
      mounted.current = false;
      refreshRevision.current += 1;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
      window.removeEventListener("focus", refreshWhenVisible);
    };
  }, [refresh]);

  const dismissTerminationNotice = useCallback(
    () => setTerminationNotice(null),
    [],
  );

  return {
    banner,
    sessions,
    inputRequests,
    terminationNotice,
    dismissTerminationNotice,
    error,
    ready,
    inputPermission,
    screenPermission,
    platform,
    controlPort,
    lanIp,
    lastUpdated,
    refresh,
  };
}
