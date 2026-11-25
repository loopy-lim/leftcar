import { Scroll, View, signal, type Accessor } from "nsh";
import { t } from "../i18n";
import { Action } from "../ui/Action";
import { Label } from "../ui/Label";
import { Notice } from "../ui/Notice";
import { Spinner } from "../ui/Spinner";
import { Surface } from "../ui/Surface";
import type { ActiveStream, CatalogModel, DisplayInfo } from "./model";
import { createCatalogModel } from "./model";
import { resolveCatalogSource } from "./source";
import { SettingsSheet } from "./SettingsSheet";

export function CatalogScreen() {
  const model = createCatalogModel(resolveCatalogSource());
  const settingsOpen = signal(false);
  const displays = () => model.snapshot()?.displays ?? [];
  return (
    <View class="flex-1 bg-canvas">
      <Scroll
        class="flex-1"
        contentClass="mx-auto w-full max-w-3xl gap-5 px-4 pb-8 pt-4"
        refreshing={model.loading() && model.snapshot() !== null}
        onRefresh={() => void model.refresh()}
      >
        <CatalogListHeader model={model} onOpenSettings={() => settingsOpen.set(true)} />
        <View class="flex-row items-center justify-between gap-2">
          <Label variant="title" class="flex-1">
            {t().viewer.displaysSectionTitle}
          </Label>
          <Action
            variant="ghost"
            size="compact"
            busy={model.loading()}
            disabled={model.openingOrStopping()}
            onPress={() => void model.refresh()}
            label={t().common.refresh}
            testID="refresh"
          />
        </View>
        <If when={displays().length === 0}>
          <DisplayListEmpty model={model} />
        </If>
        <Each of={displays()} key={(display) => display.index}>
          {(display) => <DisplayRow display={display} model={model} />}
        </Each>
      </Scroll>
      <SettingsSheet open={settingsOpen()} onOpenChange={settingsOpen.set} />
    </View>
  );
}

function CatalogListHeader(props: { model: CatalogModel; onOpenSettings: () => void }) {
  return (
    <View class="gap-5">
      <CatalogHeader model={props.model} onOpenSettings={props.onOpenSettings} />
      <CatalogErrorNotice model={props.model} />
      <ActiveStreams model={props.model} />
    </View>
  );
}

function CatalogHeader(props: { model: CatalogModel; onOpenSettings: () => void }) {
  const connected = () => props.model.hostAddress !== null;
  return (
    <View class="gap-3">
      <View class="min-w-0 gap-0.5">
        <View class="flex-row items-center gap-2">
          <View
            class={`h-2 w-2 rounded-full ${connected() ? "bg-live" : "bg-strong"}`}
            accessibilityElementsHidden
            importantForAccessibility="no-hide-descendants"
          />
          <Label variant="caption" tone="muted">
            {connected() ? t().viewer.connectedHostLabel.replace(/:\s*$/, "") : t().viewer.standbyBadge}
          </Label>
        </View>
        <Label variant="code" class="select-text">
          {props.model.hostAddress || "—"}
        </Label>
      </View>
      <View class="flex-row flex-wrap gap-2">
        <Action
          variant="secondary"
          class="grow"
          onPress={props.onOpenSettings}
          label={t().viewer.btnSettings}
          testID="settings"
        />
      </View>
    </View>
  );
}

function CatalogErrorNotice(props: { model: CatalogModel }) {
  return (
    <If when={props.model.error()}>
      {(message) => (
        <Notice tone="error" testID="catalog-error">
          <Label>{message()}</Label>
          <Action
            variant="secondary"
            busy={props.model.loading()}
            disabled={props.model.openingOrStopping()}
            onPress={() => void props.model.refresh()}
            label={t().common.retry}
            testID="error-retry"
          />
        </Notice>
      )}
    </If>
  );
}

function ActiveStreams(props: { model: CatalogModel }) {
  const streams = () => props.model.snapshot()?.streams ?? [];
  return (
    <If when={streams().length > 0}>
      <View class="gap-3" testID="active-streams">
        <Label variant="title">{t().viewer.activeStreamsSection}</Label>
        <Each of={streams()} key={(stream) => stream.session}>
          {(stream) => <ActiveStreamCard stream={stream()} model={props.model} />}
        </Each>
      </View>
    </If>
  );
}

