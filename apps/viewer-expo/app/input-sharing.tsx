import { ScrollView, View } from "react-native";
import { useAppLanguage, type TranslationSchema } from "../src/i18n";
import { kmReasonIssue, type KmNativeStatus } from "../src/km-session";
import { useKmModel } from "../src/use-km-model";
import { Action, Label, Notice, SafeArea, Surface } from "../src/ui/primitives";

type KmModel = ReturnType<typeof useKmModel>;
interface KmPanelProps { t: TranslationSchema; model: KmModel }
interface KmControlsProps extends KmPanelProps { active: boolean }

function nativeStateLabel(status: KmNativeStatus | null, t: TranslationSchema): string {
  if (!status) return t.km.loading;
  switch (status.state) {
    case "cleanupFailed": return t.km.cleanupUnconfirmed;
    case "macToAndroid": return t.km.activeMacToAndroid;
    case "androidToMac": return t.km.activeAndroidToMac;
    default: return t.km[status.state];
  }
}
function permissionLabel(granted: boolean | null, t: TranslationSchema): string {
  if (granted === null) return t.km.loading;
  return granted ? t.km.ready : t.km.required;
}
function SharingFailure({ t, model }: KmPanelProps) {
  if (model.status?.state === "cleanupFailed" || model.error === "stopFailed") return <Notice tone="error">
    <Label>{t.km.stopFailed}</Label><Label>{t.km.cleanupRecovery}</Label>
  </Notice>;
  const issue = model.error ?? (model.status?.reason ? kmReasonIssue(model.status.reason) ?? "nativeStopped" : null);
  if (!issue) return null;
  const stage = model.failure?.stage;
  const needsHost =
    issue === "connectionRequired" ||
    issue === "wifiRequired" ||
    issue === "pairingRequired";
  return <Notice tone="error">
    {stage && <Label>{t.km[stage]}</Label>}
    <Label>{t.km[issue]}</Label>
    {stage && issue === "startFailed" && <Label>{t.km[`${stage}Recovery`]}</Label>}
    {needsHost && typeof model.openHostPicker === "function" && (
      <Action variant="secondary" label={t.viewer.btnFindHost} onPress={model.openHostPicker} />
    )}
  </Notice>;
}
function SharingStatus({ t, model }: KmPanelProps) {
  return <>
    <Label variant="title">{t.km.title}</Label>
    <Label tone="muted">{t.km.description}</Label>
    {model.peerHost && <Label variant="code" tone="muted">{model.peerHost}</Label>}
    <Label accessibilityLiveRegion="polite">{nativeStateLabel(model.status, t)}</Label>
    {!model.available && <Notice tone="error"><Label>{t.km.nativeUnavailable}</Label></Notice>}
    <SharingFailure t={t} model={model} />
  </>;
}
function DirectionConsent({ t, model, active }: KmControlsProps) {
  const { options, busy } = model;
  return <>
    <Label tone="muted">{t.km.bothRequired}</Label>
    <View className="gap-2">
      {(["macToAndroid", "androidToMac"] as const).map(direction => (
        <Action key={direction} variant={options[direction] ? "secondary" : "ghost"}
          label={`${t.km[direction]} · ${options[direction] ? t.km.consentOn : t.km.consentOff}`} accessibilityRole="switch"
          accessibilityState={{ checked: options[direction], disabled: busy }} disabled={busy || !model.available}
          onPress={() => {
            const next = { ...options, [direction]: !options[direction] };
            model.setOptions(next);
            if (active && !next[direction]) model.stop();
          }} />
      ))}
    </View>
  </>;
}
function ArrangementControls({ t, model, active }: KmControlsProps) {
  const { options, busy } = model;
  return <>
    <Label>{t.km.arrangement}</Label>
    <View className="flex-row gap-2">
      {(["left", "right"] as const).map(side => {
        const selected = options.arrangementSide === side;
        return <Action key={side} variant={selected ? "secondary" : "ghost"}
          label={`${selected ? "✓ " : ""}${t.km[side]}`}
          accessibilityRole="radio" accessibilityState={{ checked: selected }} disabled={busy || active}
          className="flex-1" onPress={() => model.setOptions({ ...options, arrangementSide: side })} />;
      })}
    </View>
  </>;
}
function sessionActionState(model: KmModel, active: boolean) {
  const { status, options, busy, pendingAction } = model;
  const starting = pendingAction !== undefined ? pendingAction === "start" : busy;
  const stopping = pendingAction !== undefined ? pendingAction === "stop" : busy;
  const permissionReady =
    Boolean(status?.shizukuRunning) &&
    Boolean(status?.shizukuGranted) &&
    Boolean(status?.accessibilityEnabled);
  const startBlocked =
    !model.available || !permissionReady || !options.macToAndroid || !options.androidToMac;
  const isStopMode =
    active || status?.state === "cleanupFailed" || model.error === "stopFailed";
  return { starting, stopping, startBlocked, isStopMode, busy };
}

