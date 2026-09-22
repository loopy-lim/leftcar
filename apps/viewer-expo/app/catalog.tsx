import { ExtensionDisplayCard } from "../src/ExtensionDisplayCard";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ActivityIndicator,
  FlatList,
  type ListRenderItemInfo,
  Modal,
  Pressable,
  RefreshControl,
  ScrollView,
  Switch,
  Text,
  View,
  useWindowDimensions,
  type ViewStyle,
} from "react-native";
import { Ionicons } from "@expo/vector-icons";
import { SafeAreaView } from "react-native-safe-area-context";
import { router, Stack } from "expo-router";
import type { DisplayInfo } from "../src/control";
import { controlClient } from "../src/session";
import {
  getUsbState,
  subscribeUsbState,
  type UsbAccessoryState,
} from "../src/usb";
import {
  STREAM_PROFILES,
} from "../src/stream-profile";
import type {
  EncoderExperimentId,
  EncoderExperimentInfo,
} from "../src/encoder-experiment";
import { UdpStabilityControls } from "../src/UdpStabilityControls";
import { applyPanelDensity, panelDensityScale } from "../src/panel-density";
import { createCatalogStyles } from "../src/catalog-styles";
import type {
  UdpStabilityOptions,
  UdpStabilitySelection,
} from "../src/udp-stability";
import {
  resolveInitialStreamTarget,
  type StreamingPriority,
} from "../src/streaming-policy";
import {
  resolveStreamMaximum,
  resolveViewerProfileId,
  type ViewerProfileSelection,
} from "../src/viewer-preferences";
import type { ActiveStream } from "../src/catalog-model-types";
import { useCatalogModel } from "../src/use-catalog-model";
import { transportBadgeLabel } from "../src/transport-label";
import { DisplaySizeCard } from "../src/DisplaySizeCard";
import { FileTransferCard } from "../src/FileTransferCard";
import { useAppTheme, type ThemeTokens } from "../src/theme";
import { useAppLanguage } from "../src/i18n";

function navigateToHostPicker() {
  router.push("/host");
}

function displayKey(display: DisplayInfo) {
  return String(display.index);
}

function DisplayAspectMiniature({
  width,
  height,
  colors,
}: {
  width: number;
  height: number;
  colors: ThemeTokens;
}) {
  const aspect = width / Math.max(1, height);
  const isWide = aspect > 1.8;
  const isPortrait = aspect < 1.0;

  return (
    <View
      style={{
        width: 44,
        height: 44,
        borderRadius: 10,
        backgroundColor: colors.bgSubtle,
        borderWidth: 1,
        borderColor: colors.borderSubtle,
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
      }}
    >
      <Ionicons
        name={isPortrait ? "phone-portrait-outline" : isWide ? "tv-outline" : "desktop-outline"}
        size={22}
        color={colors.textPrimary}
      />
    </View>
  );
}

const OPTION_ROW_STYLE: ViewStyle = {
  flexDirection: "row",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 12,
};

interface ViewerOptionsCardProps {
  showFps: boolean;
  onToggleFps: (showFps: boolean) => void;
  localCursor: boolean;
  onToggleCursor: (localCursor: boolean) => void;
  localAudio: boolean;
  onToggleAudio: (localAudio: boolean) => void;
  clipboardShare: boolean;
  onToggleClipboardShare: (enabled: boolean) => void;
  colors: ThemeTokens;
}

function ViewerOptionsCard({
  showFps,
  onToggleFps,
  localCursor,
  onToggleCursor,
  localAudio,
  onToggleAudio,
  clipboardShare,
  onToggleClipboardShare,
  colors,
}: ViewerOptionsCardProps) {
  const { t } = useAppLanguage();
  const cardStyle = {
    gap: 10,
    borderRadius: 14,
    borderWidth: 1,
    borderColor: colors.borderSubtle,
    backgroundColor: colors.bgSurface,
    padding: 12,
  };
  const switchColor = {
    trackColor: { false: colors.borderCard, true: colors.btnPrimaryBg },
    thumbColor: colors.btnPrimaryText,
  };

  return (
    <View style={cardStyle}>
      <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>
        {t.viewer.viewerOptionsTitle}
      </Text>
      <View style={OPTION_ROW_STYLE}>
        <View style={{ flex: 1, gap: 2 }}>
          <Text style={{ fontSize: 12, fontWeight: "700", color: colors.textPrimary }}>
            {t.viewer.fpsToggleLabel}
          </Text>
        </View>
        <Switch
          value={showFps}
          onValueChange={onToggleFps}
          accessibilityLabel={t.viewer.fpsToggleLabel}
          {...switchColor}
        />
      </View>
      <View style={OPTION_ROW_STYLE}>
        <View style={{ flex: 1, gap: 2 }}>
          <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>
            {t.viewer.cursorOverlayLabel}
          </Text>
          <Text style={{ fontSize: 12, lineHeight: 16, color: colors.textSecondary }}>
            {t.viewer.cursorOverlayHint}
          </Text>
        </View>
        <Switch
          value={localCursor}
          onValueChange={onToggleCursor}
          accessibilityLabel={t.viewer.cursorOverlayLabel}
          {...switchColor}
        />
      </View>
      <View style={OPTION_ROW_STYLE}>
        <View style={{ flex: 1, gap: 2 }}>
          <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>
            {t.viewer.audioToggleLabel}
          </Text>
          <Text style={{ fontSize: 12, lineHeight: 16, color: colors.textSecondary }}>
            {t.viewer.audioToggleHint}
          </Text>
        </View>
        <Switch
          value={localAudio}
          onValueChange={onToggleAudio}
          accessibilityLabel={t.viewer.audioToggleLabel}
          {...switchColor}
        />
      </View>
      {/* 클립보드 공유 토글(U5) — 호스트 게이트가 기본 꺼짐인 이중 잠금. */}
      <View style={OPTION_ROW_STYLE}>
        <View style={{ flex: 1, gap: 2 }}>
          <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>
            {t.viewer.clipboardShareLabel}
          </Text>
          <Text style={{ fontSize: 12, lineHeight: 16, color: colors.textSecondary }}>
            {t.viewer.clipboardShareHint}
          </Text>
        </View>
        <Switch
          value={clipboardShare}
          onValueChange={onToggleClipboardShare}
          accessibilityLabel={t.viewer.clipboardShareLabel}
          {...switchColor}
        />
      </View>
    </View>
  );
}