function transportLabel(transport: ActiveStream["transport"]): string {
  if (transport === "usb") return "USB";
  if (transport === "tcp") return "Wi-Fi (TCP)";
  return "Wi-Fi";
}

function ActiveStreamCard(props: { stream: ActiveStream; model: CatalogModel }) {
  const session = props.stream.session;
  const stopping = () => props.model.stoppingSession() === session;
  const failed = () => props.model.stopFailed().includes(session);
  return (
    <Surface variant="card" testID={`stream-${session}`}>
      <Label variant="title">{props.stream.sourceName}</Label>
      <Label variant="code" tone="muted">
        {props.stream.width} × {props.stream.height} · {props.stream.fps} FPS · {transportLabel(props.stream.transport)}
      </Label>
      <Action
        variant="danger"
        busy={stopping()}
        disabled={props.model.openingOrStopping()}
        onPress={() => void props.model.stop(session)}
        label={t().common.stop}
        testID={`stop-${session}`}
      />
      <If when={failed()}>
        <Notice tone="error">
          <Label>{t().viewer.streamStopFailed}</Label>
          <Action
            variant="secondary"
            busy={stopping()}
            disabled={props.model.openingOrStopping()}
            onPress={() => void props.model.stop(session)}
            label={t().common.retry}
            testID={`stop-retry-${session}`}
          />
        </Notice>
      </If>
    </Surface>
  );
}

function DisplayListEmpty(props: { model: CatalogModel }) {
  const loading = () => props.model.loading();
  const empty = () => !loading() && props.model.error() === null;
  return (
    <If when={loading() || empty()}>
      <Surface variant="card" class="py-6" testID="displays-empty">
        <If when={loading()}>
          <Spinner />
          <Label android={{ accessibilityLiveRegion: "polite" }}>{t().viewer.searchingDisplays}</Label>
        </If>
        <If when={empty()}>
          <Label tone="muted">{t().viewer.emptyDisplays}</Label>
          <Action
            variant="secondary"
            disabled={props.model.openingOrStopping()}
            onPress={() => void props.model.refresh()}
            label={t().common.retry}
            testID="retry"
          />
        </If>
      </Surface>
    </If>
  );
}

function DisplayRow(props: { display: Accessor<DisplayInfo>; model: CatalogModel }) {
  const display = props.display;
  const active = () =>
    (props.model.snapshot()?.streams ?? []).some((stream) => stream.sourceIndex === display().index);
  const launching = () => props.model.launchingIndex() === display().index;
  const primary = () => display().index === 0 && (props.model.snapshot()?.streams ?? []).length === 0;
  return (
    <Surface variant="card" testID={`display-${display().index}`}>
      <View class="flex-row items-center gap-3">
        <DisplayMiniature display={display} />
        <View class="min-w-0 flex-1 gap-1">
          <Label variant="title">{display().name}</Label>
          <Label variant="code" tone="muted">
            {display().width} × {display().height} · {props.model.snapshot()?.fps} FPS
          </Label>
          <Label variant="caption" tone="muted">
            #{display().index}{" "}
            {display().index === 0 ? t().viewer.primaryDisplay : t().viewer.secondaryDisplay}
          </Label>
        </View>
      </View>
      <If when={active()}>
        <Label tone="muted" android={{ accessibilityLiveRegion: "polite" }}>
          {t().viewer.currentlyStreaming}
        </Label>
      </If>
      <If when={!active()}>
        <Action
          variant={primary() ? "primary" : "secondary"}
          busy={launching()}
          disabled={props.model.openingOrStopping()}
          accessibilityLabel={`${t().common.open}: ${display().name}`}
          label={launching() ? t().viewer.openingScreen : t().common.open}
          onPress={() => void props.model.open(display())}
          testID={`open-${display().index}`}
        />
      </If>
    </Surface>
  );
}

function DisplayMiniature(props: { display: Accessor<DisplayInfo> }) {
  const box = () => {
    const ratio = Math.max(0.35, Math.min(3, props.display().width / Math.max(1, props.display().height)));
    const width = Math.min(48, 32 * ratio);
    return { width, height: width / ratio };
  };
  return (
    <View
      class="h-14 w-14 items-center justify-center"
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
    >
      <View class="rounded border border-strong bg-subtle" style={box()} />
      <View class="mt-1 h-0.5 w-4 rounded-full bg-strong" />
    </View>
  );
}
