import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import type { SessionRow } from "../sessionTypes";
import { hostErrorView } from "./useHostStatus";

export function useSessionActions(t: TranslationSchema, refresh: () => Promise<void>) {
  const [inputActionError, setInputActionError] = useState<string | null>(null);
  const [inputBusy, setInputBusy] = useState<number | "permission" | null>(null);
  const [qualityBusy, setQualityBusy] = useState<number | null>(null);
  const [pendingStopSession, setPendingStopSession] = useState<SessionRow | null>(null);

  const openAccessibilitySettings = async () => {
    try {
      await invoke("open_system_settings", { pane: "accessibility" });
    } catch (cause) {
      setInputActionError(hostErrorView(cause, t).message);
    }
  };

  const requestInputPermission = async () => {
    setInputBusy("permission");
    try {
      const granted = await invoke<boolean>("request_input_permission");
      if (!granted) {
        await invoke("open_system_settings", { pane: "accessibility" }).catch(() => {});
        setInputActionError(t.host.permissionErrorGuide);
      } else {
        setInputActionError(null);
      }
      await refresh();
    } catch (cause) {
      setInputActionError(hostErrorView(cause, t).message);
    } finally {
      setInputBusy(null);
    }
  };

  const runSessionAction = async <B extends number | "permission">(
    setBusy: (value: B | null) => void,
    busyValue: B,
    action: () => Promise<void>,
    onSuccess?: () => void,
  ): Promise<void> => {
    setBusy(busyValue);
    try {
      await action();
      setInputActionError(null);
      await refresh();
      onSuccess?.();
    } catch (cause) {
      setInputActionError(hostErrorView(cause, t).message);
    } finally {
      setBusy(null);
    }
  };

  const toggleSessionInput = (session: SessionRow) =>
    runSessionAction(setInputBusy, session.session, () =>
      invoke("set_session_input", {
        session: session.session,
        enabled: !session.inputEnabled,
      }),
    );

  const setSessionQuality = (session: SessionRow, quality: number | null) =>
    runSessionAction(
      setQualityBusy,
      session.session,
      () => invoke("set_session_quality", { session: session.session, quality }),
    );

  const forceStopSession = (session: SessionRow) =>
    runSessionAction(
      setInputBusy,
      session.session,
      () => invoke("force_stop_session", { session: session.session }),
      () => setPendingStopSession(null),
    );

  return {
    inputActionError,
    setInputActionError,
    inputBusy,
    qualityBusy,
    pendingStopSession,
    setPendingStopSession,
    openAccessibilitySettings,
    requestInputPermission,
    toggleSessionInput,
    setSessionQuality,
    forceStopSession,
  };
}
