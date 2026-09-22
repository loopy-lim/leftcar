import { useState } from "react";
import { ActivityIndicator, Pressable, StyleSheet, Text, TextInput, View } from "react-native";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import type { ExtendedDisplayMode, ExtendedDisplayPosition, ExtendedDisplayStatus } from "./control";
import type { ThemeTokens } from "./theme";

type Props = {
  t: TranslationSchema; colors: ThemeTokens; exists: boolean; pending: boolean;
  busy: boolean; configurable: boolean; status?: ExtendedDisplayStatus;
  onOpen: (mode?: ExtendedDisplayMode) => void;
  onRemove: () => void;
  onResize: (mode: ExtendedDisplayMode) => void;
  onArrange: (position: ExtendedDisplayPosition) => void;
};
function Action({ label, onPress, disabled, colors }: {
  label: string; onPress: () => void; disabled: boolean; colors: ThemeTokens;
}) {
  return <Pressable accessibilityRole="button" accessibilityLabel={label} accessibilityState={{ disabled }}
    disabled={disabled} onPress={onPress} style={[styles.action, { borderColor: colors.borderSubtle, opacity: disabled ? 0.5 : 1 }]}>
    <Text style={{ color: colors.textPrimary }}>{label}</Text>
  </Pressable>;
}
function validDisplayMode(mode: { width: number; height: number } | undefined): boolean {
  return Boolean(mode && Number.isInteger(mode.width) && Number.isInteger(mode.height)
    && mode.width >= 640 && mode.width <= 4096 && mode.width % 2 === 0
    && mode.height >= 480 && mode.height <= 4096 && mode.height % 2 === 0);
}

function ExtensionSettings({ t, colors, status, exists, busy, onOpen, onResize, onArrange, onRemove }: Props) {
  const initial = status?.live;
  const [width, setWidth] = useState(String(initial?.logicalWidth ?? status?.suggested?.width ?? 1280));
  const [height, setHeight] = useState(String(initial?.logicalHeight ?? status?.suggested?.height ?? 800));
  const [scale, setScale] = useState(initial?.scale ?? 2);
  const mode = { width: Number(width), height: Number(height), scale };
  const valid = validDisplayMode(mode);
  return <View style={styles.settings}>
    {exists && <>
      <Text style={{ color: colors.textPrimary }}>{t.host.extDisplayPosition}</Text>
      <View style={styles.row}>{([
        ["left", t.host.extDisplayLeft], ["right", t.host.extDisplayRight],
        ["above", t.host.extDisplayAbove], ["below", t.host.extDisplayBelow],
      ] as const).map(([position, label]) => <Action key={position} label={label} colors={colors} disabled={busy} onPress={() => onArrange(position)} />)}</View>
    </>}
    <Text style={{ color: colors.textSecondary }}>{t.host.extDisplaySizeHint}</Text>
    <View style={styles.row}>
      {([ [1280, 800], [1440, 900], [1600, 1000] ] as const).map(([w, h]) => <Action key={w} label={`${w}×${h}`} colors={colors} disabled={busy}
        onPress={() => { setWidth(String(w)); setHeight(String(h)); setScale(2); }} />)}
    </View>
    <View style={styles.row}>
      <TextInput style={[styles.input, { color: colors.textPrimary, borderColor: colors.borderSubtle }]}
        accessibilityLabel={t.host.extDisplayWidth} keyboardType="number-pad" maxLength={4} value={width} editable={!busy} onChangeText={setWidth} />
      <Text style={{ color: colors.textPrimary }}>×</Text>
      <TextInput style={[styles.input, { color: colors.textPrimary, borderColor: colors.borderSubtle }]}
        accessibilityLabel={t.host.extDisplayHeight} keyboardType="number-pad" maxLength={4} value={height} editable={!busy} onChangeText={setHeight} />
      <Action label={scale === 2 ? "Retina (2×)" : "1×"} colors={colors} disabled={busy} onPress={() => setScale(scale === 2 ? 1 : 2)} />
    </View>
    <Action label={exists ? t.viewer.extApplySize : t.viewer.extCreateButton} colors={colors} disabled={busy || !valid}
      onPress={() => { if (valid) { if (exists) onResize(mode); else onOpen(mode); } }} />
    {exists && <Action label={t.viewer.extRemoveButton} colors={colors} disabled={busy} onPress={onRemove} />}
  </View>;
}
export function ExtensionDisplayCard(props: Props) {
  const { t, colors, exists, pending, busy, configurable, onOpen, status } = props;
  const [settings, setSettings] = useState(false);
  return <View style={[styles.card, { backgroundColor: colors.bgSubtle, borderColor: colors.borderSubtle }]}>
    <View style={styles.row}>
      {busy || pending ? <ActivityIndicator color={colors.textPrimary} /> : null}
      <Action label={pending ? t.viewer.extRemovingLabel : exists ? t.viewer.extOpenButton : t.viewer.extCreateButton}
        colors={colors} disabled={busy || pending} onPress={() => onOpen()} />
      {configurable && !pending && <Action label={t.viewer.extSettingsButton} colors={colors} disabled={busy} onPress={() => setSettings(!settings)} />}
    </View>
    <Text style={{ color: colors.textSecondary }}>{t.viewer.extKeepHint}</Text>
    {settings && !pending && <ExtensionSettings key={`${status?.live?.sourceId ?? 'new'}:${status?.live?.logicalWidth}:${status?.live?.logicalHeight}:${status?.live?.scale}`} {...props} />}
    {exists && !configurable && !pending && <Action label={t.viewer.extRemoveButton} colors={colors} disabled={busy} onPress={props.onRemove} />}
  </View>;
}
const styles = StyleSheet.create({
  card: { borderWidth: 1, borderRadius: 12, padding: 12, marginVertical: 8, gap: 10 },
  row: { flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 },
  action: { minHeight: 44, borderWidth: 1, borderRadius: 8, paddingHorizontal: 12, paddingVertical: 10, justifyContent: "center" },
  settings: { gap: 10, paddingTop: 8 },
  input: { width: 88, minHeight: 44, borderWidth: 1, borderRadius: 8, paddingHorizontal: 10 },
});
