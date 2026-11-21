import { useCallback, useEffect, useReducer, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import { Button, Notice, Surface, Text, Toggle } from "../ui/primitives";
import { loadKmDevice, saveKmDeviceConsent, type KmDeviceConsent, type KmDeviceStatus } from "./km-consent";

interface KmPanelState {
  consent: KmDeviceConsent | null;
  status: KmDeviceStatus | null;
  busy: boolean;
  error?: "refreshFailed" | "saveFailed" | null;
  loadError?: "refreshFailed" | null;
  actionError?: "refreshFailed" | "saveFailed" | null;
}
function reducePanel(state: KmPanelState, patch: Partial<KmPanelState>) { return { ...state, ...patch }; }
interface KmConsentViewProps extends KmPanelState {
  t: TranslationSchema;
  onToggle(direction: keyof KmDeviceConsent): void;
  onRefresh(): void;
  onSettings(pane: "inputMonitoring" | "accessibility"): void;
}
function hostStateLabel(status: KmDeviceStatus | null, t: TranslationSchema): string {
  if (!status) return t.km.loading;
  if (!status.armed) return t.km.off;
  const labels = { local: t.km.armed, macToAndroid: t.km.activeMacToAndroid, androidToMac: t.km.activeAndroidToMac };
  return labels[status.owner];
}
function DirectionApprovals({ t, consent, busy, onToggle }: Pick<KmConsentViewProps, "t" | "consent" | "busy" | "onToggle">) {
  return <>
    <Text variant="caption" tone="muted" className="block">{t.km.bothRequired}</Text>
    {(["macToAndroid", "androidToMac"] as const).map(direction => <div key={direction} className="flex items-center justify-between gap-3">
      <Text>{t.km[direction]}</Text>
      <Toggle checked={consent?.[direction] ?? false} disabled={!consent || busy} aria-label={t.km[direction]} onClick={() => onToggle(direction)} />
    </div>)}
  </>;
}
interface MacPermissionProps {
  granted: boolean;
  label: string;
  settingsLabel: string;
  pane: "inputMonitoring" | "accessibility";
  t: TranslationSchema;
  onSettings: KmConsentViewProps["onSettings"];
}
function MacPermission({ granted, label, settingsLabel, pane, t, onSettings }: MacPermissionProps) {
  return <div className="flex flex-wrap items-center justify-between gap-2">
    <Text variant="caption">{label}: {granted ? t.km.ready : t.km.required}</Text>
    {!granted && <Button variant="secondary" size="compact" onClick={() => onSettings(pane)}>{settingsLabel}</Button>}
  </div>;
}
function MacPermissions({ t, status, onSettings }: Pick<KmConsentViewProps, "t" | "status" | "onSettings">) {
  if (!status) return null;
  return <div className="space-y-2">
    <MacPermission t={t} granted={status.capabilities.macCapture} label={t.km.macCapture}
      settingsLabel={t.km.openInputMonitoring} pane="inputMonitoring" onSettings={onSettings} />
    <MacPermission t={t} granted={status.capabilities.macInjection} label={t.km.macInjection}
      settingsLabel={t.km.openMacAccessibility} pane="accessibility" onSettings={onSettings} />
  </div>;
}
export function KmConsentView({ t, consent, status, busy, error, loadError, actionError, onToggle, onRefresh, onSettings }: KmConsentViewProps) {
  const visibleError = error ?? actionError ?? loadError ?? null;
  return <Surface variant="inset" className="w-full space-y-3">
    <div className="flex flex-wrap items-center justify-between gap-2">
      <Text className="font-semibold">{t.km.hostConsent}</Text>
      <Button variant="ghost" size="compact" disabled={busy} onClick={onRefresh}>{t.common.refresh}</Button>
    </div>
    <Text role="status" className="block">{hostStateLabel(status, t)}</Text>
    <DirectionApprovals t={t} consent={consent} busy={busy} onToggle={onToggle} />
    {visibleError && <Notice tone="error">{t.km[visibleError]}</Notice>}
    {status?.blockedReason && <Notice>{t.km.hostBlocked}</Notice>}
    <MacPermissions t={t} status={status} onSettings={onSettings} />
    <Text variant="caption" tone="muted" className="block">{t.km.emergency}</Text>
  </Surface>;
}
export default function KmConsentPanel({ deviceId, t }: { deviceId: string; t: TranslationSchema }) {
  const [state, update] = useReducer(reducePanel, { consent: null, status: null, busy: false, loadError: null, actionError: null });
  const mounted = useRef(false);
  const running = useRef(false);
  const revision = useRef(0);
  const refresh = useCallback(async () => {
    const requestRevision = ++revision.current;
    try {
      const data = await loadKmDevice(deviceId);
      if (mounted.current && requestRevision === revision.current) update({ ...data, loadError: null });
    } catch { if (mounted.current && requestRevision === revision.current) update({ consent: null, status: null, loadError: "refreshFailed" }); }
  }, [deviceId]);
  useEffect(() => {
    mounted.current = true;
    void refresh();
    const timer = window.setInterval(() => { if (!running.current) void refresh(); }, 1000);
    return () => { mounted.current = false; revision.current += 1; window.clearInterval(timer); };
  }, [refresh]);
  const toggle = async (direction: keyof KmDeviceConsent) => {
    if (!state.consent || running.current) return;
    running.current = true; revision.current += 1; update({ busy: true, actionError: null });
    try {
      const consent = await saveKmDeviceConsent(deviceId, { ...state.consent, [direction]: !state.consent[direction] });
      if (mounted.current) update({ consent });
      await refresh();
    } catch { if (mounted.current) update({ actionError: "saveFailed" }); }
    finally { running.current = false; if (mounted.current) update({ busy: false }); }
  };
  const settings = async (pane: "inputMonitoring" | "accessibility") => {
    try { await invoke("open_system_settings", { pane }); }
    catch { if (mounted.current) update({ actionError: "refreshFailed" }); }
  };
  const manualRefresh = () => {
    update({ actionError: null });
    void refresh();
  };
  return <KmConsentView {...state} t={t} onToggle={direction => void toggle(direction)} onRefresh={manualRefresh} onSettings={pane => void settings(pane)} />;
}
