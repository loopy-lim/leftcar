import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  ActivityIndicator,
  FlatList,
  Modal,
  RefreshControl,
  ScrollView,
  Switch,
  View,
  type ListRenderItemInfo,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { router } from "expo-router";
import { cn } from "@leftcar/ui-tokens";
import type { DisplayInfo } from "../src/control";
import { captureRequestContext, isRequestContextCurrent } from "../src/session";
import { requestForCurrentSelection } from "../src/catalog-helpers";
import {
  getUsbState,
  subscribeUsbState,
  type UsbAccessoryState,
} from "../src/usb";
import { STREAM_PROFILES } from "../src/stream-profile";
import { UdpStabilityControls } from "../src/UdpStabilityControls";
import { resolveInitialStreamTarget } from "../src/streaming-policy";
import {
  resolveStreamMaximum,
  resolveViewerProfileId,
  type ViewerProfileSelection,
} from "../src/viewer-preferences";
import type { ActiveStream } from "../src/catalog-model-types";
import type { NativeSettingsKey } from "../src/native-settings";
import { useCatalogModel } from "../src/use-catalog-model";
import type { PreferencePersistenceIssue } from "../src/use-catalog-preferences";
import { transportBadgeLabel } from "../src/transport-label";
import { DisplaySizeCard } from "../src/DisplaySizeCard";
import { ExtensionDisplayCard } from "../src/ExtensionDisplayCard";
import { FileTransferCard } from "../src/FileTransferCard";
import { formatErrorMessage } from "../src/control";
import { useAppTheme } from "../src/theme";
import { useAppLanguage } from "../src/i18n";
import { SafeArea, Action, Label, Notice, Surface } from "../src/ui/primitives";

type CatalogModel = ReturnType<typeof useCatalogModel>;
const QUALITY_COPY = {
  auto: ["qualityAutoLabel", "qualityAutoDetail"],
  latency: ["qualityLatencyLabel", "qualityLatencyDetail"],
  video: ["qualityVideoLabel", "qualityVideoDetail"],
  smooth: ["qualitySmoothLabel", "qualitySmoothDetail"],
  balanced: ["qualityBalancedLabel", "qualityBalancedDetail"],
  clarity: ["qualityClarityLabel", "qualityClarityDetail"],
} as const;

