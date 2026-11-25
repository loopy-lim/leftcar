import { expect, test } from "nsh/test";
import { language, restoreStoredLanguageUnlessChosen, setLanguage } from "./i18n";

test("language: what was chosen before is taken back, as one of the two and unless the user chose meanwhile", async () => {
  language.set("en");
  restoreStoredLanguageUnlessChosen(() => Promise.resolve("fr"));
  await Promise.resolve();
  expect(language()).toBe("en");
  let stored!: (value: string | null) => void;
  restoreStoredLanguageUnlessChosen(() => new Promise<string | null>((resolve) => { stored = resolve; }));
  language.set("ko");
  stored("en");
  await Promise.resolve();
  expect(language()).toBe("en");
  restoreStoredLanguageUnlessChosen(() => new Promise<string | null>((resolve) => { stored = resolve; }));
  setLanguage("ko");
  stored("en");
  await Promise.resolve();
  expect(language()).toBe("ko");
});
