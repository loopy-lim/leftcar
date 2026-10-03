import {
  getTranslation,
  interpolate,
  type SupportedLanguage,
} from "@leftcar/ui-tokens";
import { encoderDiagnosticsView } from "./encoderDiagnostics";
import { Button, Text } from "./ui/primitives";
import type { SessionRow } from "./sessionTypes";
interface Props {
  session: SessionRow;
  qualitySupported: boolean;
  qualityPercent: number;
  qualityBusy: boolean;
  blocked?: boolean;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
  language?: SupportedLanguage;
}
export default function QualityOverride({
  session,
  qualitySupported,
  qualityPercent,
  qualityBusy,
  blocked = false,
  onSetQuality,
  language: propLanguage,
}: Props) {
  const language =
    propLanguage ??
    (typeof localStorage !== "undefined" &&
    localStorage.getItem("leftcar_lang") === "en"
      ? "en"
      : "ko");
  const t = getTranslation(language).host.inspector;
  const { qualityBasis } = encoderDiagnosticsView(session, language);
  return (
    <div className="space-y-2" aria-busy={qualityBusy}>
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <Text variant="caption" className="font-semibold">
          {t.manualQualityCeilingLabel}
        </Text>
        <Text variant="code" tone="muted">
          {session.qualityOverride != null
            ? interpolate(t.fixedQualityPercent, { percent: qualityPercent })
            : t.auto}
          {qualityBasis !== null ? ` · ${qualityBasis}` : null}
        </Text>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <Text variant="caption" tone="muted">
          {t.lowEndpoint}
        </Text>
        <input
          type="range"
          min="25"
          max="50"
          step="5"
          value={Math.max(25, Math.min(50, qualityPercent))}
          disabled={
            !qualitySupported || session.state !== "running" || qualityBusy || blocked
          }
          aria-label={t.manualQualityCeilingLabel}
          aria-valuetext={interpolate(t.fixedQualityPercent, {
            percent: qualityPercent,
          })}
          className="h-11 min-w-28 flex-1 accent-action focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
          onChange={(event) =>
            void onSetQuality(session, Number(event.currentTarget.value) / 100)
          }
        />
        <Text variant="caption" tone="muted">
          {t.defaultEndpoint}
        </Text>
        <Button
          variant="secondary"
          size="compact"
          disabled={
            !qualitySupported || session.qualityOverride == null || qualityBusy || blocked
          }
          onClick={() => void onSetQuality(session, null)}
        >
          {t.autoRevertButton}
        </Button>
      </div>
      <Text variant="caption" tone="muted" className="block">
        {t.qualityOverrideHelp}
      </Text>
      {qualityBusy && <Text variant="caption" tone="muted" role="status" className="block">{t.qualityApplying}</Text>}
    </div>
  );
}
