import { createElement, createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import {
  getTranslation,
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";

function detectInitialLanguage(): SupportedLanguage {
  try {
    const locale = typeof Intl !== "undefined"
      ? Intl.DateTimeFormat().resolvedOptions().locale
      : "ko";
    if (locale.toLowerCase().startsWith("en")) {
      return "en";
    }
  } catch {
    // fallback
  }
  return "ko";
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
  const [language, setLanguage] = useState<SupportedLanguage>(detectInitialLanguage);

  const toggleLanguage = useCallback(() => {
    setLanguage((prev) => (prev === "ko" ? "en" : "ko"));
  }, []);

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
    [language, t, toggleLanguage, format],
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
