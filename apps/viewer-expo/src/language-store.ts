import {
  getTranslation,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";

/**
 * React 바깥(세션/런처/오류 포맷터)에서도 현재 언어를 읽을 수 있게 하는
 * 모듈 수준 저장소. LanguageProvider가 마운트/변경 시 동기화하며, 초기값은
 * OS 로케일 감지(i18n resolvePersistedLanguage)와 같은 규칙을 따른다.
 */

let current: SupportedLanguage = detectDefaultLanguage();

function detectDefaultLanguage(): SupportedLanguage {
  try {
    const locale = typeof Intl !== "undefined"
      ? Intl.DateTimeFormat().resolvedOptions().locale
      : "ko";
    if (locale.toLowerCase().startsWith("en")) return "en";
    return "ko";
  } catch {
    return "ko";
  }
}

export function setCurrentLanguage(language: SupportedLanguage): void {
  current = language;
}

export function currentLanguage(): SupportedLanguage {
  return current;
}

export function currentTranslation(): TranslationSchema {
  return getTranslation(current);
}