/**
 * 전문 토글(인코더/재생 실험) — 고급 설정 안의 한 단계 더 안쪽. 일상 토글과
 * 분리해 실험 기능이 메인 흐름에 섞이지 않게 한다.
 */
function ExpertOptionsCard({
  opusAudio,
  onToggleOpusAudio,
  balancedPresentation,
  onToggleBalancedPresentation,
  presentationSmooth,
  onTogglePresentationSmooth,
  colors,
}: {
  opusAudio: boolean;
  onToggleOpusAudio: (enabled: boolean) => void;
  balancedPresentation: boolean;
  onToggleBalancedPresentation: (enabled: boolean) => void;
  presentationSmooth: boolean;
  onTogglePresentationSmooth: (enabled: boolean) => void;
  colors: ThemeTokens;
}) {
  const { t } = useAppLanguage();
  const cardStyle = {
    gap: 10,
    borderRadius: 14,
    borderWidth: 1,
    borderColor: colors.borderSubtle,
    backgroundColor: colors.bgSurface,
    padding: 12,
  };
  const switchColor = {
    trackColor: { false: colors.borderCard, true: colors.btnPrimaryBg },
    thumbColor: colors.btnPrimaryText,
  };
  return (
    <View style={cardStyle}>
      <View style={OPTION_ROW_STYLE}>
        <Text style={{ flex: 1, color: colors.textPrimary }}>{t.viewer.opusToggleLabel}</Text>
        <Switch value={opusAudio} onValueChange={onToggleOpusAudio}
          accessibilityLabel={t.viewer.opusToggleLabel} {...switchColor} />
      </View>
      <View style={OPTION_ROW_STYLE}>
        <View style={{ flex: 1, gap: 2 }}>
          <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>{t.viewer.balancedPresentationLabel}</Text>
          <Text style={{ fontSize: 12, lineHeight: 16, color: colors.textSecondary }}>{t.viewer.balancedPresentationHint}</Text>
        </View>
        <Switch value={balancedPresentation} onValueChange={onToggleBalancedPresentation}
          accessibilityLabel={t.viewer.balancedPresentationLabel} {...switchColor} />
      </View>
      <View style={OPTION_ROW_STYLE}>
        <View style={{ flex: 1, gap: 2 }}>
          <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>{t.viewer.presentationSmoothLabel}</Text>
          <Text style={{ fontSize: 12, lineHeight: 16, color: colors.textSecondary }}>{t.viewer.presentationSmoothHint}</Text>
        </View>
        <Switch value={presentationSmooth} onValueChange={onTogglePresentationSmooth}
          accessibilityLabel={t.viewer.presentationSmoothLabel} {...switchColor} />
      </View>
    </View>
  );
}

function EncoderExperimentChoices({ experiments, selected, requiresReconnect, colors, t, onSelect }: { experiments: EncoderExperimentInfo[]; selected: EncoderExperimentId; requiresReconnect: boolean; colors: ThemeTokens; t: ReturnType<typeof useAppLanguage>["t"]; onSelect: (id: EncoderExperimentId) => void }) {
  if (experiments.length <= 1) return null;
  return <View style={{ gap: 10, borderRadius: 14, borderWidth: 1, borderColor: colors.borderSubtle, backgroundColor: colors.bgSurface, padding: 12 }}>
    <View style={{ gap: 2 }}><Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>{t.viewer.encoderExperiments}</Text>{requiresReconnect ? <Text style={{ fontSize: 12, color: colors.textMuted, lineHeight: 16 }}>{t.viewer.encoderReconnectNotice}</Text> : null}</View>
    <View style={{ gap: 8 }}>{experiments.map((experiment) => {
      const isSelected = experiment.id === selected;
      return <Pressable key={experiment.id} style={{ minHeight: 44, gap: 3, borderRadius: 8, borderWidth: 1, borderColor: isSelected ? colors.btnPrimaryBg : colors.borderSubtle, backgroundColor: isSelected ? colors.btnPrimaryBg : colors.bgSubtle, paddingHorizontal: 12, paddingVertical: 8 }} onPress={() => onSelect(experiment.id)} accessibilityRole="button" accessibilityState={{ selected: isSelected, disabled: false }} accessibilityLabel={`${experiment.label}: ${experiment.hint}`}>
        <Text style={{ fontSize: 13, fontWeight: "700", color: isSelected ? colors.btnPrimaryText : colors.textPrimary }}>{experiment.label}</Text>
        <Text style={{ fontSize: 12, lineHeight: 16, color: isSelected ? colors.btnPrimaryText : colors.textSecondary, opacity: isSelected ? 0.85 : 1 }}>{experiment.hint}</Text>
      </Pressable>;
    })}</View>
  </View>;
}

const QUALITY_TAB_KEYS = {
  auto: { label: "qualityAutoLabel", detail: "qualityAutoDetail" },
  latency: { label: "qualityLatencyLabel", detail: "qualityLatencyDetail" },
  video: { label: "qualityVideoLabel", detail: "qualityVideoDetail" },
  smooth: { label: "qualitySmoothLabel", detail: "qualitySmoothDetail" },
  balanced: { label: "qualityBalancedLabel", detail: "qualityBalancedDetail" },
  clarity: { label: "qualityClarityLabel", detail: "qualityClarityDetail" },
} as const;

