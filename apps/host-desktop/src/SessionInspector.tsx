import { getTranslation, type SupportedLanguage } from "@leftcar/ui-tokens";
import SessionEncoderDiagnostics from "./SessionEncoderDiagnostics";
import SessionPipelineDiagnostics from "./SessionPipelineDiagnostics";
import type { SessionRow } from "./sessionTypes";

interface SessionInspectorProps {
  session: SessionRow;
  transportLabel: string;
  qualitySupported: boolean;
  qualityPercent: number;
  qualityBusy: boolean;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
  language?: SupportedLanguage;
}

function inspectorLanguage(saved: string | null): SupportedLanguage {
  return saved === "en" ? "en" : "ko";
}

export default function SessionInspector({
  session,
  transportLabel,
  qualitySupported,
  qualityPercent,
  qualityBusy,
  onSetQuality,
  language: propLanguage,
}: SessionInspectorProps) {
  const saved = typeof localStorage === "undefined" ? null : localStorage.getItem("leftcar_lang");
  const language = propLanguage ?? inspectorLanguage(saved);
  const t = getTranslation(language).host.inspector;
  return (
    <div className="inspector-panel">
      <div className="inspector-group">
        <div className="inspector-group-header">
          <span className="inspector-header">{t.pipelineMetricsTitle}</span>
        </div>
        <div className="inspector-grid">
          <SessionPipelineDiagnostics
            session={session}
            transportLabel={transportLabel}
            language={language}
          />
        </div>
      </div>

      <div className="inspector-divider" />

      <div className="inspector-group">
        <div className="inspector-group-header">
          <span className="inspector-header">{t.encoderControlTitle}</span>
        </div>
        <div className="inspector-grid">
          <SessionEncoderDiagnostics
            session={session}
            qualitySupported={qualitySupported}
            qualityPercent={qualityPercent}
            qualityBusy={qualityBusy}
            onSetQuality={onSetQuality}
            language={language}
          />
        </div>
      </div>
    </div>
  );
}
