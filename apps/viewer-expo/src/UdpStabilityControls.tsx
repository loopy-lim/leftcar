import { useMemo, useState } from "react";
import { View } from "react-native";
import { cn } from "@leftcar/ui-tokens";
import { Action, Label, Notice, Surface } from "./ui/primitives";
import type {
  UdpBurstDatagrams,
  UdpFecParityShards,
  UdpStabilityOptions,
  UdpStabilityProfileId,
  UdpStabilitySelection,
} from "./udp-stability";
import { useAppLanguage, type TranslationSchema } from "./i18n";

type ViewerTextKey = keyof TranslationSchema["viewer"];

const PRESET_COPY_KEYS: Record<
  Exclude<UdpStabilityProfileId, "custom">,
  { label: ViewerTextKey; hint: ViewerTextKey }
> = {
  auto: { label: "udpPresetAutoLabel", hint: "udpPresetAutoHint" },
  responsive: {
    label: "udpPresetLowLatencyLabel",
    hint: "udpPresetLowLatencyHint",
  },
  balanced: { label: "udpPresetStandardLabel", hint: "udpPresetStandardHint" },
  stable: { label: "udpPresetStableLabel", hint: "udpPresetStableHint" },
};

const PRESET_VALUES: Record<
  Exclude<UdpStabilityProfileId, "custom">,
  Required<Omit<UdpStabilitySelection, "profile">>
> = {
  auto: { burstDatagrams: 4, fecParityShards: 2, adaptivePacing: true },
  responsive: { burstDatagrams: 8, fecParityShards: 2, adaptivePacing: false },
  balanced: { burstDatagrams: 4, fecParityShards: 2, adaptivePacing: false },
  stable: { burstDatagrams: 2, fecParityShards: 4, adaptivePacing: false },
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
  const [expanded, setExpanded] = useState(false);
  const effective = useMemo(() => {
    if (selection.profile === "custom") {
      return {
        burstDatagrams:
          selection.burstDatagrams ?? options?.burstDatagrams[0] ?? 4,
        fecParityShards:
          selection.fecParityShards ?? options?.fecParityShards[0] ?? 2,
        adaptivePacing: selection.adaptivePacing ?? false,
      };
    }
    return PRESET_VALUES[selection.profile];
  }, [options, selection]);

  if (!options) return null;

  const selectCustom = (
    next: Partial<{
      burstDatagrams: UdpBurstDatagrams;
      fecParityShards: UdpFecParityShards;
      adaptivePacing: boolean;
    }>,
  ) => {
    onChange({ profile: "custom", ...effective, ...next });
  };

  const detailsAvailable =
    options.burstDatagrams.length > 1 ||
    options.fecParityShards.length > 1 ||
    options.adaptivePacing;
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
      {detailsAvailable ? (
        <Action
          variant="ghost"
          label={t.viewer.udpDetailToggle}
          accessibilityState={{ expanded }}
          onPress={() => setExpanded(!expanded)}
        />
      ) : null}
      {expanded ? (
        <View className="gap-3">
          <Label variant="caption" tone="muted">
            {t.viewer.udpBurstLabel}
          </Label>
          <View className="flex-row flex-wrap gap-2">
            {options.burstDatagrams.map((value) => (
              <Choice
                key={value}
                active={effective.burstDatagrams === value}
                label={String(value)}
                disabled={controlsDisabled}
                onPress={() => selectCustom({ burstDatagrams: value })}
              />
            ))}
          </View>
          <Label variant="caption" tone="muted">
            {t.viewer.udpFecLabel}
          </Label>
          <View className="flex-row flex-wrap gap-2">
            {options.fecParityShards.map((value) => (
              <Choice
                key={value}
                active={effective.fecParityShards === value}
                label={
                  value === 4 ? t.viewer.udpFecStrong : t.viewer.udpFecStandard
                }
                disabled={controlsDisabled}
                onPress={() => selectCustom({ fecParityShards: value })}
              />
            ))}
          </View>
          {options.adaptivePacing ? (
            <>
              <Label variant="caption" tone="muted">
                {t.viewer.udpAdaptiveLabel}
              </Label>
              <View className="flex-row gap-2">
                <Choice
                  active={effective.adaptivePacing}
                  label={t.viewer.udpOn}
                  disabled={controlsDisabled}
                  onPress={() => selectCustom({ adaptivePacing: true })}
                />
                <Choice
                  active={!effective.adaptivePacing}
                  label={t.viewer.udpOff}
                  disabled={controlsDisabled}
                  onPress={() => selectCustom({ adaptivePacing: false })}
                />
              </View>
            </>
          ) : null}
        </View>
      ) : null}
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
