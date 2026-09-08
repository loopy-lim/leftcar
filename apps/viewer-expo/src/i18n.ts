import { createElement, createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import * as SecureStore from "expo-secure-store";
import {
  getTranslation,
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";

const LANGUAGE_KEY = "leftcar.language";

/**
 * 시작 언어 결정 규칙: 저장된 선택이 우선하고, 없으면 OS 로케일이 영어권인지로
 * 판단한다. 그 외 모든 로케일은 한국어(제품 기본)로 귀결된다.
 */
export function resolvePersistedLanguage(
  stored: string | null,
  locale: string,
): SupportedLanguage {
  if (stored === "ko" || stored === "en") return stored;
  return locale.toLowerCase().startsWith("en") ? "en" : "ko";
}

function detectInitialLanguage(): SupportedLanguage {
  try {
    const locale = typeof Intl !== "undefined"
      ? Intl.DateTimeFormat().resolvedOptions().locale
      : "ko";
    return resolvePersistedLanguage(null, locale);
  } catch {
    // fallback
    return "ko";
  }
}

interface LanguageContextValue {
  language: SupportedLanguage;
  t: TranslationSchema;
  setLanguage: (lang: SupportedLanguage) => void;
  toggleLanguage: () => void;
  format: (template: string, params?: Record<string, string | number>) => string;
}

const LanguageContext = createContext<LanguageContextValue | null>(null);

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [language, setLanguageState] = useState<SupportedLanguage>(detectInitialLanguage);

  // 저장된 선택이 있으면 감지된 로케일 값을 대체한다(마운트 후 1회).
  useEffect(() => {
    let active = true;
    void SecureStore.getItemAsync(LANGUAGE_KEY)
      .then((stored) => {
        if (active && (stored === "ko" || stored === "en")) {
          setLanguageState(stored);
        }
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);

  const persistLanguage = useCallback((lang: SupportedLanguage) => {
    void SecureStore.setItemAsync(LANGUAGE_KEY, lang).catch(() => undefined);
  }, []);

  const setLanguage = useCallback(
    (lang: SupportedLanguage) => {
      setLanguageState(lang);
      persistLanguage(lang);
    },
    [persistLanguage],
  );

  const toggleLanguage = useCallback(() => {
    setLanguageState((prev) => {
      const next = prev === "ko" ? "en" : "ko";
      persistLanguage(next);
      return next;
    });
  }, [persistLanguage]);

  const t = useMemo(() => getTranslation(language), [language]);

  const format = useCallback((template: string, params?: Record<string, string | number>) => {
    return interpolate(template, params);
  }, []);

  const value = useMemo<LanguageContextValue>(
    () => ({
      language,
      t,
      setLanguage,
      toggleLanguage,
      format,
    }),
    [language, t, setLanguage, toggleLanguage, format],
  );

  return createElement(LanguageContext.Provider, { value }, children);
}

export function useAppLanguage(): LanguageContextValue {
  const context = useContext(LanguageContext);
  if (!context) {
    // Fallback if rendered outside provider
    const defaultLang = detectInitialLanguage();
    return {
      language: defaultLang,
      t: getTranslation(defaultLang),
      setLanguage: () => undefined,
      toggleLanguage: () => undefined,
      format: interpolate,
    };
  }
  return context;
}

export { type SupportedLanguage, type TranslationSchema };
