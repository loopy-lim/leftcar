import { ActivityIndicator, View } from "react-native";
import type { TranslationSchema } from "../i18n";
import type { RecentHostItem } from "../recent-hosts";
import { RecentHostQuickConnect } from "./RecentHostQuickConnect";
import { formatHostEndpoint } from "../pairing";
import { Action, Label, Notice, Surface } from "../ui/primitives";

export interface StandbyHeroCardProps {
  connectionSignal: AbortSignal | undefined;
  lastHost: RecentHostItem | null;
  autoConnecting: boolean;
  t: TranslationSchema;
  onCheckConnection: () => void;
  onOpenHostPicker: () => void;
  onOpenPairing: () => void;
  onCancelReconnect: () => void;
}
export function StandbyHeroCard({
  connectionSignal,
  lastHost,
  autoConnecting,
  t,
  onCheckConnection,
  onOpenHostPicker,
  onOpenPairing,
  onCancelReconnect,
}: StandbyHeroCardProps) {
  return (
    <Surface variant="plain" className="gap-5 py-3">
      <View className="gap-1">
        <Label variant="heading">
          {lastHost?.name || t.viewer.standbyHeroTitle}
        </Label>
        {lastHost ? (
          <Label variant="code" tone="muted" selectable>
            {formatHostEndpoint(lastHost.host, lastHost.port)}
          </Label>
        ) : (
          <Label tone="muted">{t.viewer.standbyHeroDesc}</Label>
        )}
        <View className="flex-row items-center gap-2">
          <View
            className="h-2 w-2 rounded-full bg-strong"
            accessibilityElementsHidden
            importantForAccessibility="no-hide-descendants"
          />
          <Label tone="muted">{t.viewer.standbyBadge}</Label>
        </View>
      </View>
      {autoConnecting ? (
        <Notice>
          <View className="flex-row items-center gap-3">
            <ActivityIndicator accessibilityElementsHidden />
            <Label className="flex-1" accessibilityLiveRegion="polite">
              {t.viewer.connectingToHost}
            </Label>
          </View>
          <Action
            variant="secondary"
            onPress={onCancelReconnect}
            label={t.common.cancel}
          />
        </Notice>
      ) : lastHost ? (
        <RecentHostQuickConnect
          signal={connectionSignal}
          item={lastHost}
          onFinished={onCheckConnection}
        />
      ) : (
        <Action onPress={onOpenHostPicker} label={t.viewer.btnFindHost} />
      )}
      <View className="flex-row flex-wrap gap-2">
        {lastHost ? (
          <Action
            variant="secondary"
            className="grow"
            onPress={onOpenHostPicker}
            label={t.viewer.btnConnectOther}
          />
        ) : null}
        <Action
          variant="secondary"
          className="grow"
          onPress={onOpenPairing}
          label={t.viewer.btnQrConnect}
        />
      </View>
    </Surface>
  );
}
export default StandbyHeroCard;