function QualityProfileTabs({
  profileId,
  styles,
  colors,
  onSelect,
}: {
  profileId: ViewerProfileSelection;
  styles: ReturnType<typeof createCatalogStyles>;
  colors: ThemeTokens;
  onSelect: (id: ViewerProfileSelection) => void;
}) {
  const { t } = useAppLanguage();
  const currentDetail = useMemo(() => {
    if (profileId === "auto") return t.viewer.qualityAutoDetail;
    const copy = QUALITY_TAB_KEYS[profileId];
    return copy ? t.viewer[copy.detail] : "";
  }, [profileId, t]);

  return (
    <View style={styles.qualityContainer}>
      <View style={styles.qualitySegmentWrapper}>
        <View style={styles.qualitySegmentTabs}>
          <Pressable
            onPress={() => onSelect("auto")}
            style={[styles.qualityTab, profileId === "auto" && styles.qualityTabActive]}
            accessibilityRole="button"
            accessibilityState={{ selected: profileId === "auto" }}
            accessibilityLabel={t.viewer.qualityAutoA11y}
          >
            <Text
              style={[
                styles.qualityTabLabel,
                profileId === "auto" && styles.qualityTabLabelActive,
              ]}
              numberOfLines={1}
            >
              {t.viewer.qualityAutoLabel}
            </Text>
          </Pressable>
          {STREAM_PROFILES.map((p) => {
            const selected = p.id === profileId;
            const copy = QUALITY_TAB_KEYS[p.id];
            const a11yLabel = `${t.viewer[copy.label]}: ${t.viewer[copy.detail]}`;
            return (
              <Pressable
                key={p.id}
                onPress={() => onSelect(p.id)}
                style={[styles.qualityTab, selected && styles.qualityTabActive]}
                accessibilityRole="button"
                accessibilityState={{ selected }}
                accessibilityLabel={a11yLabel}
              >
                <Text
                  style={[
                    styles.qualityTabLabel,
                    selected && styles.qualityTabLabelActive,
                  ]}
                  numberOfLines={1}
                >
                  {t.viewer[copy.label]}
                </Text>
              </Pressable>
            );
          })}
        </View>
      </View>
      {currentDetail ? (
        <View style={styles.qualityHintRow}>
          <Ionicons name="sparkles-outline" size={12} color={colors.textMuted} />
          <Text style={styles.qualityHintText}>{currentDetail}</Text>
        </View>
      ) : null}
    </View>
  );
}

interface CatalogHeaderProps {
  error: string | null;
  host: string;
  loading: boolean;
  profileId: ViewerProfileSelection;
  refreshing: boolean;
  onRefresh: () => void;
  onSelectProfile: (id: ViewerProfileSelection) => void;
  onOpenSettings: () => void;
  hasSettingsNotice: boolean;
  styles: ReturnType<typeof createCatalogStyles>;
  colors: ThemeTokens;
}

interface CatalogSettingsModalProps {
  visible: boolean;
  onClose: () => void;
  showFps: boolean;
  onToggleFps: (showFps: boolean) => void;
  localCursor: boolean;
  onToggleCursor: (localCursor: boolean) => void;
  localAudio: boolean;
  onToggleAudio: (localAudio: boolean) => void;
  clipboardShare: boolean;
  onToggleClipboardShare: (enabled: boolean) => void;
  opusAudio: boolean;
  onToggleOpusAudio: (enabled: boolean) => void;
  balancedPresentation: boolean;
  onToggleBalancedPresentation: (enabled: boolean) => void;
  presentationSmooth: boolean;
  onTogglePresentationSmooth: (enabled: boolean) => void;
  /** 이 기기의 상시 원격 입력 허용 여부(호스트 정책). 구호스트는 undefined로
   * 행을 숨긴다. */
  inputAllowed?: boolean | null;
  encoderExperiments: EncoderExperimentInfo[];
  encoderExperiment: EncoderExperimentId;
  onSelectEncoderExperiment: (id: EncoderExperimentId) => void;
  udpStabilityOptions: UdpStabilityOptions | null;
  udpStability: UdpStabilitySelection;
  udpReconnectRequired: boolean;
  udpReconnecting: boolean;
  onSelectUdpStability: (selection: UdpStabilitySelection) => void;
  onApplyUdpStability: () => void;
  styles: ReturnType<typeof createCatalogStyles>;
  colors: ThemeTokens;
}

