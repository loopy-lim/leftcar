import { Globe, HelpCircle, Laptop, Moon, Settings, Sun } from "lucide-react";
import type { SupportedLanguage, TranslationSchema } from "@leftcar/ui-tokens";
import { Button, Text } from "../ui/primitives";
export type ThemeMode = "system" | "light" | "dark";
export interface DashboardHeaderProps {
  isStreaming: boolean;
  sessionCount: number;
  themeMode: ThemeMode;
  themeLabel: string;
  language: SupportedLanguage;
  t: TranslationSchema;
  onHelp: () => void;
  onTheme: () => void;
  onToggleLanguage: () => void;
  onOpenSettings: () => void;
}
export function DashboardHeader({
  themeMode,
  themeLabel,
  language,
  t,
  onHelp,
  onTheme,
  onToggleLanguage,
  onOpenSettings,
}: DashboardHeaderProps) {
  const ThemeIcon = { light: Sun, dark: Moon, system: Laptop }[themeMode];
  return (
    <header className="flex h-14 shrink-0 items-center justify-between gap-3 border-b border-outline bg-surface px-4">
      <h1 className="text-title text-ink font-semibold">Leftcar</h1>
      <div className="flex shrink-0 items-center gap-1">
        <Button
          variant="ghost"
          onClick={onToggleLanguage}
          title={t.common.toggleLanguage}
          aria-label={t.common.toggleLanguage}
        >
          <Globe size={16} />
          <Text variant="caption" className="font-semibold">
            {language === "ko" ? "EN" : "한국어"}
          </Text>
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={onTheme}
          title={`${t.common.theme}: ${themeLabel}`}
          aria-label={`${t.common.theme}: ${themeLabel}`}
        >
          <ThemeIcon size={16} />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={onHelp}
          title={`${t.host.btnHelp} (${t.host.shortcutHelp})`}
          aria-label={t.host.btnHelp}
        >
          <HelpCircle size={16} />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={onOpenSettings}
          title={`${t.host.settingsTitle} (${t.host.shortcutSettings})`}
          aria-label={t.host.settingsTitle}
        >
          <Settings size={16} />
        </Button>
      </div>
    </header>
  );
}
export default DashboardHeader;
