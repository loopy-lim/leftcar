import { Fragment } from "react";
import { Text, View } from "react-native";
import type { TranslationSchema } from "../i18n";
import type { HubStyles } from "./hub-styles";

export interface FeatureCardsGridProps {
  styles: HubStyles;
  t: TranslationSchema;
}

/* 정의형 행 2줄 — 균일 카드 그리드(design.md G2) 대신 제목 + 한 줄 설명을
   헤어라인 구분선으로 나열한다. */
export function FeatureCardsGrid({ styles, t }: FeatureCardsGridProps) {
  const features = [
    [t.viewer.feature1Title, t.viewer.feature1Desc],
    [t.viewer.feature2Title, t.viewer.feature2Desc],
  ] as const;

  return (
    <View style={styles.featureList}>
      {features.map(([title, desc], index) => (
        <Fragment key={title}>
          {index > 0 && <View style={styles.featureDivider} />}
          <View style={styles.featureRow}>
            <Text style={styles.featureRowTitle}>{title}</Text>
            <Text style={styles.featureRowDesc}>{desc}</Text>
          </View>
        </Fragment>
      ))}
    </View>
  );
}

export default FeatureCardsGrid;
