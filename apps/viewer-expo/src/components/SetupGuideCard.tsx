import { View } from "react-native";
import type { TranslationSchema } from "../i18n";
import { Label, Surface } from "../ui/primitives";

export interface SetupGuideCardProps {
  t: TranslationSchema;
}
export function SetupGuideCard({ t }: SetupGuideCardProps) {
  const steps = [
    [1, t.viewer.step1Title, t.viewer.step1Desc],
    [2, t.viewer.step2Title, t.viewer.step2Desc],
    [3, t.viewer.step3Title, t.viewer.step3Desc],
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
    </Surface>
  );
}
export default SetupGuideCard;
