import { Fragment } from "react";
import { Text, View } from "react-native";
import type { TranslationSchema } from "../i18n";
import type { HubStyles } from "./hub-styles";

export interface SetupGuideCardProps {
  styles: HubStyles;
  t: TranslationSchema;
}

export function SetupGuideCard({ styles, t }: SetupGuideCardProps) {
  const steps = [
    [1, t.viewer.step1Title, t.viewer.step1Desc],
    [2, t.viewer.step2Title, t.viewer.step2Desc],
    [3, t.viewer.step3Title, t.viewer.step3Desc],
  ] as const;

  return (
    <View style={styles.sectionCard}>
      <Text style={styles.sectionTitle}>{t.viewer.guideTitle}</Text>

      <View style={styles.stepsContainer}>
        {steps.map(([num, name, desc], index) => (
          <Fragment key={num}>
            <View style={styles.stepItem}>
              <View style={styles.stepBadge}>
                <Text style={styles.stepNum}>{num}</Text>
              </View>
              <View style={styles.stepInfo}>
                <Text style={styles.stepName}>{name}</Text>
                <Text style={styles.stepText}>{desc}</Text>
              </View>
            </View>
            {index < steps.length - 1 && <View style={styles.stepDivider} />}
          </Fragment>
        ))}
      </View>
    </View>
  );
}

export default SetupGuideCard;
