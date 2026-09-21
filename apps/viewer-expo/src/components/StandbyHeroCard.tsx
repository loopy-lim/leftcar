import { ActivityIndicator, Pressable, Text, View } from "react-native";
import { Ionicons } from "@expo/vector-icons";
import type { TranslationSchema } from "../i18n";
import type { ThemeTokens } from "../theme";
import type { RecentHostItem } from "../recent-hosts";
import type { HubStyles } from "./hub-styles";
import { RecentHostQuickConnect } from "./RecentHostQuickConnect";

export interface StandbyHeroCardProps {
  lastHost: RecentHostItem | null;
  autoConnecting: boolean;
  colors: ThemeTokens;
  styles: HubStyles;
  t: TranslationSchema;
  onCheckConnection: () => void;
  onOpenHostPicker: () => void;
  onOpenPairing: () => void;
}

export function StandbyHeroCard({
  lastHost,
  autoConnecting,
  colors,
  styles,
  t,
  onCheckConnection,
  onOpenHostPicker,
  onOpenPairing,
}: StandbyHeroCardProps) {
  if (lastHost && !autoConnecting) {
    const hostDisplayName = lastHost.name || t.common.myComputer;

    return (
      <View style={styles.heroCardStandby}>
        {/* Device Identity Header (Apple Sidecar style: icon + name + live badge) */}
        <View style={styles.deviceHeaderRow}>
          <View style={styles.deviceIconBox}>
            <Ionicons name="desktop-outline" size={24} color={colors.textPrimary} />
          </View>
          <View style={styles.deviceInfoColumn}>
            <View style={styles.deviceNameRow}>
              <Text style={styles.heroTitle} numberOfLines={1}>
                {hostDisplayName}
              </Text>
              <View style={styles.badgeSuccess}>
                <View style={styles.dotSuccess} />
                <Text style={styles.badgeSuccessText}>{t.viewer.quickConnectAvailable}</Text>
              </View>
            </View>
            <Text style={styles.heroDesc}>{t.host.remoteReadyDesc}</Text>
          </View>
        </View>

        {/* Primary Action Button: Big, comfortable [▶ 지금 화면 열기] */}
        <RecentHostQuickConnect
          item={lastHost}
          styles={styles}
          onFinished={onCheckConnection}
        />

        {/* Subtle, balanced secondary actions */}
        <View style={styles.heroActionRow}>
          <Pressable
            onPress={onOpenHostPicker}
            style={({ pressed }) => [
              styles.secondaryActionBtn,
              { flex: 1 },
              pressed && styles.btnPressed,
            ]}
            accessibilityRole="button"
            accessibilityLabel={t.viewer.btnConnectOther}
          >
            <Text style={styles.secondaryActionText}>{t.viewer.btnConnectOther}</Text>
          </Pressable>
          <Pressable
            onPress={onOpenPairing}
            style={({ pressed }) => [
              styles.secondaryActionBtn,
              pressed && styles.btnPressed,
            ]}
            accessibilityRole="button"
            accessibilityLabel={t.viewer.btnQrConnect}
          >
            <Ionicons
              name="qr-code-outline"
              size={14}
              color={colors.textPrimary}
              style={{ marginRight: 5 }}
            />
            <Text style={styles.secondaryActionText}>{t.viewer.btnQrConnect}</Text>
          </Pressable>
        </View>
      </View>
    );
  }

  return (
    <View style={styles.heroCardStandby}>
      <View style={styles.deviceHeaderRow}>
        <View style={styles.deviceIconBox}>
          <Ionicons name="desktop-outline" size={24} color={colors.textPrimary} />
        </View>
        <View style={styles.deviceInfoColumn}>
          <View style={styles.deviceNameRow}>
            <Text style={styles.heroTitle}>
              {autoConnecting ? t.viewer.connectingToHost : t.viewer.standbyHeroTitle}
            </Text>
            <View style={styles.badgeStandby}>
              {autoConnecting ? (
                <ActivityIndicator size="small" color={colors.textSecondary} />
              ) : (
                <View style={styles.dotStandby} />
              )}
              <Text style={styles.badgeStandbyText}>
                {autoConnecting ? t.viewer.connectingToHost : t.viewer.standbyBadge}
              </Text>
            </View>
          </View>
          <Text style={styles.heroDesc}>{t.viewer.standbyHeroDesc}</Text>
        </View>
      </View>

      <View style={styles.heroActionRow}>
        <Pressable
          onPress={onOpenHostPicker}
          style={({ pressed }) => [
            styles.primaryActionBtn,
            { flex: 1 },
            pressed && styles.btnPressed,
          ]}
          accessibilityRole="button"
          accessibilityLabel={t.viewer.btnFindHost}
        >
          <Text style={styles.primaryActionText}>{t.viewer.btnFindHost}</Text>
        </Pressable>
        <Pressable
          onPress={onOpenPairing}
          style={({ pressed }) => [
            styles.secondaryActionBtn,
            pressed && styles.btnPressed,
          ]}
          accessibilityRole="button"
          accessibilityLabel={t.viewer.btnQrConnect}
        >
          <Ionicons
            name="qr-code-outline"
            size={15}
            color={colors.textPrimary}
            style={{ marginRight: 5 }}
          />
          <Text style={styles.secondaryActionText}>{t.viewer.btnQrConnect}</Text>
        </Pressable>
      </View>
    </View>
  );
}

export default StandbyHeroCard;