function CatalogSettingsModal({
  visible,
  onClose,
  showFps,
  onToggleFps,
  localCursor,
  onToggleCursor,
  localAudio,
  onToggleAudio,
  clipboardShare,
  onToggleClipboardShare,
  opusAudio,
  onToggleOpusAudio,
  balancedPresentation,
  onToggleBalancedPresentation,
  presentationSmooth,
  onTogglePresentationSmooth,
  inputAllowed = null,
  encoderExperiments,
  encoderExperiment,
  onSelectEncoderExperiment,
  udpStabilityOptions,
  udpStability,
  udpReconnectRequired,
  udpReconnecting,
  onSelectUdpStability,
  onApplyUdpStability,
  styles,
  colors,
}: CatalogSettingsModalProps) {
  const { t } = useAppLanguage();
  const requiresReconnect = encoderExperiments.some(
    (experiment) => experiment.requiresReconnect,
  );
  const [expertManuallyToggled, setExpertManuallyToggled] = useState<boolean | null>(null);
  const showExpert =
    expertManuallyToggled !== null ? expertManuallyToggled : udpReconnectRequired;

  const handleToggleExpert = useCallback(() => {
    setExpertManuallyToggled(!showExpert);
  }, [showExpert]);

  return (
    <Modal
      visible={visible}
      animationType="slide"
      transparent
      onRequestClose={onClose}
    >
      <View style={styles.modalOverlay}>
        <View style={styles.modalSheet}>
          <View style={styles.modalHeader}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
              <Ionicons name="settings-outline" size={18} color={colors.textPrimary} />
              <Text style={styles.modalTitle}>{t.viewer.settingsTitle}</Text>
            </View>
            <Pressable
              onPress={onClose}
              style={styles.modalCloseBtn}
              accessibilityRole="button"
              accessibilityLabel={t.viewer.settingsModalClose}
            >
              <Ionicons name="close" size={20} color={colors.textSecondary} />
            </Pressable>
          </View>
          <ScrollView
            contentContainerStyle={styles.modalScrollView}
            showsVerticalScrollIndicator={false}
          >
            <View style={styles.modalContentGap}>
              <View style={styles.modalSection}>
                <Text style={styles.modalSectionTitle}>{t.viewer.sectionViewerOptions}</Text>
                <ViewerOptionsCard
                  showFps={showFps}
                  onToggleFps={onToggleFps}
                  localCursor={localCursor}
                  onToggleCursor={onToggleCursor}
                  localAudio={localAudio}
                  onToggleAudio={onToggleAudio}
                  clipboardShare={clipboardShare}
                  onToggleClipboardShare={onToggleClipboardShare}
                  colors={colors}
                />
                {typeof inputAllowed === "boolean" && (
                  <View
                    style={{
                      flexDirection: "row",
                      alignItems: "center",
                      justifyContent: "space-between",
                      marginTop: 10,
                    }}
                    accessibilityLabel={`${t.viewer.remoteInputLabel} ${
                      inputAllowed
                        ? t.viewer.remoteInputAllowedLabel
                        : t.viewer.remoteInputBlockedLabel
                    }`}
                  >
                    <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                      <Ionicons
                        name={inputAllowed ? "lock-open-outline" : "lock-closed-outline"}
                        size={14}
                        color={inputAllowed ? colors.textSecondary : colors.textMuted}
                      />
                      <Text style={styles.advancedToggleText}>{t.viewer.remoteInputLabel}</Text>
                    </View>
                    <Text
                      style={{
                        fontSize: 12,
                        color: inputAllowed ? colors.textSecondary : colors.textMuted,
                      }}
                    >
                      {inputAllowed
                        ? t.viewer.remoteInputAllowedLabel
                        : t.viewer.remoteInputBlockedLabel}
                    </Text>
                  </View>
                )}
              </View>

              <View style={styles.modalSection}>
                <Text style={styles.modalSectionTitle}>{t.viewer.sectionFileTransfer}</Text>
                <FileTransferCard colors={colors} />
              </View>

              <View style={styles.modalSection}>
                <Text style={styles.modalSectionTitle}>{t.viewer.sectionAdvanced}</Text>
                <Pressable
                  style={styles.advancedToggleRow}
                  onPress={handleToggleExpert}
                  accessibilityRole="button"
                  accessibilityLabel={t.viewer.expertSettingsToggle}
                >
                  <View style={styles.advancedToggleLeft}>
                    <Ionicons name="hardware-chip-outline" size={14} color={colors.textMuted} />
                    <Text style={styles.advancedToggleText}>{t.viewer.expertSettingsToggle}</Text>
                    {udpReconnectRequired ? <View style={styles.reconnectDot} /> : null}
                  </View>
                  <Ionicons
                    name={showExpert ? "chevron-up" : "chevron-down"}
                    size={14}
                    color={colors.textMuted}
                  />
                </Pressable>

                {showExpert ? (
                  <View style={styles.advancedSectionContainer}>
                    <ExpertOptionsCard
                      opusAudio={opusAudio}
                      onToggleOpusAudio={onToggleOpusAudio}
                      balancedPresentation={balancedPresentation}
                      onToggleBalancedPresentation={onToggleBalancedPresentation}
                      presentationSmooth={presentationSmooth}
                      onTogglePresentationSmooth={onTogglePresentationSmooth}
                      colors={colors}
                    />

                    <EncoderExperimentChoices
                      experiments={encoderExperiments}
                      selected={encoderExperiment}
                      requiresReconnect={requiresReconnect}
                      colors={colors}
                      t={t}
                      onSelect={onSelectEncoderExperiment}
                    />

                    <UdpStabilityControls
                      options={udpStabilityOptions}
                      selection={udpStability}
                      reconnectRequired={udpReconnectRequired}
                      reconnecting={udpReconnecting}
                      onChange={onSelectUdpStability}
                      onApplyReconnect={onApplyUdpStability}
                    />
                  </View>
                ) : null}
              </View>
            </View>
          </ScrollView>
        </View>
      </View>
    </Modal>
  );
}

