import { useMemo, useState, type ReactNode } from "react";
import { ActivityIndicator, Pressable, StyleSheet, Text, View } from "react-native";
import type {
  UdpBurstDatagrams,
  UdpFecParityShards,
  UdpStabilityOptions,
  UdpStabilityProfileId,
  UdpStabilitySelection,
} from "./udp-stability";
import { useAppTheme, type ThemeTokens } from "./theme";

const PRESET_COPY: Record<
  Exclude<UdpStabilityProfileId, "custom">,
  { label: string; hint: string }
> = {
  auto: {
    label: "자동 (권장)",
    hint: "네트워크 상태에 맞게 손실 복구와 전송량을 자동 조절합니다.",
  },
  responsive: {
    label: "저지연",
    hint: "원활한 Wi-Fi 환경에서 지연 시간을 최소화합니다.",
  },
  balanced: {
    label: "표준",
    hint: "표준 전송 속도와 손실 복구를 균형 있게 유지합니다.",
  },
  stable: {
    label: "안정성 우선",
    hint: "신호가 약한 Wi-Fi에서 끊김과 프레임 드랍을 방지합니다.",
  },
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
  onChange: (selection: UdpStabilitySelection) => void;
  onApplyReconnect: () => void;
}

function Choice({
  active,
  label,
  onPress,
  colors,
}: {
  active: boolean;
  label: string;
  onPress: () => void;
  colors: ThemeTokens;
}) {
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ selected: active }}
      style={{
        borderRadius: 8,
        borderWidth: active ? 1 : 1,
        borderColor: active ? colors.btnPrimaryBg : colors.borderSubtle,
        backgroundColor: active ? colors.btnPrimaryBg : colors.bgSubtle,
        paddingHorizontal: 12,
        paddingVertical: 8,
      }}
      onPress={onPress}
    >
      <Text
        style={{
          fontSize: 12,
          fontWeight: active ? "700" : "600",
          color: active ? colors.btnPrimaryText : colors.textSecondary,
        }}
      >
        {label}
      </Text>
    </Pressable>
  );
}

function PresetChoices({ options, selection, colors, onChange }: { options: UdpStabilityOptions; selection: UdpStabilitySelection; colors: ThemeTokens; onChange: (selection: UdpStabilitySelection) => void }) {
  return <View style={{ gap: 8 }}>{options.profiles.map((profile) => {
    const copy = PRESET_COPY[profile];
    const active = selection.profile === profile;
    return <Pressable key={profile} accessibilityRole="button" accessibilityState={{ selected: active }} style={{ gap: 2, borderRadius: 8, borderWidth: 1, borderColor: active ? colors.btnPrimaryBg : colors.borderSubtle, backgroundColor: active ? colors.btnPrimaryBg : colors.bgSubtle, padding: 12 }} onPress={() => onChange({ profile })}>
      <Text style={{ fontSize: 13, fontWeight: "700", color: active ? colors.btnPrimaryText : colors.textPrimary }}>{copy.label}</Text>
      <Text style={{ fontSize: 11, lineHeight: 15, color: active ? colors.btnPrimaryText : colors.textSecondary, opacity: active ? 0.85 : 1 }}>{copy.hint}</Text>
    </Pressable>;
  })}</View>;
}

function DetailChoices({ options, effective, expanded, colors, onToggle, onChange }: { options: UdpStabilityOptions; effective: Required<Omit<UdpStabilitySelection, "profile">>; expanded: boolean; colors: ThemeTokens; onToggle: () => void; onChange: (next: Partial<{ burstDatagrams: UdpBurstDatagrams; fecParityShards: UdpFecParityShards; adaptivePacing: boolean }>) => void }) {
  const choices = (title: string, children: ReactNode) => <View style={{ gap: 6 }}><Text style={{ fontSize: 11, fontWeight: "600", color: colors.textMuted }}>{title}</Text><View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>{children}</View></View>;
  const detailsAvailable = options.burstDatagrams.length > 1 || options.fecParityShards.length > 1 || options.adaptivePacing;
  if (!detailsAvailable) return null;
  return <View style={{ gap: 10, borderTopWidth: 1, borderTopColor: colors.borderSubtle, paddingTop: 10 }}>
    <Pressable accessibilityRole="button" style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }} onPress={onToggle}><Text style={{ fontSize: 12, fontWeight: "700", color: colors.textSecondary }}>세부 설정</Text><Text style={{ fontSize: 12, fontWeight: "600", color: colors.textMuted }}>{expanded ? "접기" : "열기"}</Text></Pressable>
    {expanded ? <View style={{ gap: 10 }}>
      {choices("UDP 묶음 전송", options.burstDatagrams.map((burst) => <Choice key={burst} active={effective.burstDatagrams === burst} label={`${burst}개`} onPress={() => onChange({ burstDatagrams: burst })} colors={colors} />))}
      {choices("손실 복구 강도", options.fecParityShards.map((parity) => <Choice key={parity} active={effective.fecParityShards === parity} label={parity === 4 ? "강함 (4)" : "표준 (2)"} onPress={() => onChange({ fecParityShards: parity })} colors={colors} />))}
      {options.adaptivePacing ? choices("상태에 맞춰 자동 조절", <><Choice active={effective.adaptivePacing} label="켜기" onPress={() => onChange({ adaptivePacing: true })} colors={colors} /><Choice active={!effective.adaptivePacing} label="끄기" onPress={() => onChange({ adaptivePacing: false })} colors={colors} /></>) : null}
    </View> : null}
  </View>;
}

export function UdpStabilityControls({
  options,
  selection,
  reconnectRequired,
  reconnecting,
  onChange,
  onApplyReconnect,
}: UdpStabilityControlsProps) {
  const { colors } = useAppTheme();
  const [expanded, setExpanded] = useState(false);
  const effective = useMemo(() => {
    if (selection.profile === "custom") {
      return {
        burstDatagrams: selection.burstDatagrams ?? options?.burstDatagrams[0] ?? 4,
        fecParityShards: selection.fecParityShards ?? options?.fecParityShards[0] ?? 2,
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

  return (
    <View
      style={{
        gap: 12,
        borderRadius: 12,
        borderWidth: 1,
        borderColor: colors.borderSubtle,
        backgroundColor: colors.bgSurface,
        padding: 12,
      }}
    >
      <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>전송 안정성</Text>

      <PresetChoices options={options} selection={selection} colors={colors} onChange={onChange} />

      <DetailChoices options={options} effective={effective} expanded={expanded} colors={colors} onToggle={() => setExpanded((current) => !current)} onChange={selectCustom} />

      {reconnectRequired ? (
        <Pressable
          accessibilityRole="button"
          style={{
            flexDirection: "row",
            alignItems: "center",
            justifyContent: "center",
            gap: 8,
            borderRadius: 8,
            backgroundColor: reconnecting ? colors.borderStrong : colors.btnPrimaryBg,
            paddingVertical: 12,
            paddingHorizontal: 12,
          }}
          disabled={reconnecting}
          onPress={onApplyReconnect}
        >
          {reconnecting ? <ActivityIndicator color={colors.btnPrimaryText} size="small" /> : null}
          <Text style={{ fontSize: 13, fontWeight: "700", color: colors.btnPrimaryText }}>
            {reconnecting ? "다시 연결하는 중…" : "적용하고 다시 연결"}
          </Text>
        </Pressable>
      ) : null}
    </View>
  );
}
