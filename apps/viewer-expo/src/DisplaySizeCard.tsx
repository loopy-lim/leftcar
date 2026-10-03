import { useMemo, useState } from "react";
import { View } from "react-native";
import { cn } from "@leftcar/ui-tokens";
import { displaySizePresets, normalizeCustomSize } from "./display-size";
import type { ActiveStream } from "./catalog-model-types";
import type { ThemeTokens } from "./theme";
import { useAppLanguage } from "./i18n";
import { Action, Field, Label, Notice, Surface } from "./ui/primitives";

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
}
export function DisplaySizeCard({
  stream,
  resizing,
  onResizeSession,
}: DisplaySizeCardProps) {
  const { t, format } = useAppLanguage();
  const [width, setWidth] = useState(String(stream?.activeTarget.width ?? ""));
  const [height, setHeight] = useState(
    String(stream?.activeTarget.height ?? ""),
  );
  const [error, setError] = useState<string | null>(null);
  const [applied, setApplied] = useState(false);
  const presets = useMemo(
    () => displaySizePresets(stream?.activeTarget ?? null),
    [stream?.activeTarget],
  );
  if (!stream) return null;
  const apply = async () => {
    if (resizing) return;
    const size = normalizeCustomSize(
      Number(width.trim()),
      Number(height.trim()),
    );
    setApplied(false);
    if (!size) {
      setError(
        format(t.viewer.customSizeError, {
          min: "640×480px",
          max: "4096×4096px",
        }),
      );
      return;
    }
    setError(null);
    try {
      const accepted = await onResizeSession(
        stream,
        size.width,
        size.height,
        stream.fps,
      );
      if (accepted) setApplied(true);
      else setError(t.viewer.resolutionFailed);
    } catch (cause) {
      setError(String(cause instanceof Error ? cause.message : cause));
    }
  };
  const changeDimension = (setter: (value: string) => void, value: string) => {
    setter(value);
    setError(null);
    setApplied(false);
  };
  return (
    <Surface className="gap-4">
      <Label
        variant="code"
        tone="muted"
        accessibilityLabel={format(t.viewer.displaySizeA11y, {
          width: stream.activeTarget.width,
          height: stream.activeTarget.height,
        })}
      >
        {t.viewer.displaySizeCurrentPrefix} {stream.activeTarget.width} ×{" "}
        {stream.activeTarget.height} · {stream.activeTarget.fps} FPS
      </Label>
      <View className="flex-row flex-wrap gap-2">
        {presets.map((preset) => (
          <View
            key={`${preset.width}x${preset.height}`}
            className="min-w-32 grow gap-1"
          >
            <Action
              variant="secondary"
              label={preset.label}
              disabled={resizing}
              className={cn(
                Number(width) === preset.width &&
                  Number(height) === preset.height &&
                  "border-strong bg-active",
              )}
              accessibilityState={{
                selected:
                  Number(width) === preset.width &&
                  Number(height) === preset.height,
              }}
              accessibilityHint={`${preset.width} × ${preset.height}`}
              onPress={() => {
                setWidth(String(preset.width));
                setHeight(String(preset.height));
                setError(null);
                setApplied(false);
              }}
            />
            <Label variant="code" tone="muted" className="text-center">
              {preset.width} × {preset.height}
            </Label>
          </View>
        ))}
      </View>
      <Label tone="muted">{t.viewer.resolutionApplyHint}</Label>
      <View className="flex-row items-center gap-2">
        <View className="min-w-0 flex-1 gap-1">
          <Label variant="caption">{t.viewer.customWidthA11y}</Label>
          <Field
            value={width}
            invalid={Boolean(error)}
            editable={!resizing}
            onChangeText={(value) => changeDimension(setWidth, value)}
            keyboardType="number-pad"
            accessibilityLabel={t.viewer.customWidthA11y}
          />
        </View>
        <Label tone="muted">×</Label>
        <View className="min-w-0 flex-1 gap-1">
          <Label variant="caption">{t.viewer.customHeightA11y}</Label>
          <Field
            value={height}
            invalid={Boolean(error)}
            editable={!resizing}
            onChangeText={(value) => changeDimension(setHeight, value)}
            keyboardType="number-pad"
            accessibilityLabel={t.viewer.customHeightA11y}
          />
        </View>
      </View>
      {error ? (
        <Notice tone="error">
          <Label>{error}</Label>
        </Notice>
      ) : applied ? (
        <Notice>
          <Label>{t.viewer.resolutionApplied}</Label>
        </Notice>
      ) : null}
      <Action
        busy={resizing}
        onPress={() => void apply()}
        label={resizing ? t.viewer.applyingSize : t.viewer.applySizeA11y}
      />
    </Surface>
  );
}
