import { useMemo, useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from "react-native";
import { displaySizePresets, normalizeCustomSize } from "./display-size";
import type { ActiveStream } from "./catalog-model-types";
import type { ThemeTokens } from "./theme";

/**
 * "가상 화면 크기" card: shows the current session size, offers preset
 * candidates (tablet match when metrics are known, 1080p/1440p/4K), and a
 * manual pixel input. Applies through the model's resize handlers, falling
 * back to a session-only resolution change while the host-side managed
 * display listing is unavailable.
 */

export interface DisplaySizeCardProps {
  stream: ActiveStream | null;
  tabletMatch: { width: number; height: number } | null;
  resizing: boolean;
  /** Managed virtual display id, when the viewer knows one. Task 8 wiring. */
  virtualDisplayId?: string | null;
  colors: ThemeTokens;
  onResizeVirtualDisplay: (
    stream: ActiveStream,
    virtualDisplayId: string,
    width: number,
    height: number,
    scale: 1 | 2,
    fps: number,
  ) => Promise<boolean>;
  onResizeSession: (
    stream: ActiveStream,
    width: number,
    height: number,
    fps: number,
  ) => Promise<boolean>;
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
  tabletMatch,
  resizing,
  virtualDisplayId,
  colors,
  onResizeVirtualDisplay,
  onResizeSession,
}: DisplaySizeCardProps) {
  const [customWidth, setCustomWidth] = useState("");
  const [customHeight, setCustomHeight] = useState("");
  const [customError, setCustomError] = useState<string | null>(null);

  const currentWidth = stream?.activeTarget.width ?? null;
  const currentHeight = stream?.activeTarget.height ?? null;
  const presets = useMemo(
    () =>
      displaySizePresets(
        tabletMatch,
        currentWidth !== null && currentHeight !== null
          ? { width: currentWidth, height: currentHeight, scale: 1 as const }
          : null,
      ),
    [tabletMatch, currentWidth, currentHeight],
  );

  if (!stream) return null;

  const applyPreset = (width: number, height: number, scale: 1 | 2) => {
    if (resizing || !stream) return;
    if (virtualDisplayId) {
      void onResizeVirtualDisplay(stream, virtualDisplayId, width, height, scale, stream.fps);
      return;
    }
    // No managed-display id yet: change the session resolution only.
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
    if (virtualDisplayId) {
      void onResizeVirtualDisplay(
        stream,
        virtualDisplayId,
        normalized.width,
        normalized.height,
        1,
        stream.fps,
      );
      return;
    }
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
      <View style={{ gap: 2 }}>
        <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>
          가상 화면 크기
        </Text>
        <Text style={{ fontSize: 11, lineHeight: 15, color: colors.textMuted }}>
          {virtualDisplayId
            ? "컴퓨터의 가상 화면 크기를 바꾸고 열린 창에 바로 적용합니다."
            : "열린 화면 창의 해상도를 바로 바꿉니다. 가상 화면까지 바꾸려면 호스트 목록 조회가 필요합니다."}
        </Text>
      </View>

      <Text
        style={{ fontSize: 12, fontWeight: "600", color: colors.textSecondary }}
        accessibilityLabel={`현재 크기 ${stream.activeTarget.width} 곱하기 ${stream.activeTarget.height}`}
      >
        현재 {stream.activeTarget.width} × {stream.activeTarget.height} · {stream.fps} FPS
      </Text>

      <View style={styles.presetRow}>
        {presets.map((preset) => {
          const active =
            stream.activeTarget.width === preset.width &&
            stream.activeTarget.height === preset.height;
          return (
            <PresetButton
              key={`${preset.label}-${preset.width}x${preset.height}`}
              label={preset.label}
              detail={`${preset.width} × ${preset.height}${preset.scale === 2 ? " · 2x" : ""}`}
              active={active}
              disabled={resizing}
              colors={colors}
              onPress={() => applyPreset(preset.width, preset.height, preset.scale)}
            />
          );
        })}
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