function CatalogHeader({
  error,
  host,
  loading,
  profileId,
  refreshing,
  onRefresh,
  onSelectProfile,
  onOpenSettings,
  hasSettingsNotice,
  styles,
  colors,
}: CatalogHeaderProps) {
  const { t } = useAppLanguage();
  const refreshDisabled = loading || refreshing;

  return (
    <View style={styles.headerContainer}>
      {/* Slim Connected Host Strip */}
      <View style={styles.hostStrip}>
        <View style={styles.hostStripLeft}>
          <View style={styles.dotConnected} />
          <Text style={styles.hostStripText} numberOfLines={1}>
            {t.viewer.connectedHostLabel}{" "}
            <Text style={styles.hostStripAddr}>{host || "—"}</Text>
          </Text>
        </View>
        <View style={styles.hostStripActions}>
          <Pressable
            onPress={onOpenSettings}
            style={styles.btnHostSettings}
            accessibilityRole="button"
            accessibilityLabel={t.viewer.settingsTitle}
          >
            <Ionicons name="settings-outline" size={12} color={colors.btnSecondaryText} />
            <Text style={styles.btnHostSettingsText}>{t.viewer.btnSettings}</Text>
            {hasSettingsNotice ? <View style={styles.reconnectDot} /> : null}
          </Pressable>
          <Pressable
            onPress={navigateToHostPicker}
            style={styles.btnHostChange}
            accessibilityRole="button"
            accessibilityLabel={t.viewer.btnChangeHost}
          >
            <Text style={styles.btnHostChangeText}>{t.viewer.btnChangeHost}</Text>
          </Pressable>
        </View>
      </View>

      <UsbTransportStatus styles={styles} />

      {/* Error Card */}
      {error ? (
        <View style={styles.errorCard}>
          <Ionicons name="alert-circle-outline" size={16} color={colors.textPrimary} />
          <View style={styles.errorBody}>
            <Text style={styles.errorText}>{error}</Text>
            <View style={styles.errorActions}>
              <Pressable
                onPress={onRefresh}
                style={styles.errorRetryBtn}
                disabled={refreshDisabled}
              >
                <Text style={styles.errorRetryText}>{t.common.retry}</Text>
              </Pressable>
              <Pressable onPress={navigateToHostPicker} style={styles.errorHostBtn}>
                <Text style={styles.errorHostText}>{t.viewer.btnChangeHost}</Text>
              </Pressable>
            </View>
          </View>
        </View>
      ) : null}

      <QualityProfileTabs profileId={profileId} styles={styles} colors={colors} onSelect={onSelectProfile} />

      {/* Section Header */}
      <View style={styles.sectionTitleRow}>
        <Text style={styles.sectionTitleText}>{t.viewer.displaysSectionTitle}</Text>
        <Pressable
          onPress={onRefresh}
          style={[styles.btnRefresh, refreshDisabled && styles.btnDisabled]}
          disabled={refreshDisabled}
        >
          {refreshDisabled ? (
            <ActivityIndicator color={colors.textPrimary} size="small" />
          ) : (
            <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
              <Ionicons name="refresh-outline" size={13} color={colors.textPrimary} />
              <Text style={styles.btnRefreshText}>{t.common.refresh}</Text>
            </View>
          )}
        </Pressable>
      </View>
    </View>
  );
}

function UsbTransportStatus({ styles }: { styles: ReturnType<typeof createCatalogStyles> }) {
  const { t } = useAppLanguage();
  const { colors } = useAppTheme();
  const [state, setState] = useState<UsbAccessoryState>({ attached: false, controlPort: 0 });
  const [requesting, setRequesting] = useState(false);

  useEffect(() => {
    let active = true;
    void getUsbState().then((next) => {
      if (active) setState(next);
    });
    const subscription = subscribeUsbState(setState);
    return () => {
      active = false;
      subscription.remove();
    };
  }, []);

  const canRequestPermission =
    !state.attached && !state.permissionPending && state.accessoryPresent === true;

  // 화면을 열 때와 같은 requestUsb 제어 명령으로 시스템 권한 다이얼로그를
  // 미리 띄운다. 상태 갱신은 USB 구독이 담당한다.
  const handleGrantPermission = useCallback(() => {
    if (requesting) return;
    const client = controlClient();
    if (!client) return;
    setRequesting(true);
    client
      .request("requestUsb")
      .catch(() => undefined)
      .finally(() => setRequesting(false));
  }, [requesting]);

  if (!state.attached && !state.permissionPending && !state.accessoryPresent) {
    return null;
  }

  let transportLabel: string = t.viewer.usbDetected;
  if (state.attached) {
    transportLabel = t.viewer.usbAttached;
  } else if (state.permissionPending) {
    transportLabel = t.viewer.usbPending;
  }

  return (
    <View style={styles.transportStrip}>
      <View style={[styles.transportDot, state.attached && styles.transportDotUsb]} />
      <Text style={styles.transportText}>{transportLabel}</Text>
      {canRequestPermission && !requesting ? (
        <Pressable
          onPress={handleGrantPermission}
          style={styles.transportAction}
          accessibilityRole="button"
          accessibilityLabel={t.viewer.usbGrantAction}
        >
          <Text style={styles.transportActionText}>{t.viewer.usbGrantAction}</Text>
        </Pressable>
      ) : null}
      {canRequestPermission && requesting ? (
        <ActivityIndicator size="small" color={colors.textPrimary} />
      ) : null}
    </View>
  );
}

interface DisplayListItemProps {
  display: DisplayInfo;
  disabled: boolean;
  isLaunching: boolean;
  isSwitching: boolean;
  isActive: boolean;
  hasActiveStream: boolean;
  profileSelection: ViewerProfileSelection;
  streamingPriority: StreamingPriority;
  /** LAN 밖 경로(테일넷·공인) — 시작 크기 표시가 실제 시작과 같아야 한다. */
  externalRoute: boolean;
  /** 클라이언트 패널 단변 캡 — 카드 라벨도 열기 경로와 같은 크기를 보여준다. */
  panelShortSide?: number;
  onOpen: (display: DisplayInfo) => void;
  onSwitch: (display: DisplayInfo) => void;
  styles: ReturnType<typeof createCatalogStyles>;
  colors: ThemeTokens;
}

