import { useMemo, useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import {
  displaySizePresets,
  normalizeCustomSize,
} from "./display-size";
import {
  WINDOW_ASPECT_RATIO_PRESETS,
  type WindowAspectRatioPresetId,
} from "./window-aspect-ratio";
import type { ActiveStream } from "./catalog-model-types";
import type { ThemeTokens } from "./theme";

/**
 * "화면 해상도" card: shows the current session resolution, offers preset
 * candidates (1080p/1440p/4K), and a manual pixel input. Applies through the
 * model's session reconfigure path.
 */

export interface DisplaySizeCardProps {
  stream: ActiveStream | null;
  resizing: boolean;
  colors: ThemeTokens;
  onResizeSession: (
    stream: ActiveStream,
    width: number,
    height: number,
    fps: number,
  ) => Promise<boolean>;
  /** Currently applied XR window ratio preset id, when known. */
  windowRatio?: WindowAspectRatioPresetId | null;
  /** Selects an XR window ratio preset; failures are ignored upstream. */
  onSelectWindowRatio?: (presetId: WindowAspectRatioPresetId) => void;
}

const MIN_WIDTH = 640;
const MIN_HEIGHT = 480;
const MAX_WIDTH = 4096;
const MAX_HEIGHT = 4096;

function parseDimension(raw: string, fallback: number): number {
  const parsed = Number.parseInt(raw.trim(), 10);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function PresetButton({
  label,
  detail,
  active,
  disabled,
  colors,
  onPress,
}: {
  label: string;
  detail: string;
  active: boolean;
  disabled: boolean;
  colors: ThemeTokens;
  onPress: () => void;
}) {
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`${label} ${detail}`}
      accessibilityState={{ selected: active, disabled }}
      style={{
        flexBasis: "31%",
        flexGrow: 1,
        gap: 2,
        borderRadius: 8,
        borderWidth: 1,
        borderColor: active ? colors.btnPrimaryBg : colors.borderSubtle,
        backgroundColor: active ? colors.btnPrimaryBg : colors.bgSubtle,
        paddingHorizontal: 10,
        paddingVertical: 10,
      }}
      disabled={disabled}
      onPress={onPress}
    >
      <Text
        style={{
          fontSize: 13,
          fontWeight: "700",
          color: active ? colors.btnPrimaryText : colors.textPrimary,
        }}
      >
        {label}
      </Text>
      <Text
        style={{
          fontSize: 11,
          lineHeight: 15,
          color: active ? colors.btnPrimaryText : colors.textSecondary,
          opacity: active ? 0.85 : 1,
        }}
      >
        {detail}
      </Text>
    </Pressable>
  );
}