function SessionActions({ t, model, active }: KmControlsProps) {
  const { starting, stopping, startBlocked, isStopMode, busy } =
    sessionActionState(model, active);
  return <>
    {isStopMode ? (
      <Action label={t.km.stop} onPress={model.stop} busy={stopping} disabled={busy} />
    ) : (
      <Action
        label={starting ? t.km.startCheck : t.km.start}
        onPress={model.start}
        busy={starting}
        disabled={busy || startBlocked}
      />
    )}
    {starting && !active && <Action variant="ghost" label={t.common.cancel} onPress={model.cancelStart} />}
    {!active && !starting && <Label variant="caption" tone="muted">{t.km.unverified}</Label>}
  </>;
}
function ShizukuPermission({ t, model }: KmPanelProps) {
  const { status, busy, pendingAction } = model;
  const granted = status ? status.shizukuRunning && status.shizukuGranted : null;
  return <>
    <Label>{t.km.shizuku}: {permissionLabel(granted, t)}</Label>
    {status && !status.shizukuRunning && <Label tone="muted">{t.km.shizukuSetup}</Label>}
    {status?.shizukuRunning && !status.shizukuGranted && <Action variant="secondary" label={t.km.requestShizuku} onPress={model.requestShizuku} busy={pendingAction === "shizuku"} disabled={busy} />}
  </>;
}
function AndroidPermissions({ t, model }: KmPanelProps) {
  const accessibilityGranted = model.status ? model.status.accessibilityEnabled : null;
  return <Surface className="gap-3">
    <ShizukuPermission t={t} model={model} />
    <Label>{t.km.accessibility}: {permissionLabel(accessibilityGranted, t)}</Label>
    <Action variant="secondary" label={t.km.openAccessibility} onPress={model.openAccessibility} busy={model.pendingAction === "accessibility"} disabled={model.busy || !model.available} />
    <Action variant="ghost" label={t.common.refresh} onPress={model.refresh} disabled={model.busy || !model.available} />
    <Label variant="caption" tone="muted">{t.km.emergency}</Label>
  </Surface>;
}
export default function InputSharingScreen() {
  const { t } = useAppLanguage();
  const model = useKmModel();
  const active = !!model.status && ["armed", "macToAndroid", "androidToMac"].includes(model.status.state);
  return <SafeArea className="flex-1 bg-canvas" edges={["left", "right", "bottom"]}>
    <ScrollView contentContainerClassName="mx-auto w-full max-w-2xl gap-4 px-5 py-4">
      <Surface className="gap-3">
        <SharingStatus t={t} model={model} />
        <DirectionConsent t={t} model={model} active={active} />
        <ArrangementControls t={t} model={model} active={active} />
        <SessionActions t={t} model={model} active={active} />
      </Surface>
      <AndroidPermissions t={t} model={model} />
    </ScrollView>
  </SafeArea>;
}