function DisplayListItem({
  display,
  disabled,
  isLaunching,
  isSwitching,
  isActive,
  hasActiveStream,
  profileSelection,
  streamingPriority,
  externalRoute,
  panelShortSide,
  onOpen,
  onSwitch,
  styles,
  colors,
}: DisplayListItemProps) {
  const { t } = useAppLanguage();
  const effectiveProfileId = resolveViewerProfileId(profileSelection, display);
  const profile = STREAM_PROFILES.find((candidate) => candidate.id === effectiveProfileId)
    ?? STREAM_PROFILES[0];
  // 목록에 실제로 열릴 시작 크기(우선순위 적용)를 표시한다 — 최대와 다르면
  // 사용자가 시작 크기를 알 수 있어야 한다. 열기 경로와 같은 공유 헬퍼로
  // 최대를 정한다.
  const size = resolveInitialStreamTarget(
    display,
    streamingPriority,
    resolveStreamMaximum(display, profileSelection),
    { externalRoute, panelShortSide },
  );
  const isBusy = isLaunching || isSwitching;
  const isItemDisabled = disabled || isBusy || isActive;

  const handlePress = useCallback(() => {
    if (isItemDisabled) return;
    if (hasActiveStream) {
      onSwitch(display);
    } else {
      onOpen(display);
    }
  }, [display, hasActiveStream, isItemDisabled, onOpen, onSwitch]);

  const monitorLabel = display.index === 0 ? t.viewer.primaryDisplay : t.viewer.secondaryDisplay;

  return (
    <Pressable
      style={({ pressed }) => [
        styles.displayCard,
        pressed && !isItemDisabled && styles.itemPressed,
      ]}
      onPress={handlePress}
      disabled={isItemDisabled}
    >
      <DisplayAspectMiniature width={display.width} height={display.height} colors={colors} />

      <View style={styles.displayMain}>
        <View style={styles.displayNameRow}>
          <Text style={styles.displayName} numberOfLines={1}>
            {display.name}
          </Text>
          <View style={styles.displayIndexBadge}>
            <Text style={styles.displayIndexText}>
              #{display.index} {monitorLabel}
            </Text>
          </View>
        </View>
        <View style={styles.chipsRow}>
          <View style={styles.chip}>
            <Text style={styles.chipText}>
              {size.width} × {size.height}
            </Text>
          </View>
          <View style={styles.chip}>
            <Text style={styles.chipText}>{profile.fps} FPS</Text>
          </View>
        </View>
      </View>

      {isBusy ? (
        <View style={[styles.openBtn, styles.btnDisabled]}>
          <ActivityIndicator color={colors.btnPrimaryText} size="small" />
        </View>
      ) : isActive ? (
        <View style={styles.streamingBadge}>
          <View style={styles.dotActive} />
          <Text style={styles.streamingBadgeText}>{t.viewer.currentlyStreaming}</Text>
        </View>
      ) : hasActiveStream ? (
        <View style={styles.btnSwitchDisplay}>
          <Ionicons name="swap-horizontal" size={12} color={colors.btnSecondaryText} />
          <Text style={styles.btnSwitchDisplayText}>{t.viewer.switchToDisplay}</Text>
        </View>
      ) : (
        <View style={styles.openBtn}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
            <Text style={styles.openBtnText}>{t.common.open}</Text>
            <Ionicons name="arrow-forward" size={12} color={colors.btnPrimaryText} />
          </View>
        </View>
      )}
    </Pressable>
  );
}

function EmptyDisplayList({
  loading,
  styles,
  colors,
}: {
  loading: boolean;
  styles: ReturnType<typeof createCatalogStyles>;
  colors: ThemeTokens;
}) {
  const { t } = useAppLanguage();
  return (
    <View style={styles.emptyCard}>
      {loading ? (
        <>
          <ActivityIndicator size="large" color={colors.textPrimary} />
          <Text style={styles.loadingText}>{t.viewer.searchingDisplays}</Text>
        </>
      ) : (
        <Text style={styles.emptyText}>{t.viewer.emptyDisplays}</Text>
      )}
    </View>
  );
}

interface ActiveStreamItemProps {
  stream: ActiveStream;
  displays: DisplayInfo[];
  switching: boolean;
  onStop: (stream: ActiveStream) => void;
  onChangeResolution: (stream: ActiveStream) => void;
  onSwitchDisplay: (stream: ActiveStream, display: DisplayInfo) => void;
  styles: ReturnType<typeof createCatalogStyles>;
  colors: ThemeTokens;
}

function ActiveStreamItem({
  stream,
  displays,
  switching,
  onStop,
  onChangeResolution,
  onSwitchDisplay,
  styles,
  colors,
}: ActiveStreamItemProps) {
  const { t } = useAppLanguage();
  const handleStop = useCallback(() => onStop(stream), [onStop, stream]);
  const handleChangeResolution = useCallback(
    () => onChangeResolution(stream),
    [onChangeResolution, stream],
  );
  return (
    <View style={styles.streamCard}>
      <View style={styles.streamCardTop}>
        <View style={styles.streamInfo}>
          <View style={styles.streamNameRow}>
            <View style={styles.dotActive} />
            <Text style={styles.streamName} numberOfLines={1}>
              {stream.sourceName}
            </Text>
          </View>
          <View style={styles.streamSpecRow}>
            <Text style={styles.streamPort} numberOfLines={1}>
              {stream.width} × {stream.height} · {stream.fps} FPS
            </Text>
            <Text style={styles.transportBadge}>{transportBadgeLabel(stream.mediaTransport)}</Text>
          </View>
        </View>
        <View style={styles.streamActions}>
          <Pressable
            style={styles.btnChangeResolution}
            onPress={handleChangeResolution}
            accessibilityRole="button"
            accessibilityLabel={t.viewer.changeResolution}
          >
            <Ionicons name="resize-outline" size={12} color={colors.btnSecondaryText} />
            <Text style={styles.btnChangeResolutionText}>{t.viewer.changeResolution}</Text>
          </Pressable>
          <Pressable
            style={styles.stopBtn}
            onPress={handleStop}
            accessibilityRole="button"
            accessibilityLabel={t.common.stop}
          >
            <Text style={styles.stopBtnText}>{t.common.stop}</Text>
          </Pressable>
        </View>
      </View>
      {displays.length > 1 ? (
        <View style={styles.displaySwitchBar}>
          <Text style={styles.displaySwitchLabel}>{t.viewer.switchSourceLabel}:</Text>
          <View style={styles.displaySwitchChips}>
            {displays.map((d) => {
              const isCurrent =
                d.sourceId && stream.sourceId
                  ? d.sourceId === stream.sourceId
                  : d.index === stream.sourceIndex;
              const monitorLabel = d.index === 0 ? t.viewer.primaryDisplay : t.viewer.secondaryDisplay;
              return (
                <Pressable
                  key={d.index}
                  style={[
                    styles.displaySwitchChip,
                    isCurrent && styles.displaySwitchChipActive,
                    switching && styles.btnDisabled,
                  ]}
                  disabled={isCurrent || switching}
                  onPress={() => onSwitchDisplay(stream, d)}
                  accessibilityRole="button"
                  accessibilityLabel={`#${d.index} ${monitorLabel}`}
                >
                  <Text
                    style={[
                      styles.displaySwitchText,
                      isCurrent && styles.displaySwitchTextActive,
                    ]}
                  >
                    #{d.index} {monitorLabel}
                  </Text>
                </Pressable>
              );
            })}
          </View>
        </View>
      ) : null}
    </View>
  );
}

