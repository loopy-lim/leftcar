import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import type { SessionRow } from "../sessionTypes";
import { hostErrorView } from "./useHostStatus";
export type SessionActionKind = "input" | "quality" | "stop";
export function useSessionActions(
  t: TranslationSchema,
  refresh: () => Promise<void>,
) {
  const [inputActionError, setInputActionError] = useState<string | null>(null);
  const [inputBusy, setInputBusy] = useState<number | "permission" | null>(
    null,
  );
  const [qualityBusy, setQualityBusy] = useState<number | null>(null);
  const [actionsBusy, setActionsBusy] = useState<
    Record<number, SessionActionKind>
  >({});
  const [actionErrors, setActionErrors] = useState<Record<number, string>>({});
  const [stopError, setStopError] = useState<string | null>(null);
  const [pendingStopSession, setPendingStopSession] =
    useState<SessionRow | null>(null);
  const running = useRef(new Set<number>());
  const retries = useRef(new Map<number, () => Promise<void>>());
  const permissionRunning = useRef(false);
  const openAccessibilitySettings = async () => {
    try {
      await invoke("open_system_settings", { pane: "accessibility" });
    } catch (cause) {
      setInputActionError(hostErrorView(cause, t).message);
    }
  };
  const requestInputPermission = async () => {
    if (permissionRunning.current) return;
    permissionRunning.current = true;
    setInputBusy("permission");
    try {
      const granted = await invoke<boolean>("request_input_permission");
      if (!granted) {
        await invoke("open_system_settings", { pane: "accessibility" }).catch(
          () => {},
        );
        setInputActionError(t.host.permissionErrorGuide);
      } else setInputActionError(null);
      await refresh();
    } catch (cause) {
      setInputActionError(hostErrorView(cause, t).message);
    } finally {
      permissionRunning.current = false;
      setInputBusy(null);
    }
  };
  const runSessionAction = async (
    sessionId: number,
    kind: SessionActionKind,
    action: () => Promise<void>,
  ) => {
    if (running.current.has(sessionId)) return;
    running.current.add(sessionId);
    setActionsBusy((previous) => ({ ...previous, [sessionId]: kind }));
    setActionErrors((previous) => {
      const next = { ...previous };
      delete next[sessionId];
      return next;
    });
    if (kind === "input") setInputBusy(sessionId);
    if (kind === "quality") setQualityBusy(sessionId);
    if (kind === "stop") setStopError(null);
    try {
      await action();
      retries.current.delete(sessionId);
      await refresh();
      if (kind === "stop") setPendingStopSession(null);
    } catch (cause) {
      const detail = String(cause instanceof Error ? cause.message : cause);
      const message = `${{ input: t.host.inputFailed, quality: t.host.qualityFailed, stop: t.host.stopFailed }[kind]} ${detail}`;
      if (kind === "stop") setStopError(message);
      else {
        setActionErrors((previous) => ({ ...previous, [sessionId]: message }));
        retries.current.set(sessionId, () =>
          runSessionAction(sessionId, kind, action),
        );
      }
    } finally {
      running.current.delete(sessionId);
      setActionsBusy((previous) => {
        const next = { ...previous };
        delete next[sessionId];
        return next;
      });
      if (kind === "input")
        setInputBusy((previous) => (previous === sessionId ? null : previous));
      if (kind === "quality")
        setQualityBusy((previous) =>
          previous === sessionId ? null : previous,
        );
    }
  };
  const toggleSessionInput = (session: SessionRow) =>
    runSessionAction(session.session, "input", () =>
      invoke("set_session_input", {
        session: session.session,
        enabled: !session.inputEnabled,
      }),
    );
  const setSessionQuality = (session: SessionRow, quality: number | null) =>
    runSessionAction(session.session, "quality", () =>
      invoke("set_session_quality", { session: session.session, quality }),
    );
  const forceStopSession = (session: SessionRow) =>
    runSessionAction(session.session, "stop", () =>
      invoke("force_stop_session", { session: session.session }),
    );
  const selectStopSession = (session: SessionRow | null) => {
    setStopError(null);
    setPendingStopSession(session);
  };
  const retrySessionAction = (sessionId: number) => {
    void retries.current.get(sessionId)?.();
  };
  return {
    inputActionError,
    setInputActionError,
    inputBusy,
    qualityBusy,
    actionsBusy,
    actionErrors,
    retrySessionAction,
    pendingStopSession,
    setPendingStopSession: selectStopSession,
    stopError,
    openAccessibilitySettings,
    requestInputPermission,
    toggleSessionInput,
    setSessionQuality,
    forceStopSession,
  };
}
