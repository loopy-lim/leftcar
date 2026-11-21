import { Globe, HelpCircle, Laptop, Moon, QrCode, Settings, Sun } from "lucide-react";
import {
  cn,
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
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
  onOpenPairing?: () => void;
}
export function DashboardHeader({
  isStreaming,
  sessionCount,
  themeMode,
  themeLabel,
  language,
  t,
  onHelp,
  onTheme,
  onToggleLanguage,
  onOpenSettings,
  onOpenPairing,
}: DashboardHeaderProps) {
  const ThemeIcon = { light: Sun, dark: Moon, system: Laptop }[themeMode];
  const statusText = isStreaming
    ? interpolate(t.host.statusStreaming, { count: sessionCount })
    : t.host.statusIdle;
  return (
    <header className="flex min-h-14 shrink-0 items-center justify-between gap-3 border-b border-outline bg-surface px-4 py-2">
      <div className="flex min-w-0 items-center gap-3">
        <h1 className="text-title text-ink shrink-0 font-semibold">Leftcar</h1>
        <div className="flex min-w-0 items-center gap-1.5" role="status">
          <span
            aria-hidden="true"
            className={cn(
              "h-2 w-2 shrink-0 rounded-full",
              isStreaming ? "bg-live" : "bg-strong",
            )}
          />
          <Text variant="caption" tone="muted" className="truncate">
            {statusText}
          </Text>
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-1">
        {isStreaming && onOpenPairing && (
          <Button
            variant="secondary"
            size="compact"
            onClick={onOpenPairing}
            title={`${t.host.btnManageDevices} (${t.host.shortcutPair})`}
          >
            <QrCode size={16} />
            {t.host.btnManageDevices}
          </Button>
        )}
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