interface ResolutionModalProps {
  stream: ActiveStream | null;
  onClose: () => void;
  resizing: boolean;
  onResizeSession: React.ComponentProps<typeof DisplaySizeCard>["onResizeSession"];
  styles: ReturnType<typeof createCatalogStyles>;
  colors: ThemeTokens;
}

function ResolutionModal({
  stream,
  onClose,
  resizing,
  onResizeSession,
  styles,
  colors,
}: ResolutionModalProps) {
  const { t } = useAppLanguage();
  if (!stream) return null;

  return (
    <Modal
      visible={Boolean(stream)}
      animationType="slide"
      transparent
      onRequestClose={onClose}
    >
      <View style={styles.modalOverlay}>
        <View style={styles.modalSheet}>
          <View style={styles.modalHeader}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
              <Ionicons name="resize-outline" size={18} color={colors.textPrimary} />
              <Text style={styles.modalTitle}>{t.viewer.resolutionSettingsTitle}</Text>
            </View>
            <Pressable
              onPress={onClose}
              style={styles.modalCloseBtn}
              accessibilityRole="button"
              accessibilityLabel={t.viewer.resolutionModalClose}
            >
              <Ionicons name="close" size={20} color={colors.textSecondary} />
            </Pressable>
          </View>
          <ScrollView
            contentContainerStyle={styles.modalScrollView}
            showsVerticalScrollIndicator={false}
          >
            <DisplaySizeCard
              stream={stream}
              resizing={resizing}
              onResizeSession={onResizeSession}
              colors={colors}
            />
          </ScrollView>
        </View>
      </View>
    </Modal>
  );
}

interface CatalogFooterProps {
  streams: ActiveStream[];
  displays: DisplayInfo[];
  switchingSession: number | null;
  onStop: (stream: ActiveStream) => void;
  onChangeResolution: (stream: ActiveStream) => void;
  onSwitchDisplay: (stream: ActiveStream, display: DisplayInfo) => void;
  styles: ReturnType<typeof createCatalogStyles>;
  colors: ThemeTokens;
}

function CatalogFooter({
  streams,
  displays,
  switchingSession,
  onStop,
  onChangeResolution,
  onSwitchDisplay,
  styles,
  colors,
}: CatalogFooterProps) {
  const { t } = useAppLanguage();

  if (streams.length === 0) return null;

  return (
    <View style={styles.footerContainer}>
      <View style={styles.activeSection}>
        <View style={styles.activeSectionHeader}>
          <Text style={styles.activeSectionTitle}>{t.viewer.activeStreamsSection}</Text>
          <View style={styles.activeCountBadge}>
            <Text style={styles.activeCountText}>{streams.length}</Text>
          </View>
        </View>
        {streams.map((stream) => (
          <ActiveStreamItem
            key={stream.session}
            stream={stream}
            displays={displays}
            switching={switchingSession === stream.session}
            onStop={onStop}
            onChangeResolution={onChangeResolution}
            onSwitchDisplay={onSwitchDisplay}
            styles={styles}
            colors={colors}
          />
        ))}
      </View>
    </View>
  );
}

