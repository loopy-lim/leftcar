import { useCallback, useEffect, useReducer, useRef } from "react";
import { AppState } from "react-native";
import { router, useFocusEffect } from "expo-router";
import { KmError, kmReasonIssue, type KmFailure, type KmIssue, type KmNativeStatus, type KmStartOptions } from "./km-session";
import { controlTarget } from "./session";
import { kmNative, kmSession, subscribeKmStatus } from "./km.native";

export type KmActionKind = "start" | "stop" | "shizuku" | "accessibility";
interface PanelState {
  status: KmNativeStatus | null;
  options: KmStartOptions;
  busy: boolean;
  pendingAction: KmActionKind | null;
  error: KmIssue | "refreshFailed" | null;
  failure: KmFailure | null;
}
function reducePanel(state: PanelState, patch: Partial<PanelState>): PanelState { return { ...state, ...patch }; }
export function useKmModel() {
  const [state, update] = useReducer(reducePanel, {
    status: null,
    options: kmSession?.getOptions() ?? { arrangementSide: "right", macToAndroid: false, androidToMac: false },
    busy: false,
    pendingAction: null,
    error: null,
    failure: null,
  });
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
  const action = async (kind: KmActionKind, operation: () => Promise<unknown>) => {
    if (running.current) return;
    running.current = true; update({ busy: true, pendingAction: kind, error: null, failure: null });
    try { await operation(); await refresh(); }
    catch (cause) {
      if (mounted.current && !(cause instanceof Error && cause.name === "AbortError")) update({ error: cause instanceof KmError ? cause.issue : "startFailed",
        failure: cause instanceof KmError ? { issue: cause.issue, stage: cause.stage } : null });
    } finally { running.current = false; if (mounted.current) update({ busy: false, pendingAction: null }); }
  };
  return {
    ...state, available: !!kmNative, peerHost: kmSession?.getPeerHost() ?? controlTarget()?.host ?? null,
    setOptions: (options: KmStartOptions) => { kmSession?.setOptions(options); update({ options }); },
    refresh: () => void refresh(),
    start: () => void action("start", async () => { if (kmSession) await kmSession.start(state.options); }),
    cancelStart: () => kmSession?.cancelPending(),
    stop: () => void action("stop", async () => { if (kmSession) await kmSession.stop(); }),
    requestShizuku: () => void action("shizuku", async () => { await kmNative?.requestShizukuPermission(); }),
    openAccessibility: () => void action("accessibility", async () => { await kmNative?.openAccessibilitySettings(); }),
    openHostPicker: () => router.push("/host"),
  };
}
