import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ActivityIndicator,
  FlatList,
  type ListRenderItemInfo,
  Pressable,
  RefreshControl,
  Switch,
  Text,
  View,
} from "react-native";
import { Ionicons } from "@expo/vector-icons";
import { SafeAreaView } from "react-native-safe-area-context";
import { router } from "expo-router";
import type { DisplayInfo } from "../src/control";
import {
  getUsbState,
  subscribeUsbState,
  type UsbAccessoryState,
} from "../src/usb";
import {
  STREAM_PROFILES,
  type StreamProfile,
  type StreamProfileId,
} from "../src/stream-profile";
import type {
  EncoderExperimentId,
  EncoderExperimentInfo,
} from "../src/encoder-experiment";
import { UdpStabilityControls } from "../src/UdpStabilityControls";
import { createCatalogStyles } from "../src/catalog-styles";
import type {
  UdpStabilityOptions,
  UdpStabilitySelection,
} from "../src/udp-stability";
import { fitProfileToDisplay } from "../src/catalog-helpers";
import { recommendedStreamProfileId } from "../src/viewer-preferences";
import type { ActiveStream } from "../src/catalog-model-types";
import { useCatalogModel } from "../src/use-catalog-model";
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
  let miniW = 32;
  let miniH = 19;
  if (aspect >= 2.0) {
    miniW = 36;
    miniH = 15;
  } else if (aspect >= 1.7) {
    miniW = 34;
    miniH = 19;
  } else if (aspect >= 1.4) {
    miniW = 30;
    miniH = 20;
  } else if (aspect >= 1.1) {
    miniW = 26;
    miniH = 20;
  } else {
    miniW = 18;
    miniH = 28;
  }

  return (
    <View
      style={{
        width: 44,
        height: 40,
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
      }}
    >
      <View
        style={{
          width: miniW,
          height: miniH,
          borderWidth: 1.5,
          borderColor: colors.textPrimary,
          borderRadius: 3,
          backgroundColor: colors.bgSubtle,
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <View
          style={{
            width: "70%",
            height: "50%",
            backgroundColor: colors.borderCard,
            borderRadius: 1,
          }}
        />
      </View>
      <View
        style={{
          width: 3,
          height: 3,
          backgroundColor: colors.textPrimary,
        }}
      />
      <View
        style={{
          width: 14,
          height: 2,
          backgroundColor: colors.textPrimary,
          borderRadius: 1,
        }}
      />
    </View>
  );
}

interface CatalogHeaderProps {
  error: string | null;
  host: string;
  loading: boolean;
  profileId: StreamProfileId;
  refreshing: boolean;
  onRefresh: () => void;
  onSelectProfile: (id: StreamProfileId) => void;
  showFps: boolean;
  onToggleFps: (showFps: boolean) => void;
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

function CatalogHeader({
  error,
  host,
  loading,
  profileId,
  refreshing,
  onRefresh,
  onSelectProfile,
  showFps,
  onToggleFps,
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
}: CatalogHeaderProps) {
  const { t } = useAppLanguage();
  const refreshDisabled = loading || refreshing;
  const requiresReconnect = encoderExperiments.some(
    (experiment) => experiment.requiresReconnect,
  );
  const hasViewerOptions = true;
  const hasAdvancedOptions =
    hasViewerOptions || encoderExperiments.length > 1 || udpStabilityOptions !== null;
  const [manuallyToggled, setManuallyToggled] = useState<boolean | null>(null);
  const showAdvanced = manuallyToggled !== null ? manuallyToggled : udpReconnectRequired;

  return (
    <View style={styles.headerContainer}>
      {/* Slim Connected Host Strip */}
      <View style={styles.hostStrip}>
        <View style={styles.hostStripLeft}>
          <View style={styles.dotConnected} />
          <Text style={styles.hostStripText} numberOfLines={1}>
            {t.viewer.connectedHostLabel} <Text style={styles.hostStripAddr}>{host}</Text>
          </Text>
        </View>
        <Pressable onPress={navigateToHostPicker} style={styles.btnHostChange}>
          <Text style={styles.btnHostChangeText}>{t.viewer.btnChangeHost}</Text>
        </Pressable>
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

      {/* Segmented Quality Control */}
      <View style={styles.qualitySegmentWrapper}>
        <View style={styles.qualitySegmentTabs}>
          {STREAM_PROFILES.map((p) => {
            const isSelected = p.id === profileId;
            return (
              <Pressable
                key={p.id}
                onPress={() => onSelectProfile(p.id)}
                style={[styles.qualityTab, isSelected && styles.qualityTabActive]}
              >
                <Text style={[styles.qualityTabLabel, isSelected && styles.qualityTabLabelActive]}>
                  {p.label}
                </Text>
                <Text style={[styles.qualityTabDetail, isSelected && styles.qualityTabDetailActive]}>
                  {p.detail}
                </Text>
              </Pressable>
            );
          })}
        </View>
      </View>

      {/* Collapsible Advanced Settings (Encoder Experiments & UDP Stability) */}
      {hasAdvancedOptions ? (
        <Pressable
          style={styles.advancedToggleRow}
          onPress={() => setManuallyToggled(!showAdvanced)}
          accessibilityRole="button"
          accessibilityLabel={t.viewer.advancedSettingsToggle}
        >
          <View style={styles.advancedToggleLeft}>
            <Ionicons name="options-outline" size={14} color={colors.textMuted} />
            <Text style={styles.advancedToggleText}>{t.viewer.advancedSettingsToggle}</Text>
            {udpReconnectRequired ? <View style={styles.reconnectDot} /> : null}
          </View>
          <Ionicons
            name={showAdvanced ? "chevron-up" : "chevron-down"}
            size={14}
            color={colors.textMuted}
          />
        </Pressable>
      ) : null}

      {showAdvanced && hasAdvancedOptions ? (
        <View style={styles.advancedSectionContainer}>
          <View
            style={{
              gap: 10,
              borderRadius: 12,
              borderWidth: 1,
              borderColor: colors.borderSubtle,
              backgroundColor: colors.bgSurface,
              padding: 12,
            }}
          >
            <View style={{ gap: 2 }}>
              <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>
                시청 옵션
              </Text>
              <Text style={{ fontSize: 11, lineHeight: 15, color: colors.textMuted }}>
                스트림 창에 실제 렌더링 FPS를 항상 표시할지 선택합니다.
              </Text>
            </View>
            <View
              style={{
                flexDirection: "row",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 12,
              }}
            >
              <View style={{ flex: 1, gap: 2 }}>
                <Text style={{ fontSize: 12, fontWeight: "700", color: colors.textPrimary }}>
                  실제 FPS 항상 표시
                </Text>
                <Text style={{ fontSize: 11, lineHeight: 15, color: colors.textSecondary }}>
                  새로 여는 스트림 창의 오른쪽 아래에 표시합니다.
                </Text>
              </View>
              <Switch
                value={showFps}
                onValueChange={onToggleFps}
                accessibilityLabel="실제 FPS 항상 표시"
                trackColor={{ false: colors.borderCard, true: colors.btnPrimaryBg }}
                thumbColor={colors.btnPrimaryText}
              />
            </View>
          </View>

          {encoderExperiments.length > 1 ? (
            <View
              style={{
                gap: 10,
                borderRadius: 12,
                borderWidth: 1,
                borderColor: colors.borderSubtle,
                backgroundColor: colors.bgSurface,
                padding: 12,
              }}
            >
              <View style={{ gap: 2 }}>
                <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>
                  {t.viewer.encoderExperiments}
                </Text>
                {requiresReconnect ? (
                  <Text style={{ fontSize: 11, color: colors.textMuted, lineHeight: 15 }}>
                    {t.viewer.encoderReconnectNotice}
                  </Text>
                ) : null}
              </View>
              <View style={{ gap: 8 }}>
                {encoderExperiments.map((experiment) => {
                  const isSelected = experiment.id === encoderExperiment;
                  return (
                    <Pressable
                      key={experiment.id}
                      style={{
                        minHeight: 44,
                        gap: 3,
                        borderRadius: 8,
                        borderWidth: 1,
                        borderColor: isSelected ? colors.btnPrimaryBg : colors.borderSubtle,
                        backgroundColor: isSelected ? colors.btnPrimaryBg : colors.bgSubtle,
                        paddingHorizontal: 12,
                        paddingVertical: 8,
                      }}
                      onPress={() => onSelectEncoderExperiment(experiment.id)}
                      accessibilityRole="button"
                      accessibilityState={{ selected: isSelected, disabled: false }}
                      accessibilityLabel={`${experiment.label}: ${experiment.hint}`}
                    >
                      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                        <Text
                          style={{
                            fontSize: 13,
                            fontWeight: "700",
                            color: isSelected ? colors.btnPrimaryText : colors.textPrimary,
                          }}
                        >
                          {experiment.label}
                        </Text>
                      </View>
                      <Text
                        style={{
                          fontSize: 11,
                          lineHeight: 15,
                          color: isSelected ? colors.btnPrimaryText : colors.textSecondary,
                          opacity: isSelected ? 0.85 : 1,
                        }}
                      >
                        {experiment.hint}
                      </Text>
                    </Pressable>
                  );
                })}
              </View>
            </View>
          ) : null}

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
  const [state, setState] = useState<UsbAccessoryState>({ attached: false, controlPort: 0 });

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

  if (!state.attached && !state.permissionPending && !state.accessoryPresent) {
    return null;
  }

  return (
    <View style={styles.transportStrip}>
      <View style={[styles.transportDot, state.attached && styles.transportDotUsb]} />
      <Text style={styles.transportText}>
        {state.attached
          ? t.viewer.usbAttached
          : state.permissionPending
            ? t.viewer.usbPending
            : t.viewer.usbDetected}
      </Text>
    </View>
  );
}

interface DisplayListItemProps {
  display: DisplayInfo;
  disabled: boolean;
  isLaunching: boolean;
  profile: StreamProfile;
  onOpen: (display: DisplayInfo) => void;
  styles: ReturnType<typeof createCatalogStyles>;
  colors: ThemeTokens;
}

function DisplayListItem({
  display,
  disabled,
  isLaunching,
  profile,
  onOpen,
  styles,
  colors,
}: DisplayListItemProps) {
  const { t } = useAppLanguage();
  const size = fitProfileToDisplay(display, profile);
  const recommendedId = recommendedStreamProfileId(display);
  const recommendedProfile = STREAM_PROFILES.find((candidate) => candidate.id === recommendedId);
  const handlePress = useCallback(() => onOpen(display), [display, onOpen]);
  return (
    <Pressable
      style={({ pressed }) => [
        styles.displayCard,
        pressed && !disabled && styles.itemPressed,
      ]}
      onPress={handlePress}
      disabled={disabled}
    >
      <DisplayAspectMiniature width={display.width} height={display.height} colors={colors} />

      <View style={styles.displayMain}>
        <Text style={styles.displayName} numberOfLines={1}>
          {display.name}
        </Text>
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
        {recommendedProfile ? (
          <Text style={styles.displayRecommendation} numberOfLines={1}>
            추천: {recommendedProfile.label}
            {profile.id === recommendedId ? " · 현재 선택과 일치" : ""}
          </Text>
        ) : null}
      </View>

      <View style={[styles.openBtn, isLaunching && styles.btnDisabled]}>
        {isLaunching ? (
          <ActivityIndicator color={colors.btnPrimaryText} size="small" />
        ) : (
          <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
            <Text style={styles.openBtnText}>{t.common.open}</Text>
            <Ionicons name="arrow-forward" size={12} color={colors.btnPrimaryText} />
          </View>
        )}
      </View>
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

function ActiveStreamItem({
  stream,
  onStop,
  styles,
}: {
  stream: ActiveStream;
  onStop: (stream: ActiveStream) => void;
  styles: ReturnType<typeof createCatalogStyles>;
}) {
  const { t } = useAppLanguage();
  const handleStop = useCallback(() => onStop(stream), [onStop, stream]);
  return (
    <View style={styles.streamCard}>
      <View style={styles.streamInfo}>
        <View style={styles.streamNameRow}>
          <View style={styles.dotActive} />
          <Text style={styles.streamName} numberOfLines={1}>
            #{stream.session} {stream.sourceName}
          </Text>
        </View>
        <Text style={styles.streamPort} numberOfLines={1}>
          {stream.width} × {stream.height} · {stream.fps} FPS
        </Text>
      </View>
      <Pressable style={styles.stopBtn} onPress={handleStop}>
        <Text style={styles.stopBtnText}>{t.common.stop}</Text>
      </Pressable>
    </View>
  );
}

function CatalogFooter({
  streams,
  onStop,
  styles,
}: {
  streams: ActiveStream[];
  onStop: (stream: ActiveStream) => void;
  styles: ReturnType<typeof createCatalogStyles>;
}) {
  const { t } = useAppLanguage();
  if (streams.length === 0) return null;
  return (
    <View style={styles.activeSection}>
      <View style={styles.activeSectionHeader}>
        <Text style={styles.activeSectionTitle}>{t.viewer.activeStreamsSection}</Text>
        <View style={styles.activeCountBadge}>
          <Text style={styles.activeCountText}>{streams.length}</Text>
        </View>
      </View>
      {streams.map((stream) => (
        <ActiveStreamItem key={stream.session} stream={stream} onStop={onStop} styles={styles} />
      ))}
    </View>
  );
}

export default function Catalog() {
  const { colors, isDark } = useAppTheme();
  const styles = useMemo(() => createCatalogStyles(colors, isDark), [colors, isDark]);
  const model = useCatalogModel();

  const renderDisplay = useCallback(
    ({ item }: ListRenderItemInfo<DisplayInfo>) => (
      <DisplayListItem
        display={item}
        disabled={model.launchingIndex !== null}
        isLaunching={model.launchingIndex === item.index}
        profile={model.selectedProfile}
        onOpen={model.openDisplay}
        styles={styles}
        colors={colors}
      />
    ),
    [colors, model.launchingIndex, model.openDisplay, model.selectedProfile, styles],
  );

  return (
    <SafeAreaView style={styles.safeArea} edges={["left", "right", "bottom"]}>
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
          <CatalogHeader
            error={model.visibleError}
            host={model.host || "localhost:7777"}
            loading={model.loading}
            profileId={model.profileId}
            refreshing={model.refreshing}
            onRefresh={model.handleRefresh}
            onSelectProfile={model.handleSelectProfile}
            showFps={model.showFps}
            onToggleFps={model.handleToggleFps}
            encoderExperiments={model.selectedEncoderExperiments}
            encoderExperiment={model.effectiveNextEncoderExperiment}
            onSelectEncoderExperiment={model.handleSelectEncoderExperiment}
            udpStabilityOptions={model.udpStabilityOptions}
            udpStability={model.effectiveUdpStability}
            udpReconnectRequired={model.udpSettingsDirty && model.streams.length > 0}
            udpReconnecting={model.udpReconnecting}
            onSelectUdpStability={model.handleSelectUdpStability}
            onApplyUdpStability={model.handleApplyUdpStability}
            styles={styles}
            colors={colors}
          />
        }
        data={model.displays}
        keyExtractor={displayKey}
        renderItem={renderDisplay}
        ListEmptyComponent={<EmptyDisplayList loading={model.loading} styles={styles} colors={colors} />}
        ListFooterComponent={
          <CatalogFooter streams={model.streams} onStop={model.stopStream} styles={styles} />
        }
      />
    </SafeAreaView>
  );
}
