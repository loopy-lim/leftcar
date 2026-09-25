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

// qualitySupported·qualityPercent·qualityBusy·onSetQuality는 카드 본체의
// QualityOverride(DESIGN-REVIEW X-1)가 사용한다 — 인스펙터는 읽기 전용 텔레메트리만
// 렌더링하므로 전달하지 않는다. 인터페이스는 호출측 호환을 위해 유지한다.
export default function SessionInspector({
  session,
  transportLabel,
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
            language={language}
          />
        </div>
      </div>
    </div>
  );
}
