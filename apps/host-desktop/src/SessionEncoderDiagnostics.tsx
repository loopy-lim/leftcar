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
import { inspectorButtonVariants } from "./lib/variants";
import type { SessionRow } from "./sessionTypes";

type InspectorStrings = TranslationSchema["host"]["inspector"];

interface Props {
  session: SessionRow;
  qualitySupported: boolean;
  qualityPercent: number;
  qualityBusy: boolean;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
  language: SupportedLanguage;
}

function DiagnosticRow({ label, value, tone = "default" }: { label: string; value: ReactNode; tone?: "default" | "warning" | "active" }) {
  return <div className="inspector-item"><span className="inspector-item-label">{label}</span><span className={cn(diagnosticValueVariants({ tone }))}>{value}</span></div>;
}

function QualityOverride({ session, diagnostics, qualitySupported, qualityPercent, qualityBusy, onSetQuality, t }: Omit<Props, "language"> & { diagnostics: ReturnType<typeof encoderDiagnosticsView>; t: InspectorStrings }) {
  return <div className="inspector-item quality-override-item">
    <div className="quality-override-heading">
      <span className="inspector-item-label">{t.manualQualityCeilingLabel}</span>
      <span className="inspector-item-value">{session.qualityOverride != null ? interpolate(t.fixedQualityPercent, { percent: qualityPercent }) : t.auto}{diagnostics.qualityBasis !== null ? ` · ${diagnostics.qualityBasis}` : null}</span>
    </div>
    <div className="quality-override-controls">
      <span className="quality-override-endpoint">{t.lowEndpoint}</span>
      <input key={`${session.session}-${session.qualityOverride ?? "auto"}-${Math.round((session.qualityHint ?? 0.5) * 100)}`} type="range" min="25" max="50" step="5" defaultValue={qualityPercent} disabled={!qualitySupported || session.state !== "running" || qualityBusy} aria-label={t.manualQualityCeilingLabel} onChange={(event) => void onSetQuality(session, Number(event.currentTarget.value) / 100)} />
      <span className="quality-override-endpoint">{t.defaultEndpoint}</span>
      <button className={cn("btn-ghost btn-sm quality-auto-button", inspectorButtonVariants())} disabled={!qualitySupported || session.qualityOverride == null || qualityBusy} onClick={() => void onSetQuality(session, null)}>{t.autoRevertButton}</button>
    </div>
    <span className="quality-override-help">{t.qualityOverrideHelp}</span>
  </div>;
}

export default function SessionEncoderDiagnostics({
  session,
  qualitySupported,
  qualityPercent,
  qualityBusy,
  onSetQuality,
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
      <QualityOverride {...{ session, diagnostics, qualitySupported, qualityPercent, qualityBusy, onSetQuality, t }} />
      <div className="inspector-item">
        <span className="inspector-item-label">{t.captureBackendLabel}</span>
        <span className={cn("inspector-item-value", "capitalize")}>
          {session.captureBackend || "ScreenCaptureKit"}
        </span>
      </div>
    </>
  );
}
