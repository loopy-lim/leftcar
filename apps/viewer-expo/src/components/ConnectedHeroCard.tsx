import { View } from "react-native";
import type { TranslationSchema } from "../i18n";
import { Action, Label, Surface } from "../ui/primitives";

export interface ConnectedHeroCardProps {
  hostAddr: string;
  hostName?: string;
  t: TranslationSchema;
  onOpenCatalog: () => void;
  onOpenHostPicker: () => void;
  onDisconnect: () => void;
}
export function ConnectedHeroCard({
  hostAddr,
  hostName,
  t,
  onOpenCatalog,
  onOpenHostPicker,
  onDisconnect,
}: ConnectedHeroCardProps) {
  return (
    <Surface variant="plain" className="gap-5 py-3">
      <View className="gap-1">
        <Label variant="heading">{hostName || t.common.myComputer}</Label>
        <Label variant="code" tone="muted" selectable>
          {hostAddr}
        </Label>
        <Label tone="muted">{t.viewer.connectedBadge}</Label>
      </View>
      <Action onPress={onOpenCatalog} label={t.viewer.btnViewDisplays} />
      <View className="flex-row flex-wrap gap-2">
        <Action
          variant="secondary"
          className="grow"
          onPress={onOpenHostPicker}
          label={t.viewer.btnChangeHost}
        />
        <Action
          variant="ghost"
          onPress={onDisconnect}
          label={t.common.disconnect}
        />
      </View>
    </Surface>
  );
}
export default ConnectedHeroCard;
