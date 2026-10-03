import { getTranslation, type SupportedLanguage } from "@leftcar/ui-tokens";
import SessionEncoderDiagnostics from "./SessionEncoderDiagnostics";
import SessionPipelineDiagnostics from "./SessionPipelineDiagnostics";
import type { SessionRow } from "./sessionTypes";
interface SessionInspectorProps {
  session: SessionRow;
  transportLabel: string;
  language?: SupportedLanguage;
}
export default function SessionInspector({
  session,
  transportLabel,
  language: propLanguage,
}: SessionInspectorProps) {
  const language =
    propLanguage ??
    (typeof localStorage !== "undefined" &&
    localStorage.getItem("leftcar_lang") === "en"
      ? "en"
      : "ko");
  const t = getTranslation(language).host.inspector;
  return (
    <div className="space-y-4">
      <section className="space-y-3" aria-label={t.pipelineMetricsTitle}>
        <h4 className="text-caption text-muted font-semibold">
          {t.pipelineMetricsTitle}
        </h4>
        <div className="grid grid-cols-2 gap-3 min-[768px]:grid-cols-3">
          <SessionPipelineDiagnostics
            session={session}
            transportLabel={transportLabel}
            language={language}
          />
        </div>
      </section>
      <section
        className="space-y-3 border-t border-outline pt-4"
        aria-label={t.encoderControlTitle}
      >
        <h4 className="text-caption text-muted font-semibold">
          {t.encoderControlTitle}
        </h4>
        <div className="grid grid-cols-2 gap-3 min-[768px]:grid-cols-3">
          <SessionEncoderDiagnostics session={session} language={language} />
        </div>
      </section>
    </div>
  );
}
