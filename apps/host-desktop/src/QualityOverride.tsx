import {
  cn,
  getTranslation,
  interpolate,
  type SupportedLanguage,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import { encoderDiagnosticsView } from "./encoderDiagnostics";
import { inspectorButtonVariants } from "./lib/variants";
import type { SessionRow } from "./sessionTypes";

type InspectorStrings = TranslationSchema["host"]["inspector"];

function inspectorLanguage(saved: string | null): SupportedLanguage {
  return saved === "en" ? "en" : "ko";
}

interface Props {
  session: SessionRow;
  qualitySupported: boolean;
  qualityPercent: number;
  qualityBusy: boolean;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
}

/**
 * 세션 카드 본체의 수동 화질 상한 제어(DESIGN-REVIEW X-1). 개발자 인스펙터
 * 안에 갇혀 있던 사용자 제어를 카드 2차 행으로 승격했다. 슬라이더·자동 복귀의
 * 동작과 props 계약은 이동 전과 동일하다 — 렌더 위치만 바뀌었다.
 */
export default function QualityOverride({
  session,
  qualitySupported,
  qualityPercent,
  qualityBusy,
  onSetQuality,
}: Props) {
  const saved = typeof localStorage === "undefined" ? null : localStorage.getItem("leftcar_lang");
  const language = inspectorLanguage(saved);
  const t: InspectorStrings = getTranslation(language).host.inspector;
  const { qualityBasis } = encoderDiagnosticsView(session, language);

  return (
    <div className="quality-override-item">
      <div className="quality-override-heading">
        <span className="quality-override-label">{t.manualQualityCeilingLabel}</span>
        <span className="quality-override-value">
          {session.qualityOverride != null
            ? interpolate(t.fixedQualityPercent, { percent: qualityPercent })
            : t.auto}
          {qualityBasis !== null ? ` · ${qualityBasis}` : null}
        </span>
      </div>
      <div className="quality-override-controls">
        <span className="quality-override-endpoint">{t.lowEndpoint}</span>
        <input
          key={`${session.session}-${session.qualityOverride ?? "auto"}-${Math.round((session.qualityHint ?? 0.5) * 100)}`}
          type="range"
          min="25"
          max="50"
          step="5"
          defaultValue={qualityPercent}
          disabled={!qualitySupported || session.state !== "running" || qualityBusy}
          aria-label={t.manualQualityCeilingLabel}
          onChange={(event) => void onSetQuality(session, Number(event.currentTarget.value) / 100)}
        />
        <span className="quality-override-endpoint">{t.defaultEndpoint}</span>
        <button
          className={cn("btn-ghost btn-sm quality-auto-button", inspectorButtonVariants())}
          disabled={!qualitySupported || session.qualityOverride == null || qualityBusy}
          onClick={() => void onSetQuality(session, null)}
        >
          {t.autoRevertButton}
        </button>
      </div>
      <span className="quality-override-help">{t.qualityOverrideHelp}</span>
    </div>
  );
}
