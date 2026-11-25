import { Storage, locale, signal } from "nsh";
import { copy } from "./copy";

export type Language = keyof typeof copy;

const STORAGE_KEY = "leftcar.language";

export const language = signal<Language>(locale().toLowerCase().startsWith("en") ? "en" : "ko");
let userChoseLanguage = false;

export function restoreStoredLanguageUnlessChosen(read: () => Promise<string | null>) {
  read()
    .then((stored) => {
      if (!userChoseLanguage && (stored === "ko" || stored === "en")) language.set(stored);
    })
    .catch(() => undefined);
}

restoreStoredLanguageUnlessChosen(() => Storage.getItem(STORAGE_KEY));

export function t() {
  return copy[language()];
}

export function setLanguage(next: Language) {
  userChoseLanguage = true;
  language.set(next);
  Storage.setItem(STORAGE_KEY, next).catch(() => undefined);
}
