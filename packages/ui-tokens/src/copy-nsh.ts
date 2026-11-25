import { translations, type TranslationSchema } from "./i18n";

const keys = {
  common: ["close", "languageTitle", "open", "refresh", "retry", "stop"] satisfies readonly (keyof TranslationSchema["common"])[],
  viewer: [
    "activeStreamsSection", "btnSettings", "connectedHostLabel", "currentlyStreaming", "displaysSectionTitle",
    "emptyDisplays", "errGeneric", "errHostTimeout", "navCatalog", "openingScreen", "primaryDisplay", "searchingDisplays",
    "secondaryDisplay", "settingsTitle", "standbyBadge", "streamStopFailed",
  ] satisfies readonly (keyof TranslationSchema["viewer"])[],
};

export function renderNshCopy(): string {
  const copy: Record<"ko" | "en", Record<string, Record<string, string>>> = { ko: {}, en: {} };
  for (const [group, groupKeys] of Object.entries(keys)) {
    for (const language of ["ko", "en"] as const) {
      copy[language][group] = Object.fromEntries(
        groupKeys.map((key) => [key, (translations[language][group as keyof TranslationSchema] as Record<string, string>)[key]]),
      );
    }
  }
  return `export const copy = ${JSON.stringify(copy, null, 2)} as const;\n`;
}
