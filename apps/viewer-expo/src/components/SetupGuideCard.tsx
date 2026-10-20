import { View } from "react-native";
import type { TranslationSchema } from "../i18n";
import { Action, Label, Surface } from "../ui/primitives";

export interface SetupGuideCardProps {
  t: TranslationSchema;
  onOpenHostPicker: () => void;
  onOpenPairing: () => void;
}
export function SetupGuideCard({
  t,
  onOpenHostPicker,
  onOpenPairing,
}: SetupGuideCardProps) {
  const steps = [
    [1, t.viewer.step1Title, t.viewer.step1Desc],
    [2, t.viewer.step2Title, t.viewer.step2Desc],
  ] as const;
  return (
    <Surface className="gap-4">
      <Label variant="title">{t.viewer.guideTitle}</Label>
      {steps.map(([number, title, description]) => (
        <View key={number} className="flex-row gap-3">
          <Label variant="code" tone="muted">
            {number}.
          </Label>
          <View className="flex-1 gap-1">
            <Label>{title}</Label>
            <Label variant="caption" tone="muted">
              {description}
            </Label>
          </View>
        </View>
      ))}
      {/* 3단계는 안내 텍스트가 아니라 행동이다 — "맨 처음에 뭘 해야 하는지"
          라는 첫 실행의 핵심 질문에 버튼으로 바로 답한다. 카드는 최초 페어링
          전(!lastHost)에만 표시되므로 연결이 완료되면 스스로 사라진다. */}
      <View className="flex-row gap-3">
        <Label variant="code" tone="muted">
          3.
        </Label>
        <View className="flex-1 gap-2">
          <Label>{t.viewer.step3Title}</Label>
          <Label variant="caption" tone="muted">
            {t.viewer.step3Desc}
          </Label>
          <View className="flex-row gap-2">
            <Action
              variant="primary"
              label={t.viewer.btnFindHost}
              onPress={onOpenHostPicker}
            />
            <Action
              variant="secondary"
              label={t.viewer.btnQrConnect}
              onPress={onOpenPairing}
            />
          </View>
        </View>
      </View>
    </Surface>
  );
}
export default SetupGuideCard;
