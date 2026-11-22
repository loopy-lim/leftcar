import { View } from "react-native";
import { cn } from "@leftcar/ui-tokens";
import { Action, Label, Notice, Surface } from "./ui/primitives";
import type {
  UdpStabilityOptions,
  UdpStabilityProfileId,
  UdpStabilitySelection,
} from "./udp-stability";
import { useAppLanguage, type TranslationSchema } from "./i18n";

type ViewerTextKey = keyof TranslationSchema["viewer"];

// 2026-11-22 단순화: 프리셋 선택만 남긴다. 세부 수동 조정(버스트/FEC/적응
// 페이싱 개별 값, custom 진입)은 auto의 연결 적응 사다리가 대신하고,
// 'balanced'는 auto와 사실상 같은 값이라 제거했다.
const PRESET_COPY_KEYS: Record<
  UdpStabilityProfileId,
  { label: ViewerTextKey; hint: ViewerTextKey }
> = {
  auto: { label: "udpPresetAutoLabel", hint: "udpPresetAutoHint" },
  responsive: {
    label: "udpPresetLowLatencyLabel",
    hint: "udpPresetLowLatencyHint",
  },
  stable: { label: "udpPresetStableLabel", hint: "udpPresetStableHint" },
};

interface UdpStabilityControlsProps {
  options: UdpStabilityOptions | null;
  selection: UdpStabilitySelection;
  reconnectRequired: boolean;
  reconnecting: boolean;
  disabled?: boolean;
  onChange: (selection: UdpStabilitySelection) => void;
  onApplyReconnect: () => void;
}

function Choice({
  active,
  label,
  onPress,
  disabled = false,
}: {
  active: boolean;
  label: string;
  onPress: () => void;
  disabled?: boolean;
}) {
  return (
    <Action
      variant="secondary"
      size="compact"
      label={label}
      disabled={disabled}
      onPress={onPress}
      accessibilityState={{ selected: active }}
      className={cn(active && "border-strong bg-active")}
    />
  );
}

export function UdpStabilityControls({
  options,
  selection,
  reconnectRequired,
  reconnecting,
  disabled = false,
  onChange,
  onApplyReconnect,
}: UdpStabilityControlsProps) {
  const { t } = useAppLanguage();
  const controlsDisabled = reconnecting || disabled;

  if (!options) return null;

  return (
    <Surface className="gap-3">
      <Label variant="title">{t.viewer.udpStabilityTitle}</Label>
      {options.profiles.map((profile) => {
        const keys = PRESET_COPY_KEYS[profile];
        return (
          <View key={profile} className="gap-1">
            <Choice
              active={selection.profile === profile}
              label={t.viewer[keys.label]}
              disabled={controlsDisabled}
              onPress={() => onChange({ profile })}
            />
            <Label variant="caption" tone="muted">
              {t.viewer[keys.hint]}
            </Label>
          </View>
        );
      })}
      {reconnectRequired ? (
        <Notice>
          <Label>{t.viewer.encoderReconnectNotice}</Label>
          <Action
            busy={reconnecting}
            disabled={disabled}
            onPress={onApplyReconnect}
            label={
              reconnecting
                ? t.viewer.udpReconnecting
                : t.viewer.udpApplyReconnect
            }
          />
        </Notice>
      ) : null}
    </Surface>
  );
}
