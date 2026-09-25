import type { ReactNode } from "react";
import {
  cn,
  getTranslation,
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import { diagnosticValueVariants } from "./diagnosticStyles";
import { encoderDiagnosticsView } from "./encoderDiagnostics";
import type { SessionRow } from "./sessionTypes";

type InspectorStrings = TranslationSchema["host"]["inspector"];

// 수동 화질 상한 제어(QualityOverride)는 세션 카드 본체로 승격되었다(DESIGN-REVIEW X-1).
// 인스펙터는 읽기 전용 텔레메트리만 남는다.
interface Props {
  session: SessionRow;
  language: SupportedLanguage;
}

function DiagnosticRow({ label, value, tone = "default" }: { label: string; value: ReactNode; tone?: "default" | "warning" | "active" }) {
  return <div className="inspector-item"><span className="inspector-item-label">{label}</span><span className={cn(diagnosticValueVariants({ tone }))}>{value}</span></div>;
}

export default function SessionEncoderDiagnostics({
  session,
  language,
}: Props) {
  const t = getTranslation(language).host.inspector;
  const diagnostics = encoderDiagnosticsView(session, language);
  const hasInFlightWork = [session.encodeInFlight, session.packetizationInFlight]
    .some((value) => typeof value === "number" && Number.isFinite(value) && value > 0);
  return (
    <>
      <div className="inspector-item">
        <span className="inspector-item-label">{t.encoderPathLabel}</span>
        <span className="inspector-item-value">{diagnostics.path} · {diagnostics.identity}</span>
      </div>
      <div className="inspector-item">
        <span className="inspector-item-label">{t.encoderConfigLabel}</span>
        <span className="inspector-item-value">{diagnostics.configuration} · {interpolate(t.appliedValue, { value: diagnostics.applied })}</span>
      </div>
      <div className="inspector-item">
        <span className="inspector-item-label">{t.experimentProfileLabel}</span>
        <span className={cn(diagnosticValueVariants({
          tone: diagnostics.hasExperimentFallback ? "warning" : "default",
        }))}>
          {diagnostics.experiment} · {diagnostics.experimentDetail} · {interpolate(t.experimentFallbackValue, { value: diagnostics.experimentFallback })}
        </span>
      </div>
      <DiagnosticRow label={t.encoderPressureLabel} value={diagnostics.pressure} tone={diagnostics.hasEncoderPressure ? "warning" : "default"} />
      <DiagnosticRow label={t.inFlightLabel} value={diagnostics.inFlight} tone={hasInFlightWork ? "active" : "default"} />
      <DiagnosticRow label={t.validOutputFpsLabel} value={diagnostics.validOutputFps} />
      <div className="inspector-item">
        <span className="inspector-item-label">{t.unsupportedRejectedLabel}</span>
        <span className="inspector-item-value">{diagnostics.unavailable}</span>
      </div>
      {diagnostics.fallback !== null ? (
        <div className="inspector-item">
          <span className="inspector-item-label">{t.encoderFallbackLabel}</span>
          <span className="inspector-item-value">{diagnostics.fallback}</span>
        </div>
      ) : null}
      <div className="inspector-item">
        <span className="inspector-item-label">{t.qualityHintLabel}</span>
        <span className="inspector-item-value">
          {session.qualityHint != null
            ? `${session.qualityHint.toFixed(2)} · ${interpolate(t.changesCount, { count: session.qualityAdaptationChanges ?? 0 })} / ${interpolate(t.checksCount, { count: session.qualityAdaptationChecks ?? 0 })} · ${session.qualityAdaptationLastStatus ?? t.adaptationWaiting}`
            : t.qualityHintNotApplied}
        </span>
      </div>
      <div className="inspector-item">
        <span className="inspector-item-label">{t.captureBackendLabel}</span>
        <span className={cn("inspector-item-value", "capitalize")}>
          {session.captureBackend || "ScreenCaptureKit"}
        </span>
      </div>
    </>
  );
}
