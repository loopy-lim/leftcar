import {
  getTranslation,
  interpolate,
  type SupportedLanguage,
} from "@leftcar/ui-tokens";
import { DiagnosticMetric as Metric } from "./DiagnosticMetric";
import { encoderDiagnosticsView } from "./encoderDiagnostics";
import type { SessionRow } from "./sessionTypes";
export default function SessionEncoderDiagnostics({
  session,
  language,
}: {
  session: SessionRow;
  language: SupportedLanguage;
}) {
  const t = getTranslation(language).host.inspector;
  const diagnostics = encoderDiagnosticsView(session, language);
  return (
    <>
      <Metric label={t.encoderPathLabel}>
        {diagnostics.path} · {diagnostics.identity}
      </Metric>
      <Metric label={t.encoderConfigLabel}>
        {diagnostics.configuration} ·{" "}
        {interpolate(t.appliedValue, { value: diagnostics.applied })}
      </Metric>
      <Metric
        label={t.experimentProfileLabel}
        tone={diagnostics.hasExperimentFallback ? "warning" : "default"}
      >
        {diagnostics.experiment} · {diagnostics.experimentDetail} ·{" "}
        {interpolate(t.experimentFallbackValue, {
          value: diagnostics.experimentFallback,
        })}
      </Metric>
      <Metric
        label={t.encoderPressureLabel}
        tone={diagnostics.hasEncoderPressure ? "warning" : "default"}
      >
        {diagnostics.pressure}
      </Metric>
      <Metric label={t.inFlightLabel}>{diagnostics.inFlight}</Metric>
      <Metric label={t.validOutputFpsLabel}>
        {diagnostics.validOutputFps}
      </Metric>
      <Metric label={t.unsupportedRejectedLabel}>
        {diagnostics.unavailable}
      </Metric>
      {diagnostics.fallback !== null && (
        <Metric label={t.encoderFallbackLabel} tone="warning">
          {diagnostics.fallback}
        </Metric>
      )}
      <Metric label={t.qualityHintLabel}>
        {session.qualityHint != null
          ? `${session.qualityHint.toFixed(2)} · ${interpolate(t.changesCount, { count: session.qualityAdaptationChanges ?? t.measuring })} / ${interpolate(t.checksCount, { count: session.qualityAdaptationChecks ?? t.measuring })} · ${session.qualityAdaptationLastStatus ?? t.adaptationWaiting}`
          : t.qualityHintNotApplied}
      </Metric>
      <Metric label={t.captureBackendLabel}>
        {session.captureBackend || t.measuring}
      </Metric>
    </>
  );
}
