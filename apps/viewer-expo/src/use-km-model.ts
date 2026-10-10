import { useCallback, useEffect, useReducer, useRef } from "react";
import { AppState } from "react-native";
import { useFocusEffect } from "expo-router";
import { KmError, kmReasonIssue, type KmFailure, type KmIssue, type KmNativeStatus, type KmStartOptions } from "./km-session";
import { controlTarget } from "./session";
import { kmNative, kmSession, subscribeKmStatus } from "./km.native";

interface PanelState { status: KmNativeStatus | null; options: KmStartOptions; busy: boolean; error: KmIssue | "refreshFailed" | null; failure: KmFailure | null }
function reducePanel(state: PanelState, patch: Partial<PanelState>): PanelState { return { ...state, ...patch }; }
export function useKmModel() {
  const [state, update] = useReducer(reducePanel, { status: null, options: kmSession?.getOptions() ?? { arrangementSide: "right", macToAndroid: false, androidToMac: false }, busy: false, error: null, failure: null });
  const mounted = useRef(false);
  const running = useRef(false);
  const refresh = useCallback(async () => {
    if (!kmNative) return;
    try {
      const status = await kmNative.getStatus();
      if (mounted.current) update({ status, error: null, failure: null });
      await kmSession?.reconcile(status);
    } catch { if (mounted.current) update({ error: "refreshFailed" }); }
  }, []);
  useEffect(() => {
    mounted.current = true;
    const unsubscribe = subscribeKmStatus(status => {
      // Startup failure presentation belongs to its promise. Only a live session
      // can produce an ordinary automatic-stop notice; retired startup events cannot.
      const visibleStatus = status.state === "off" && !kmSession?.getPeerHost() ? { ...status, reason: null } : status;
      if (mounted.current) {
        if (visibleStatus.state === "cleanupFailed") update({ status: visibleStatus, error: "stopFailed", failure: null });
        else if (visibleStatus.state === "off" && visibleStatus.reason) update({ status: visibleStatus, error: kmReasonIssue(visibleStatus.reason) ?? "nativeStopped" });
        else update({ status: visibleStatus });
      }
      void kmSession?.reconcile(status).catch(() => { if (mounted.current) update({ error: "stopFailed" }); });
    });
    const lifecycle = AppState.addEventListener("change", next => { if (next === "active") void refresh(); });
    return () => { mounted.current = false; unsubscribe(); lifecycle.remove(); kmSession?.cancelPending(); };
  }, [refresh]);
  useFocusEffect(useCallback(() => { void refresh(); return () => kmSession?.cancelPending(); }, [refresh]));
  const action = async (operation: () => Promise<unknown>) => {
    if (running.current) return;
    running.current = true; update({ busy: true, error: null, failure: null });
    try { await operation(); await refresh(); }
    catch (cause) {
      if (mounted.current && !(cause instanceof Error && cause.name === "AbortError")) update({ error: cause instanceof KmError ? cause.issue : "startFailed",
        failure: cause instanceof KmError ? { issue: cause.issue, stage: cause.stage } : null });
    } finally { running.current = false; if (mounted.current) update({ busy: false }); }
  };
  return {
    ...state, available: !!kmNative, peerHost: kmSession?.getPeerHost() ?? controlTarget()?.host ?? null,
    setOptions: (options: KmStartOptions) => { kmSession?.setOptions(options); update({ options }); },
    refresh: () => void refresh(),
    start: () => void action(async () => { if (kmSession) await kmSession.start(state.options); }),
    cancelStart: () => kmSession?.cancelPending(),
    stop: () => void action(async () => { if (kmSession) await kmSession.stop(); }),
    requestShizuku: () => void action(async () => { await kmNative?.requestShizukuPermission(); }),
    openAccessibility: () => void action(async () => { await kmNative?.openAccessibilitySettings(); }),
  };
}
