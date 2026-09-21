import { Globe, HelpCircle, Laptop, Monitor, Moon, Settings, Sun } from "lucide-react";
import {
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import { buttonVariants, statusPillVariants } from "../lib/variants";

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
}: DashboardHeaderProps) {
  const ThemeIcon = { light: Sun, dark: Moon, system: Laptop }[themeMode];

  return (
    <header className="host-header">
      {/* Sleek, minimal left side: Compact logo + living status badge (no tacky large h1) */}
      <div className="host-header-left">
        <div className="host-logo-box" title="Leftcar">
          <Monitor size={15} strokeWidth={2.4} aria-hidden="true" />
        </div>
        <div className={statusPillVariants({ state: isStreaming ? "active" : "idle" })}>
          <span className="status-dot" />
          <span>
            {isStreaming
              ? interpolate(t.host.statusStreaming, { count: sessionCount })
              : t.host.statusIdle}
          </span>
        </div>
      </div>

      {/* Clean, unified right side: Essential utility controls only */}
      <div className="host-header-right">
        <button
          className={buttonVariants({ variant: "icon" })}
          onClick={onToggleLanguage}
          title={t.common.toggleLanguage}
          aria-label={t.common.toggleLanguage}
          style={{ display: "inline-flex", alignItems: "center", gap: 4, padding: "0 8px", width: "auto" }}
        >
          <Globe size={13} />
          <span style={{ fontSize: 12, fontWeight: 700 }}>{language === "ko" ? "EN" : "한국어"}</span>
        </button>

        <button
          className={buttonVariants({ variant: "icon" })}
          onClick={onTheme}
          title={`${t.common.theme}: ${themeLabel}`}
          aria-label={`${t.common.theme}: ${themeLabel}`}
        >
          <ThemeIcon size={15} />
        </button>

        <button
          className={buttonVariants({ variant: "icon" })}
          onClick={onHelp}
          title={`${t.host.btnHelp} (${t.host.shortcutHelp})`}
          aria-label={t.host.btnHelp}
        >
          <HelpCircle size={15} />
        </button>

        <button
          className={buttonVariants({ variant: "icon" })}
          onClick={onOpenSettings}
          title={`${t.host.settingsTitle} (${t.host.shortcutSettings})`}
          aria-label={t.host.settingsTitle}
        >
          <Settings size={15} />
        </button>
      </div>
    </header>
  );
}

export default DashboardHeader;
