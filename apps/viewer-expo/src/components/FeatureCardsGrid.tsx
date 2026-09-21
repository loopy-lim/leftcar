import { Text, View } from "react-native";
import { Ionicons } from "@expo/vector-icons";
import type { TranslationSchema } from "../i18n";
import type { ThemeTokens } from "../theme";
import type { HubStyles } from "./hub-styles";

export interface FeatureCardsGridProps {
  styles: HubStyles;
  colors: ThemeTokens;
  t: TranslationSchema;
}

export function FeatureCardsGrid({ styles, colors, t }: FeatureCardsGridProps) {
  return (
    <View style={styles.featureGrid}>
      <View style={styles.featureCard}>
        <View style={styles.featureIconBox}>
          <Ionicons name="speedometer-outline" size={16} color={colors.textPrimary} />
        </View>
        <Text style={styles.featureValue}>{t.viewer.feature1Title}</Text>
        <Text style={styles.featureLabel}>{t.viewer.feature1Desc}</Text>
      </View>
      <View style={styles.featureCard}>
        <View style={styles.featureIconBox}>
          <Ionicons name="copy-outline" size={16} color={colors.textPrimary} />
        </View>
        <Text style={styles.featureValue}>{t.viewer.feature2Title}</Text>
        <Text style={styles.featureLabel}>{t.viewer.feature2Desc}</Text>
      </View>
    </View>
  );
}

export default FeatureCardsGrid;
