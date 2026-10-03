import { useState } from "react";
import { View } from "react-native";
import { cn, type TranslationSchema } from "@leftcar/ui-tokens";
import type {
  ExtendedDisplayMode,
  ExtendedDisplayPosition,
  ExtendedDisplayStatus,
} from "./control";
import type { ThemeTokens } from "./theme";
import { Action, Field, Label, Notice, Surface } from "./ui/primitives";

type Props = {
  t: TranslationSchema;
  colors: ThemeTokens;
  exists: boolean;
  pending: boolean;
  busy: boolean;
  operation?: "open" | "remove" | "resize" | "arrange" | null;
  configurable: boolean;
  status?: ExtendedDisplayStatus;
  onOpen: (mode?: ExtendedDisplayMode) => void;
  onRemove: () => void;
  onResize: (mode: ExtendedDisplayMode) => void;
  onArrange: (position: ExtendedDisplayPosition) => void;
};
function ExtensionSettings({
  t,
  status,
  exists,
  busy,
  onOpen,
  onResize,
  onArrange,
  onRemove,
}: Props) {
  const initial = status?.live;
  const [width, setWidth] = useState(
    String(initial?.logicalWidth ?? status?.suggested?.width ?? 1280),
  );
  const [height, setHeight] = useState(
    String(initial?.logicalHeight ?? status?.suggested?.height ?? 800),
  );
  const [scale, setScale] = useState(initial?.scale ?? 2);
  const [invalid, setInvalid] = useState(false);
  const mode = { width: Number(width), height: Number(height), scale };
  const apply = () => {
    const valid =
      Number.isInteger(mode.width) &&
      Number.isInteger(mode.height) &&
      mode.width >= 640 &&
      mode.width <= 4096 &&
      mode.width % 2 === 0 &&
      mode.height >= 480 &&
      mode.height <= 4096 &&
      mode.height % 2 === 0;
    setInvalid(!valid);
    if (valid) {
      if (exists) onResize(mode);
      else onOpen(mode);
    }
  };
  return (
    <View className="gap-4">
      {exists ? (
        <View className="gap-2">
          <Label>{t.host.extDisplayPosition}</Label>
          <View className="flex-row flex-wrap gap-2">
            {(
              [
                ["left", t.host.extDisplayLeft],
                ["right", t.host.extDisplayRight],
                ["above", t.host.extDisplayAbove],
                ["below", t.host.extDisplayBelow],
              ] as const
            ).map(([position, label]) => (
              <Action
                key={position}
                variant="secondary"
                className="grow"
                label={label}
                disabled={busy}
                onPress={() => onArrange(position)}
              />
            ))}
          </View>
        </View>
      ) : null}
      <Label tone="muted">{t.host.extDisplaySizeHint}</Label>
      <View className="flex-row flex-wrap gap-2">
        {(
          [
            [1280, 800],
            [1440, 900],
            [1600, 1000],
          ] as const
        ).map(([w, h]) => (
          <Action
            key={w}
            variant="secondary"
            className={cn(
              "grow",
              mode.width === w &&
                mode.height === h &&
                "border-strong bg-active",
            )}
            label={`${w}×${h}`}
            disabled={busy}
            accessibilityState={{
              selected: mode.width === w && mode.height === h,
            }}
            onPress={() => {
              setWidth(String(w));
              setHeight(String(h));
              setScale(2);
              setInvalid(false);
            }}
          />
        ))}
      </View>
      <View className="flex-row items-center gap-2">
        <View className="min-w-0 flex-1 gap-1">
          <Label variant="caption">{t.host.extDisplayWidth}</Label>
          <Field
            accessibilityLabel={t.host.extDisplayWidth}
            keyboardType="number-pad"
            value={width}
            editable={!busy}
            invalid={invalid}
            onChangeText={(value) => {
              setWidth(value);
              setInvalid(false);
            }}
          />
        </View>
        <Label>×</Label>
        <View className="min-w-0 flex-1 gap-1">
          <Label variant="caption">{t.host.extDisplayHeight}</Label>
          <Field
            accessibilityLabel={t.host.extDisplayHeight}
            keyboardType="number-pad"
            value={height}
            editable={!busy}
            invalid={invalid}
            onChangeText={(value) => {
              setHeight(value);
              setInvalid(false);
            }}
          />
        </View>
      </View>
      <Action
        variant="secondary"
        label={scale === 2 ? "Retina (2×)" : "1×"}
        accessibilityLabel={t.host.extDisplayScale}
        accessibilityState={{ selected: scale === 2 }}
        disabled={busy}
        onPress={() => setScale(scale === 2 ? 1 : 2)}
      />
      {invalid ? (
        <Notice tone="error">
          <Label>{t.viewer.extensionSizeError}</Label>
        </Notice>
      ) : null}
      <Action
        variant="secondary"
        label={exists ? t.viewer.extApplySize : t.viewer.extCreateButton}
        disabled={busy}
        onPress={apply}
      />
      {exists ? (
        <Action
          variant="ghost"
          label={t.viewer.extRemoveButton}
          disabled={busy}
          onPress={onRemove}
        />
      ) : null}
    </View>
  );
}
function extensionActionLabel({ operation, pending, busy, exists, t }: Props) {
  if (pending || operation === "remove") return t.viewer.extRemovingLabel;
  if (operation === "resize") return t.viewer.applyingSize;
  if (operation === "arrange") return t.viewer.extArranging;
  if (busy) return t.viewer.openingScreen;
  return exists ? t.viewer.extOpenButton : t.viewer.extCreateButton;
}
export function ExtensionDisplayCard(props: Props) {
  const { t, exists, pending, busy, configurable, status } = props;
  const [settings, setSettings] = useState(false);
  return (
    <Surface variant="card" className="gap-3">
      <Label variant="title">{t.host.extDisplaySection}</Label>
      <Action
        variant="secondary"
        busy={busy || pending}
        label={extensionActionLabel(props)}
        onPress={() => props.onOpen()}
      />
      <Label variant="caption" tone="muted">
        {t.viewer.extKeepHint}
      </Label>
      {configurable && !pending ? (
        <Action
          variant="ghost"
          label={t.viewer.extSettingsButton}
          disabled={busy}
          accessibilityState={{ expanded: settings }}
          onPress={() => setSettings(!settings)}
        />
      ) : null}
      {settings && !pending ? (
        <ExtensionSettings
          key={`${status?.live?.sourceId ?? "new"}:${status?.live?.logicalWidth}:${status?.live?.logicalHeight}:${status?.live?.scale}`}
          {...props}
        />
      ) : null}
      {exists && !configurable && !pending ? (
        <Action
          variant="ghost"
          label={t.viewer.extRemoveButton}
          disabled={busy}
          onPress={props.onRemove}
        />
      ) : null}
    </Surface>
  );
}