function Sheet({
  visible,
  title,
  onClose,
  children,
}: {
  visible: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const insets = useSafeAreaInsets();
  const { t } = useAppLanguage();
  return (
    <Modal
      visible={visible}
      animationType="slide"
      transparent
      onRequestClose={onClose}
    >
      <View className="flex-1 justify-end bg-scrim">
        <Surface
          variant="card"
          className="max-h-[88%] w-full rounded-t-xl bg-canvas"
          accessibilityViewIsModal
          style={{ paddingBottom: Math.max(16, insets.bottom) }}
        >
          <View className="flex-row items-center justify-between gap-3">
            <Label variant="title" className="flex-1">
              {title}
            </Label>
            <Action
              variant="ghost"
              size="icon"
              label="×"
              accessibilityLabel={`${title}: ${t.common.close}`}
              onPress={onClose}
            />
          </View>
          <ScrollView
            contentContainerClassName="gap-5 pb-4"
            keyboardShouldPersistTaps="handled"
          >
            {children}
          </ScrollView>
        </Surface>
      </View>
    </Modal>
  );
}

function Option({
  title,
  hint,
  value,
  disabled,
  onChange,
}: {
  title: string;
  hint?: string;
  value: boolean;
  disabled: boolean;
  onChange: (value: boolean) => void;
}) {
  const { colors } = useAppTheme();
  return (
    <View className="gap-1">
      <Action
        variant="ghost"
        className="justify-between"
        label={title}
        accessibilityRole="switch"
        accessibilityState={{ checked: value, disabled }}
        accessibilityHint={hint}
        disabled={disabled}
        onPress={() => onChange(!value)}
      >
        <Switch
          value={value}
          disabled={disabled}
          onValueChange={onChange}
          pointerEvents="none"
          accessible={false}
          trackColor={{ false: colors.borderCard, true: colors.btnPrimaryBg }}
          thumbColor={colors.btnPrimaryText}
        />
      </Action>
      {hint ? (
        <Label variant="caption" tone="muted" className="px-4">
          {hint}
        </Label>
      ) : null}
    </View>
  );
}

function QualityChoices({ model }: { model: CatalogModel }) {
  const { t } = useAppLanguage();
  return (
    <Surface className="gap-3">
      <Label variant="title">{t.viewer.qualitySettings}</Label>
      {(
        [
          "auto",
          ...STREAM_PROFILES.map((p) => p.id),
        ] as ViewerProfileSelection[]
      ).map((id) => {
        const [label, detail] = QUALITY_COPY[id];
        return (
          <View key={id} className="gap-1">
            <Action
              variant="secondary"
              className={cn(
                "justify-start",
                model.profileId === id && "border-strong bg-active",
              )}
              label={t.viewer[label]}
              disabled={model.viewerPreferenceControlsDisabled}
              accessibilityState={{ selected: model.profileId === id }}
              accessibilityHint={t.viewer[detail]}
              onPress={() => model.handleSelectProfile(id)}
            />
            <Label variant="caption" tone="muted">
              {t.viewer[detail]}
            </Label>
          </View>
        );
      })}
    </Surface>
  );
}

function PersistenceNotice({ model }: { model: CatalogModel }) {
  const { t } = useAppLanguage();
  const messages: Record<PreferencePersistenceIssue, string> = {
    "viewer-load": t.viewer.preferenceLoadError,
    "viewer-save": t.viewer.preferenceSaveError,
    "clipboard-load": t.viewer.clipboardPreferenceLoadError,
    "clipboard-save": t.viewer.clipboardPreferenceSaveError,
    "udp-load": t.viewer.preferenceLoadError,
    "udp-save": t.viewer.preferenceSaveError,
  };
  if (model.preferencePersistenceIssue)
    return (
      <Notice tone="error">
        <Label>{messages[model.preferencePersistenceIssue]}</Label>
        <Action
          variant="secondary"
          onPress={model.retryPersistence}
          label={t.common.retry}
        />
      </Notice>
    );
  if (!model.preferenceLoading && !model.preferenceSaving) return null;
  return (
    <Notice>
      <Label accessibilityLiveRegion="polite">
        {model.preferenceLoading
          ? t.viewer.preferencesLoading
          : t.viewer.preferencesSaving}
      </Label>
    </Notice>
  );
}
function NativeSettingsNotice({ model }: { model: CatalogModel }) {
  const { t } = useAppLanguage();
  const settingNames: Record<NativeSettingsKey, string> = {
    cursor: t.viewer.cursorOverlayLabel,
    audio: t.viewer.audioToggleLabel,
    opus: t.viewer.opusToggleLabel,
    balanced: t.viewer.balancedPresentationLabel,
    smooth: t.viewer.presentationSmoothLabel,
    udp: t.viewer.udpStabilityTitle,
  };
  return (
    <>
      {model.nativeSettingsApplying ? (
        <Notice>
          <View className="flex-row items-center gap-3">
            <ActivityIndicator accessibilityElementsHidden />
            <Label accessibilityLiveRegion="polite" className="flex-1">
              {t.viewer.settingsApplying}
            </Label>
          </View>
        </Notice>
      ) : null}
      {model.nativeSettingsFailures.map(failure => (
        <Notice key={failure.key} tone="error">
          <Label>{settingNames[failure.key]}: {failure.error}</Label>
          <Label variant="caption" tone="muted">{t.viewer.settingsApplyFailed}</Label>
          <Action
            variant="secondary"
            label={t.common.retry}
            disabled={model.nativeSettingsApplying}
            onPress={() => model.handleRetryNativeSettings(failure.key)}
          />
        </Notice>
      ))}
    </>
  );
}
function RemoteInputPolicy({ allowed }: { allowed?: boolean | null }) {
  const { t } = useAppLanguage();
  if (typeof allowed !== "boolean") return null;
  return (
    <Surface variant="inset">
      <Label>
        {t.viewer.remoteInputLabel}:{" "}
        {allowed
          ? t.viewer.remoteInputAllowedLabel
          : t.viewer.remoteInputBlockedLabel}
      </Label>
      {!allowed ? (
        <Label variant="caption" tone="muted">
          {t.viewer.remoteInputHostHint}
        </Label>
      ) : null}
    </Surface>
  );
}
function EncoderChoices({ model }: { model: CatalogModel }) {
  const { t } = useAppLanguage();
  if (model.selectedEncoderExperiments.length <= 1) return null;
  return (
    <Surface className="gap-3">
      <Label variant="title">{t.viewer.encoderExperiments}</Label>
      <Label tone="muted">{t.viewer.encoderReconnectNotice}</Label>
      {model.selectedEncoderExperiments.map((experiment) => (
        <View key={experiment.id} className="gap-1">
          <Action
            variant="secondary"
            label={experiment.label}
            className={cn(
              model.effectiveNextEncoderExperiment === experiment.id &&
                "border-strong bg-active",
            )}
            accessibilityHint={experiment.hint}
            accessibilityState={{
              selected: model.effectiveNextEncoderExperiment === experiment.id,
            }}
            onPress={() => model.handleSelectEncoderExperiment(experiment.id)}
          />
          <Label variant="caption" tone="muted">
            {experiment.hint}
          </Label>
        </View>
      ))}
    </Surface>
  );
}
function AdvancedOptions({ model }: { model: CatalogModel }) {
  const { t } = useAppLanguage();
  const reconnectRequired = model.udpSettingsDirty && model.streams.length > 0;
  const [manualOpen, setManualOpen] = useState<boolean | null>(null);
  const open = manualOpen ?? reconnectRequired;
  return (
    <Surface className="gap-3">
      <Action
        variant="secondary"
        label={t.viewer.expertSettingsToggle}
        accessibilityState={{ expanded: open }}
        onPress={() => setManualOpen(!open)}
      />
      {open ? (
        <View className="gap-4">
          <Option
            title={t.viewer.opusToggleLabel}
            value={model.opusAudio}
            disabled={model.viewerPreferenceControlsDisabled || model.nativeSettingsApplying}
            onChange={model.handleToggleOpusAudio}
          />
          <Option
            title={t.viewer.balancedPresentationLabel}
            hint={t.viewer.balancedPresentationHint}
            value={model.balancedPresentation}
            disabled={model.viewerPreferenceControlsDisabled || model.nativeSettingsApplying}
            onChange={model.handleToggleBalancedPresentation}
          />
          <Option
            title={t.viewer.presentationSmoothLabel}
            hint={t.viewer.presentationSmoothHint}
            value={model.presentationSmooth}
            disabled={model.viewerPreferenceControlsDisabled || model.nativeSettingsApplying}
            onChange={model.handleTogglePresentationSmooth}
          />
          <EncoderChoices model={model} />
          <UdpStabilityControls
            options={
              model.udpPreferenceControlsDisabled
                ? null
                : model.udpStabilityOptions
            }
            selection={model.effectiveUdpStability}
            reconnectRequired={reconnectRequired}
            reconnecting={model.udpReconnecting}
            disabled={model.nativeSettingsApplying && !model.udpReconnecting}
            onChange={model.handleSelectUdpStability}
            onApplyReconnect={model.handleApplyUdpStability}
          />
        </View>
      ) : null}
    </Surface>
  );
}
function SettingsContent({ model }: { model: CatalogModel }) {
  const { t } = useAppLanguage();
  return (
    <>
      <NativeSettingsNotice model={model} />
      <PersistenceNotice model={model} />
      <Surface className="gap-2">
        <Label variant="title">{t.viewer.sectionViewerOptions}</Label>
        <Option
          title={t.viewer.fpsToggleLabel}
          value={model.showFps}
          disabled={model.viewerPreferenceControlsDisabled}
          onChange={model.handleToggleFps}
        />
        <Option
          title={t.viewer.cursorOverlayLabel}
          hint={t.viewer.cursorOverlayHint}
          value={model.localCursor}
          disabled={model.viewerPreferenceControlsDisabled || model.nativeSettingsApplying}
          onChange={model.handleToggleCursor}
        />
        <Option
          title={t.viewer.audioToggleLabel}
          hint={t.viewer.audioToggleHint}
          value={model.localAudio}
          disabled={model.viewerPreferenceControlsDisabled || model.nativeSettingsApplying}
          onChange={model.handleToggleAudio}
        />
        <Option
          title={t.viewer.clipboardShareLabel}
          hint={t.viewer.clipboardShareHint}
          value={model.clipboardShare}
          disabled={model.clipboardPreferenceControlDisabled}
          onChange={model.handleToggleClipboardShare}
        />
        <RemoteInputPolicy allowed={model.inputAllowed} />
      </Surface>
      <QualityChoices model={model} />
      <AdvancedOptions model={model} />
    </>
  );
}

function UsbTransportStatus() {
  const { t } = useAppLanguage();
  const [state, setState] = useState<UsbAccessoryState>({
    attached: false,
    controlPort: 0,
  });
  const [requesting, setRequesting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const lifetime = useRef({ active: true });
  const pending = useRef(false);
  useEffect(() => {
    const currentLifetime = { active: true };
    lifetime.current = currentLifetime;
    void getUsbState()
      .then((next) => {
        if (currentLifetime.active) setState(next);
      })
      .catch((cause) => {
        if (currentLifetime.active) setError(formatErrorMessage(cause));
      });
    const subscription = subscribeUsbState((next) => {
      if (currentLifetime.active) setState(next);
    });
    return () => {
      currentLifetime.active = false;
      subscription.remove();
    };
  }, []);
  const grant = async () => {
    if (pending.current) return;
    const origin = captureRequestContext();
    const currentLifetime = lifetime.current;
    if (!origin || !isRequestContextCurrent(origin)) {
      setError(t.viewer.notConnectedError);
      return;
    }
    const request = requestForCurrentSelection(
      `${origin.target.host}:${origin.target.port}`,
    );
    const current = () =>
      currentLifetime.active &&
      lifetime.current === currentLifetime &&
      isRequestContextCurrent(origin);
    pending.current = true;
    setRequesting(true);
    setError(null);
    try {
      await request("requestUsb");
    } catch (cause) {
      if (current()) setError(formatErrorMessage(cause));
    } finally {
      pending.current = false;
      if (currentLifetime.active && lifetime.current === currentLifetime)
        setRequesting(false);
    }
  };
  if (
    !state.attached &&
    !state.permissionPending &&
    !state.accessoryPresent &&
    !error
  )
    return null;
  return (
    <Notice>
      <Label>
        {state.attached
          ? t.viewer.usbAttached
          : state.permissionPending
            ? t.viewer.usbPending
            : t.viewer.usbDetected}
      </Label>
      {!state.attached && !state.permissionPending && state.accessoryPresent ? (
        <Action
          variant="secondary"
          busy={requesting}
          onPress={() => void grant()}
          label={t.viewer.usbGrantAction}
        />
      ) : null}
      {error ? <Label accessibilityRole="alert">{error}</Label> : null}
    </Notice>
  );
}

function DisplayMiniature({ display }: { display: DisplayInfo }) {
  const ratio = Math.max(
    0.35,
    Math.min(3, display.width / Math.max(1, display.height)),
  );
  const width = Math.min(48, 32 * ratio);
  const height = width / ratio;
  return (
    <View
      className="h-14 w-14 items-center justify-center"
      importantForAccessibility="no-hide-descendants"
    >
      <View
        className="rounded border border-strong bg-subtle"
        style={{ width, height }}
      />
    </View>
  );
}

function DisplayRow({
  display,
  model,
}: {
  display: DisplayInfo;
  model: CatalogModel;
}) {
  const { t } = useAppLanguage();
  const active = model.streams.find((stream) =>
    display.sourceId && stream.sourceId
      ? display.sourceId === stream.sourceId
      : display.index === stream.sourceIndex,
  );
  const switching = model.switchingSession !== null;
  const launching = model.launchingIndex === display.index;
  const profile =
    STREAM_PROFILES.find(
      (p) => p.id === resolveViewerProfileId(model.profileId, display),
    ) ?? STREAM_PROFILES[0];
  const size = resolveInitialStreamTarget(
    display,
    model.streamingPriority,
    resolveStreamMaximum(display, model.profileId),
    {
      externalRoute: model.externalMediaRoute,
      panelShortSide: model.panelShortSide,
    },
  );
  return (
    <Surface variant="card" className="gap-3">
      <View className="flex-row items-center gap-3">
        <DisplayMiniature display={display} />
        <View className="min-w-0 flex-1 gap-1">
          <Label variant="title">{display.name}</Label>
          <Label variant="code" tone="muted">
            {size.width} × {size.height} · {profile.fps} FPS
          </Label>
          <Label variant="caption" tone="muted">
            #{display.index}{" "}
            {display.index === 0
              ? t.viewer.primaryDisplay
              : t.viewer.secondaryDisplay}
          </Label>
        </View>
      </View>
      {active ? (
        <Label tone="muted">{t.viewer.currentlyStreaming}</Label>
      ) : (
        <Action
          variant="secondary"
          busy={launching}
          disabled={model.launchingIndex !== null || switching}
          accessibilityLabel={`${t.common.open}: ${display.name}`}
          label={launching ? t.viewer.openingScreen : t.common.open}
          onPress={() => void model.openDisplay(display)}
        />
      )}
    </Surface>
  );
}

function ActiveStreamCard({
  stream,
  model,
  onResolution,
}: {
  stream: ActiveStream;
  model: CatalogModel;
  onResolution: () => void;
}) {
  const { t } = useAppLanguage();
  const [stopping, setStopping] = useState(false);
  const [stopError, setStopError] = useState(false);
  const stop = async () => {
    if (stopping) return;
    setStopping(true);
    setStopError(false);
    try {
      if (!(await model.stopStream(stream))) setStopError(true);
    } finally {
      setStopping(false);
    }
  };
  return (
    <Surface variant="card" className="gap-3">
      <Label variant="title">{stream.sourceName}</Label>
      <Label variant="code" tone="muted">
        {stream.activeTarget.width} × {stream.activeTarget.height} ·{" "}
        {stream.activeTarget.fps} FPS ·{" "}
        {transportBadgeLabel(stream.mediaTransport)}
      </Label>
      <View className="flex-row flex-wrap gap-2">
        <Action
          variant="secondary"
          className="grow"
          onPress={onResolution}
          label={t.viewer.changeResolution}
        />
        <Action
          variant="danger"
          className="grow"
          busy={stopping}
          onPress={() => void stop()}
          label={t.common.stop}
        />
      </View>
      {stopError ? (
        <Notice tone="error">
          <Label>{t.viewer.streamStopFailed}</Label>
          <Action
            variant="secondary"
            onPress={() => void stop()}
            label={t.common.retry}
          />
        </Notice>
      ) : null}
      {model.displays.length > 1 ? (
        <View className="gap-2">
          <Label variant="caption" tone="muted">
            {t.viewer.switchSourceLabel}
          </Label>
          {model.switchingSession === stream.session ? (
            <Notice>
              <View className="flex-row items-center gap-3">
                <ActivityIndicator />
                <Label className="flex-1" accessibilityLiveRegion="polite">
                  {t.viewer.switchingScreen}
                </Label>
              </View>
            </Notice>
          ) : null}
          <View className="flex-row flex-wrap gap-2">
            {model.displays.map((display) => {
              const current =
                display.sourceId && stream.sourceId
                  ? display.sourceId === stream.sourceId
                  : display.index === stream.sourceIndex;
              return (
                <Action
                  key={display.sourceId || display.index}
                  variant="secondary"
                  size="compact"
                  label={display.name}
                  accessibilityState={{ selected: current }}
                  className={cn(current && "border-strong bg-active")}
                  disabled={current || model.switchingSession !== null}
                  onPress={() =>
                    void model.handleSwitchSessionSource(stream, display)
                  }
                />
              );
            })}
          </View>
        </View>
      ) : null}
    </Surface>
  );
}

function CatalogHeader({
  model,
  openSettings,
}: {
  model: CatalogModel;
  openSettings: () => void;
}) {
  const { t } = useAppLanguage();
  const settingsNotice =
    model.preferencePersistenceIssue ||
    model.nativeSettingsFailures.length > 0 ||
    (model.udpSettingsDirty && model.streams.length > 0);
  return (
    <>
      <View className="gap-3">
        <Label variant="caption" tone="muted">
          {model.host ? t.viewer.connectedHostLabel : t.viewer.standbyBadge}
        </Label>
        <Label variant="code" selectable>
          {model.host || "—"}
        </Label>
        <View className="flex-row flex-wrap gap-2">
          <Action
            variant="secondary"
            className="grow"
            onPress={openSettings}
            label={t.viewer.btnSettings}
          />
          <Action
            variant="ghost"
            className="grow"
            onPress={() => router.push("/host")}
            label={t.viewer.btnChangeHost}
          />
        </View>
      </View>
      {settingsNotice ? (
        <Notice>
          <Label>
            {model.preferencePersistenceIssue || model.nativeSettingsFailures.length > 0
              ? t.viewer.settingsNeedAttention
              : t.viewer.udpApplyReconnect}
          </Label>
        </Notice>
      ) : null}
      {model.visibleError ? (
        <Notice tone="error">
          <Label>{model.visibleError}</Label>
          <Action
            variant="secondary"
            busy={model.refreshing}
            onPress={model.handleRefresh}
            label={t.common.retry}
          />
        </Notice>
      ) : null}
    </>
  );
}
function DisplayListHeader({ model }: { model: CatalogModel }) {
  const { t } = useAppLanguage();
  return (
    <View className="flex-row items-center justify-between gap-2">
      <Label variant="title" className="flex-1">
        {t.viewer.displaysSectionTitle}
      </Label>
      <Action
        variant="ghost"
        size="compact"
        busy={model.loading || model.refreshing}
        onPress={model.handleRefresh}
        label={t.common.refresh}
      />
    </View>
  );
}
function DisplayListEmpty({ model }: { model: CatalogModel }) {
  const { t } = useAppLanguage();
  if (model.visibleError) return null;
  return (
    <Surface variant="card" className="gap-3 py-6">
      {model.loading ? (
        <>
          <ActivityIndicator />
          <Label accessibilityLiveRegion="polite">
            {t.viewer.searchingDisplays}
          </Label>
        </>
      ) : (
        <>
          <Label>{t.viewer.emptyDisplays}</Label>
          <Action
            variant="secondary"
            onPress={model.handleRefresh}
            label={t.common.retry}
          />
        </>
      )}
    </Surface>
  );
}
function CatalogListHeader({
  model,
  openSettings,
  onResolution,
}: {
  model: CatalogModel;
  openSettings: () => void;
  onResolution: (session: number) => void;
}) {
  const { t } = useAppLanguage();
  return (
    <View className="gap-5">
      <CatalogHeader model={model} openSettings={openSettings} />
      {model.streams.length ? (
        <View className="gap-3">
          <Label variant="title">{t.viewer.activeStreamsSection}</Label>
          {model.streams.map((active) => (
            <ActiveStreamCard
              key={active.session}
              stream={active}
              model={model}
              onResolution={() => onResolution(active.session)}
            />
          ))}
        </View>
      ) : null}
      <DisplayListHeader model={model} />
    </View>
  );
}
function CatalogListFooter({
  model,
  openFiles,
}: {
  model: CatalogModel;
  openFiles: () => void;
}) {
  const { t } = useAppLanguage();
  return (
    <View className="gap-5">
      <ExtensionSection model={model} />
      <UsbTransportStatus />
      <Action
        variant="secondary"
        onPress={openFiles}
        label={t.viewer.sectionFileTransfer}
      />
    </View>
  );
}
function ExtensionSection({ model }: { model: CatalogModel }) {
  const { t } = useAppLanguage();
  const { colors } = useAppTheme();
  if (!model.extensionSupported) return null;
  return (
    <ExtensionDisplayCard
      t={t}
      colors={colors}
      exists={Boolean(model.extensionDisplay)}
      pending={model.extensionRemovalPending}
      busy={model.extensionOperation !== null}
      operation={model.extensionOperation}
      configurable={model.extensionConfigurable}
      status={model.extensionStatus}
      onOpen={(mode) => void model.handleCreateExtensionDisplay(mode)}
      onRemove={() => void model.handleRemoveExtensionDisplay()}
      onResize={(mode) => void model.handleResizeExtensionDisplay(mode)}
      onArrange={(position) =>
        void model.handleArrangeExtensionDisplay(position)
      }
    />
  );
}

export default function Catalog() {
  const { t } = useAppLanguage();
  const { colors } = useAppTheme();
  const model = useCatalogModel();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [filesOpen, setFilesOpen] = useState(false);
  const [resolutionSession, setResolutionSession] = useState<number | null>(
    null,
  );
  const stream =
    model.streams.find((item) => item.session === resolutionSession) ?? null;
  const openSettings = useCallback(() => setSettingsOpen(true), []);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
  const renderDisplay = useCallback(
    ({ item }: ListRenderItemInfo<DisplayInfo>) => (
      <DisplayRow display={item} model={model} />
    ),
    [model],
  );
  return (
    <SafeArea className="flex-1 bg-canvas" edges={["left", "right", "bottom"]}>
      <FlatList
        className="flex-1"
        contentContainerClassName="mx-auto w-full max-w-3xl gap-5 px-4 pb-8 pt-4"
        data={model.displays}
        keyExtractor={(display) => String(display.sourceId || display.index)}
        renderItem={renderDisplay}
        ListHeaderComponent={
          <CatalogListHeader
            model={model}
            openSettings={openSettings}
            onResolution={setResolutionSession}
          />
        }
        ListEmptyComponent={<DisplayListEmpty model={model} />}
        ListFooterComponent={
          <CatalogListFooter
            model={model}
            openFiles={() => setFilesOpen(true)}
          />
        }
        refreshControl={
          <RefreshControl
            refreshing={model.refreshing}
            onRefresh={model.handleRefresh}
            tintColor={colors.textPrimary}
          />
        }
      />
      <Sheet
        visible={settingsOpen}
        title={t.viewer.settingsTitle}
        onClose={closeSettings}
      >
        <SettingsContent model={model} />
      </Sheet>
      <Sheet
        visible={filesOpen}
        title={t.viewer.sectionFileTransfer}
        onClose={() => setFilesOpen(false)}
      >
        {filesOpen ? <FileTransferCard colors={colors} /> : null}
      </Sheet>
      <Sheet
        visible={Boolean(stream)}
        title={t.viewer.resolutionSettingsTitle}
        onClose={() => setResolutionSession(null)}
      >
        {stream ? (
          <DisplaySizeCard
            key={stream.session}
            stream={stream}
            resizing={model.resizingSession === stream.session}
            onResizeSession={model.handleResizeSession}
            colors={colors}
          />
        ) : null}
      </Sheet>
    </SafeArea>
  );
}
