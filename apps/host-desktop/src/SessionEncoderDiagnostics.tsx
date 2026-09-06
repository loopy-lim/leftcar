import type { ReactNode } from "react";
import { diagnosticValueVariants } from "./diagnosticStyles";
import { encoderDiagnosticsView } from "./encoderDiagnostics";
import { cn } from "./lib/cn";
import { inspectorButtonVariants } from "./lib/variants";
import type { SessionRow } from "./sessionTypes";

interface Props {
  session: SessionRow;
  qualitySupported: boolean;
  qualityPercent: number;
  qualityBusy: boolean;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
}

function DiagnosticRow({ label, value, tone = "default" }: { label: string; value: ReactNode; tone?: "default" | "warning" | "active" }) {
  return <div className="inspector-item"><span className="inspector-item-label">{label}</span><span className={cn(diagnosticValueVariants({ tone }))}>{value}</span></div>;
}

function QualityOverride({ session, diagnostics, qualitySupported, qualityPercent, qualityBusy, onSetQuality }: Props & { diagnostics: ReturnType<typeof encoderDiagnosticsView> }) {
  return <div className="inspector-item quality-override-item">
    <div className="quality-override-heading">
      <span className="inspector-item-label">수동 화질 상한</span>
      <span className="inspector-item-value">{session.qualityOverride != null ? `${qualityPercent}% 고정` : "자동"}{diagnostics.qualityBasis !== null ? ` · ${diagnostics.qualityBasis}` : null}</span>
    </div>
    <div className="quality-override-controls">
      <span className="quality-override-endpoint">낮음</span>
      <input key={`${session.session}-${session.qualityOverride ?? "auto"}-${Math.round((session.qualityHint ?? 0.5) * 100)}`} type="range" min="25" max="50" step="5" defaultValue={qualityPercent} disabled={!qualitySupported || session.state !== "running" || qualityBusy} aria-label="수동 화질 상한" onChange={(event) => void onSetQuality(session, Number(event.currentTarget.value) / 100)} />
      <span className="quality-override-endpoint">기본</span>
      <button className={cn("btn-ghost btn-sm quality-auto-button", inspectorButtonVariants())} disabled={!qualitySupported || session.qualityOverride == null || qualityBusy} onClick={() => void onSetQuality(session, null)}>자동 복귀</button>
    </div>
    <span className="quality-override-help">고변화 장면에서 프레임을 지키려면 낮추고, 여유가 생기면 자동 복귀하세요.</span>
  </div>;
}

export default function SessionEncoderDiagnostics({
  session,
  qualitySupported,
  qualityPercent,
  qualityBusy,
  onSetQuality,
}: Props) {
  const diagnostics = encoderDiagnosticsView(session);
  const hasInFlightWork = [session.encodeInFlight, session.packetizationInFlight]
    .some((value) => typeof value === "number" && Number.isFinite(value) && value > 0);
  return (
    <>
      <div className="inspector-item">
        <span className="inspector-item-label">인코더 경로</span>
        <span className="inspector-item-value">{diagnostics.path} · {diagnostics.identity}</span>
      </div>
      <div className="inspector-item">
        <span className="inspector-item-label">인코더 설정</span>
        <span className="inspector-item-value">{diagnostics.configuration} · 적용 {diagnostics.applied}</span>
      </div>
      <div className="inspector-item">
        <span className="inspector-item-label">실험 프로필</span>
        <span className={cn(diagnosticValueVariants({
          tone: diagnostics.hasExperimentFallback ? "warning" : "default",
        }))}>
          {diagnostics.experiment} · {diagnostics.experimentDetail} · 실험 fallback {diagnostics.experimentFallback}
        </span>
      </div>
      <DiagnosticRow label="인코더 압력" value={diagnostics.pressure} tone={diagnostics.hasEncoderPressure ? "warning" : "default"} />
      <DiagnosticRow label="인코더/패킷화 in-flight" value={diagnostics.inFlight} tone={hasInFlightWork ? "active" : "default"} />
      <DiagnosticRow label="유효 출력 FPS" value={diagnostics.validOutputFps} />
      <div className="inspector-item">
        <span className="inspector-item-label">미지원/거부 속성</span>
        <span className="inspector-item-value">{diagnostics.unavailable}</span>
      </div>
      {diagnostics.fallback !== null ? (
        <div className="inspector-item">
          <span className="inspector-item-label">인코더 fallback</span>
          <span className="inspector-item-value">{diagnostics.fallback}</span>
        </div>
      ) : null}
      <div className="inspector-item">
        <span className="inspector-item-label">동적 화질 힌트</span>
        <span className="inspector-item-value">
          {session.qualityHint != null
            ? `${session.qualityHint.toFixed(2)} · ${session.qualityAdaptationChanges ?? 0}회 변경 / ${session.qualityAdaptationChecks ?? 0}회 확인 · ${session.qualityAdaptationLastStatus ?? "대기"}`
            : "적용 안 됨"}
        </span>
      </div>
      <QualityOverride {...{ session, diagnostics, qualitySupported, qualityPercent, qualityBusy, onSetQuality }} />
      <div className="inspector-item">
        <span className="inspector-item-label">화면 처리 방식</span>
        <span className={cn("inspector-item-value", "capitalize")}>
          {session.captureBackend || "ScreenCaptureKit"}
        </span>
      </div>
    </>
  );
}