export function DisplaySizeCard({
  stream,
  resizing,
  colors,
  onResizeSession,
  windowRatio = null,
  onSelectWindowRatio,
}: DisplaySizeCardProps) {
  const [customWidth, setCustomWidth] = useState("");
  const [customHeight, setCustomHeight] = useState("");
  const [customError, setCustomError] = useState<string | null>(null);

  const currentWidth = stream?.activeTarget.width ?? null;
  const currentHeight = stream?.activeTarget.height ?? null;
  const presets = useMemo(
    () =>
      displaySizePresets(
        currentWidth !== null && currentHeight !== null
          ? { width: currentWidth, height: currentHeight }
          : null,
      ),
    [currentWidth, currentHeight],
  );

  if (!stream) return null;

  const applyPreset = (width: number, height: number) => {
    if (resizing || !stream) return;
    void onResizeSession(stream, width, height, stream.fps);
  };

  const applyCustom = () => {
    if (resizing || !stream) return;
    const width = parseDimension(customWidth, Number.NaN);
    const height = parseDimension(customHeight, Number.NaN);
    const normalized = normalizeCustomSize(width, height);
    if (!normalized) {
      setCustomError(
        `${MIN_WIDTH}×${MIN_HEIGHT}px 이상 ${MAX_WIDTH}×${MAX_HEIGHT}px 이하의 짝수 크기를 입력해 주세요.`,
      );
      return;
    }
    setCustomError(null);
    void onResizeSession(stream, normalized.width, normalized.height, stream.fps);
  };

  const styles = StyleSheet.create({
    card: {
      gap: 10,
      borderRadius: 12,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      backgroundColor: colors.bgSurface,
      padding: 12,
    },
    presetRow: {
      flexDirection: "row",
      flexWrap: "wrap",
      gap: 6,
    },
    inputRow: {
      flexDirection: "row",
      alignItems: "center",
      gap: 6,
    },
    input: {
      flex: 1,
      borderRadius: 8,
      borderWidth: 1,
      borderColor: colors.borderSubtle,
      backgroundColor: colors.bgSubtle,
      paddingHorizontal: 10,
      paddingVertical: 8,
      fontSize: 13,
      color: colors.textPrimary,
    },
    applyButton: {
      flexDirection: "row",
      alignItems: "center",
      justifyContent: "center",
      gap: 8,
      borderRadius: 8,
      backgroundColor: resizing ? colors.borderStrong : colors.btnPrimaryBg,
      paddingVertical: 12,
      paddingHorizontal: 12,
    },
  });

  return (
    <View style={styles.card}>
      <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>
        화면 해상도
      </Text>

      <Text
        style={{ fontSize: 12, fontWeight: "600", color: colors.textSecondary }}
        accessibilityLabel={`현재 크기 ${stream.activeTarget.width} 곱하기 ${stream.activeTarget.height}`}
      >
        현재 {stream.activeTarget.width} × {stream.activeTarget.height} · {stream.fps} FPS
      </Text>

      <View style={styles.presetRow}>
        {presets.map((preset) => (
          <PresetButton
            key={`${preset.label}-${preset.width}x${preset.height}`}
            label={preset.label}
            detail={`${preset.width} × ${preset.height}`}
            active={
              stream.activeTarget.width === preset.width &&
              stream.activeTarget.height === preset.height
            }
            disabled={resizing}
            colors={colors}
            onPress={() => applyPreset(preset.width, preset.height)}
          />
        ))}
      </View>

      <View style={{ gap: 4 }}>
        <Text
          style={{ fontSize: 12, fontWeight: "600", color: colors.textSecondary }}
        >
          화면 비율
        </Text>
        <Text style={{ fontSize: 11, lineHeight: 15, color: colors.textMuted }}>
          창의 모양만 바꾸고 컴퓨터 해상도는 유지합니다.
        </Text>
        <View style={styles.presetRow}>
          {WINDOW_ASPECT_RATIO_PRESETS.map((preset) => (
            <PresetButton
              key={preset.id}
              label={preset.label}
              detail={preset.ratio < 1 ? "세로" : "가로"}
              active={windowRatio === preset.id}
              disabled={resizing || !onSelectWindowRatio}
              colors={colors}
              onPress={() => onSelectWindowRatio?.(preset.id)}
            />
          ))}
        </View>
      </View>

      <View style={styles.inputRow}>
        <TextInput
          style={styles.input}
          placeholder={`가로 (${MIN_WIDTH}~${MAX_WIDTH})`}
          placeholderTextColor={colors.textMuted}
          keyboardType="number-pad"
          value={customWidth}
          editable={!resizing}
          onChangeText={setCustomWidth}
          accessibilityLabel="사용자 지정 가로 크기 (픽셀)"
        />
        <Text style={{ fontSize: 13, color: colors.textMuted }}>×</Text>
        <TextInput
          style={styles.input}
          placeholder={`세로 (${MIN_HEIGHT}~${MAX_HEIGHT})`}
          placeholderTextColor={colors.textMuted}
          keyboardType="number-pad"
          value={customHeight}
          editable={!resizing}
          onChangeText={setCustomHeight}
          accessibilityLabel="사용자 지정 세로 크기 (픽셀)"
        />
      </View>
      {customError ? (
        <Text
          style={{ fontSize: 11, lineHeight: 15, color: colors.textPrimary }}
          accessibilityLiveRegion="polite"
        >
          {customError}
        </Text>
      ) : null}

      <Pressable
        accessibilityRole="button"
        accessibilityLabel="크기 적용"
        accessibilityState={{ disabled: resizing }}
        style={styles.applyButton}
        disabled={resizing}
        onPress={applyCustom}
      >
        {resizing ? <ActivityIndicator color={colors.btnPrimaryText} size="small" /> : null}
        <Text style={{ fontSize: 13, fontWeight: "700", color: colors.btnPrimaryText }}>
          {resizing ? "적용하는 중…" : "직접 입력 적용"}
        </Text>
      </Pressable>
    </View>
  );
}