export default function Catalog() {
  const { colors, isDark } = useAppTheme();
  const { width } = useWindowDimensions();
  const { t } = useAppLanguage();
  const density = panelDensityScale(width);
  const styles = useMemo(
    () => applyPanelDensity(createCatalogStyles(colors, isDark), density),
    [colors, isDark, density],
  );
  const model = useCatalogModel();

  const [settingsOpen, setSettingsOpen] = useState(false);
  const [resolutionSession, setResolutionSession] = useState<number | null>(null);

  const activeResolutionStream = useMemo(
    () => model.streams.find((s) => s.session === resolutionSession) ?? null,
    [model.streams, resolutionSession],
  );

  const udpReconnectRequired = model.udpSettingsDirty && model.streams.length > 0;
  const hasSettingsNotice = udpReconnectRequired;

  const handleOpenSettings = useCallback(() => {
    setSettingsOpen(true);
  }, []);

  const handleCloseSettings = useCallback(() => {
    setSettingsOpen(false);
  }, []);

  const handleChangeResolution = useCallback((stream: ActiveStream) => {
    setResolutionSession(stream.session);
  }, []);

  const handleCloseResolution = useCallback(() => {
    setResolutionSession(null);
  }, []);

  const activeStream = model.streams[0] ?? null;

  const handleSwitchDisplay = useCallback(
    (display: DisplayInfo) => {
      if (!activeStream) return;
      void model.handleSwitchSessionSource(activeStream, display);
    },
    [activeStream, model.handleSwitchSessionSource],
  );

  const handleSwitchActiveStreamSource = useCallback(
    (stream: ActiveStream, display: DisplayInfo) => {
      void model.handleSwitchSessionSource(stream, display);
    },
    [model.handleSwitchSessionSource],
  );

  const renderDisplay = useCallback(
    ({ item }: ListRenderItemInfo<DisplayInfo>) => {
      const isDisplayActive = model.streams.some((s) =>
        item.sourceId && s.sourceId
          ? s.sourceId === item.sourceId
          : s.sourceIndex === item.index,
      );
      return (
        <DisplayListItem
          display={item}
          disabled={model.launchingIndex !== null || model.switchingSession !== null}
          isLaunching={model.launchingIndex === item.index}
          isSwitching={model.switchingSession !== null}
          isActive={isDisplayActive}
          hasActiveStream={activeStream !== null}
          profileSelection={model.profileId}
          streamingPriority={model.streamingPriority}
          panelShortSide={model.panelShortSide}
          externalRoute={model.externalMediaRoute}
          onOpen={model.openDisplay}
          onSwitch={handleSwitchDisplay}
          styles={styles}
          colors={colors}
        />
      );
    },
    [
      activeStream,
      colors,
      handleSwitchDisplay,
      model.externalMediaRoute,
      model.launchingIndex,
      model.openDisplay,
      model.panelShortSide,
      model.profileId,
      model.streams,
      model.streamingPriority,
      model.switchingSession,
      styles,
    ],
  );

  return (
    <SafeAreaView style={styles.safeArea} edges={["left", "right", "bottom"]}>
      <Stack.Screen
        options={{
          headerRight: () => (
            <Pressable
              onPress={handleOpenSettings}
              hitSlop={8}
              style={{
                flexDirection: "row",
                alignItems: "center",
                paddingHorizontal: 8,
                paddingVertical: 4,
              }}
              accessibilityRole="button"
              accessibilityLabel={t.viewer.settingsTitle}
            >
              <Ionicons name="settings-outline" size={20} color={colors.textPrimary} />
              {hasSettingsNotice ? (
                <View
                  style={{
                    position: "absolute",
                    top: 2,
                    right: 6,
                    width: 6,
                    height: 6,
                    borderRadius: 3,
                    backgroundColor: colors.brandPrimary,
                  }}
                />
              ) : null}
            </Pressable>
          ),
        }}
      />
      <FlatList
        style={styles.root}
        contentContainerStyle={styles.content}
        showsVerticalScrollIndicator={false}
        refreshControl={
          <RefreshControl
            refreshing={model.refreshing}
            onRefresh={model.handleRefresh}
            tintColor={colors.textPrimary}
          />
        }
        ListHeaderComponent={
          <>
            <CatalogHeader
              error={model.visibleError}
              host={model.host}
              loading={model.loading}
              profileId={model.profileId}
              refreshing={model.refreshing}
              onRefresh={model.handleRefresh}
              onSelectProfile={model.handleSelectProfile}
              onOpenSettings={handleOpenSettings}
              hasSettingsNotice={hasSettingsNotice}
              styles={styles}
              colors={colors}
            />
            {model.extensionSupported && <ExtensionDisplayCard
              t={t} colors={colors} exists={Boolean(model.extensionDisplay)}
              pending={model.extensionRemovalPending} busy={model.extensionOperation !== null}
              configurable={model.extensionConfigurable} status={model.extensionStatus}
              onOpen={mode => void model.handleCreateExtensionDisplay(mode)}
              onRemove={() => void model.handleRemoveExtensionDisplay()}
              onResize={mode => void model.handleResizeExtensionDisplay(mode)}
              onArrange={position => void model.handleArrangeExtensionDisplay(position)}
            />}
          </>
        }
        data={model.displays}
        keyExtractor={displayKey}
        renderItem={renderDisplay}
        ListEmptyComponent={<EmptyDisplayList loading={model.loading} styles={styles} colors={colors} />}
        ListFooterComponent={
          <CatalogFooter
            streams={model.streams}
            displays={model.displays}
            switchingSession={model.switchingSession}
            onStop={model.stopStream}
            onChangeResolution={handleChangeResolution}
            onSwitchDisplay={handleSwitchActiveStreamSource}
            styles={styles}
            colors={colors}
          />
        }
      />

      <CatalogSettingsModal
        visible={settingsOpen}
        onClose={handleCloseSettings}
        showFps={model.showFps}
        onToggleFps={model.handleToggleFps}
        localCursor={model.localCursor}
        onToggleCursor={model.handleToggleCursor}
        localAudio={model.localAudio}
        onToggleAudio={model.handleToggleAudio}
        clipboardShare={model.clipboardShare}
        onToggleClipboardShare={model.handleToggleClipboardShare}
        opusAudio={model.opusAudio}
        onToggleOpusAudio={model.handleToggleOpusAudio}
        balancedPresentation={model.balancedPresentation}
        onToggleBalancedPresentation={model.handleToggleBalancedPresentation}
        presentationSmooth={model.presentationSmooth}
        onTogglePresentationSmooth={model.handleTogglePresentationSmooth}
        inputAllowed={model.inputAllowed}
        encoderExperiments={model.selectedEncoderExperiments}
        encoderExperiment={model.effectiveNextEncoderExperiment}
        onSelectEncoderExperiment={model.handleSelectEncoderExperiment}
        udpStabilityOptions={model.udpStabilityOptions}
        udpStability={model.effectiveUdpStability}
        udpReconnectRequired={udpReconnectRequired}
        udpReconnecting={model.udpReconnecting}
        onSelectUdpStability={model.handleSelectUdpStability}
        onApplyUdpStability={model.handleApplyUdpStability}
        styles={styles}
        colors={colors}
      />

      <ResolutionModal
        stream={activeResolutionStream}
        onClose={handleCloseResolution}
        resizing={model.resizingSession === activeResolutionStream?.session}
        onResizeSession={model.handleResizeSession}
        styles={styles}
        colors={colors}
      />
    </SafeAreaView>
  );
}
