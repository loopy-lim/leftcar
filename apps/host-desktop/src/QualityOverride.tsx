import { useState } from "react";
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
  const [draftPercent, setDraftPercent] = useState<number | null>(null);
  const language =
    propLanguage ??
    (typeof localStorage !== "undefined" &&
    localStorage.getItem("leftcar_lang") === "en"
      ? "en"
      : "ko");
  const t = getTranslation(language).host.inspector;
  const { qualityBasis } = encoderDiagnosticsView(session, language);
  const activePercent = draftPercent ?? qualityPercent;
  const commit = (nextPercent: number) => {
    setDraftPercent(null);
    if (nextPercent !== qualityPercent) {
      void onSetQuality(session, nextPercent / 100);
    }
  };
  return (
    <div className="space-y-2" aria-busy={qualityBusy}>
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <Text variant="caption" className="font-semibold">
          {t.manualQualityCeilingLabel}
        </Text>
        <Text variant="code" tone="muted">
          {session.qualityOverride != null || draftPercent !== null
            ? interpolate(t.fixedQualityPercent, { percent: activePercent })
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
          value={Math.max(25, Math.min(50, activePercent))}
          disabled={
            !qualitySupported || session.state !== "running" || qualityBusy || blocked
          }
          aria-label={t.manualQualityCeilingLabel}
          aria-valuetext={interpolate(t.fixedQualityPercent, {
            percent: activePercent,
          })}
          className="h-11 min-w-28 flex-1 cursor-pointer accent-action focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus disabled:cursor-default"
          onChange={(event) =>
            setDraftPercent(Number(event.currentTarget.value))
          }
          onPointerUp={(event) => commit(Number(event.currentTarget.value))}
          onKeyUp={(event) => {
            if (
              [
                "ArrowLeft",
                "ArrowRight",
                "ArrowUp",
                "ArrowDown",
                "Home",
                "End",
                "PageUp",
                "PageDown",
              ].includes(event.key)
            ) {
              commit(Number(event.currentTarget.value));
            }
          }}
          onBlur={(event) => {
            if (draftPercent !== null) {
              commit(Number(event.currentTarget.value));
            }
          }}
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
