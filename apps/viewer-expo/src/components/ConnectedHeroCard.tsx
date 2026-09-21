import { Pressable, Text, View } from "react-native";
import { Ionicons } from "@expo/vector-icons";
import type { TranslationSchema } from "../i18n";
import type { HubStyles } from "./hub-styles";

export interface ConnectedHeroCardProps {
  hostAddr: string;
  styles: HubStyles;
  t: TranslationSchema;
  onOpenCatalog: () => void;
  onOpenHostPicker: () => void;
  onDisconnect: () => void;
}

export function ConnectedHeroCard({
  hostAddr,
  styles,
  t,
  onOpenCatalog,
  onOpenHostPicker,
  onDisconnect,
}: ConnectedHeroCardProps) {
  return (
    <View style={styles.heroCardConnected}>
      <View style={styles.deviceHeaderRow}>
        <View style={styles.deviceIconBox}>
          <Ionicons name="desktop-outline" size={24} color="#10b981" />
        </View>
        <View style={styles.deviceInfoColumn}>
          <View style={styles.deviceNameRow}>
            <Text style={styles.heroTitle}>{t.viewer.connectedHeroTitle}</Text>
            <View style={styles.badgeSuccess}>
              <View style={styles.dotSuccess} />
              <Text style={styles.badgeSuccessText}>{t.viewer.connectedBadge}</Text>
            </View>
          </View>
          <Text style={styles.endpointLabel} numberOfLines={1}>
            {hostAddr}
          </Text>
        </View>
      </View>

      <Text style={styles.heroDesc}>{t.viewer.connectedHeroDesc}</Text>

      <View style={styles.heroActionRow}>
        <Pressable
          onPress={onOpenCatalog}
          style={({ pressed }) => [
            styles.primaryActionBtn,
            { flex: 1 },
            pressed && styles.btnPressed,
          ]}
          accessibilityRole="button"
          accessibilityLabel={t.viewer.btnViewDisplays}
        >
          <Text style={styles.primaryActionText}>{t.viewer.btnViewDisplays}</Text>
        </Pressable>
        <Pressable
          onPress={onOpenHostPicker}
          style={({ pressed }) => [
            styles.secondaryActionBtn,
            pressed && styles.btnPressed,
          ]}
          accessibilityRole="button"
          accessibilityLabel={t.viewer.btnChangeHost}
        >
          <Text style={styles.secondaryActionText}>{t.viewer.btnChangeHost}</Text>
        </Pressable>
        <Pressable
          onPress={onDisconnect}
          style={({ pressed }) => [
            styles.disconnectActionBtn,
            pressed && styles.btnPressed,
          ]}
          accessibilityRole="button"
          accessibilityLabel={t.common.disconnect}
        >
          <Text style={styles.disconnectActionText}>{t.common.disconnect}</Text>
        </Pressable>
      </View>
    </View>
  );
}

export default ConnectedHeroCard;
