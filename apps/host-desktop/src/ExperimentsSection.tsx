import { useEffect, useState } from "react";
import { ChevronDown } from "lucide-react";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import { buttonVariants } from "./lib/variants";
import { useExperiments, type ViewerExperiments } from "./Privacy";

/**
 * 설정 모달의 실험 섹션. 페이싱 A/B 환경변수들을 앱 안에서 저장한다 — 값은
 * settings.json에 영속되고 호스트가 프로세스 환경변수로 주입하므로 다음
 * 스트림부터 효력을 가진다(앱 재시작 불필요). 데이터는 섹션이 스스로
 * 불러오고 저장한다(모달 prop 확장 없이).
 */
type NumericExperimentKey =
  | "maxEncodeInFlight"
  | "queueMaxAgeMs"
  | "sndbufBytes"
  | "drlWindowMs"
  | "pacingBudgetPct";

interface NumericRowSpec {
  key: NumericExperimentKey;
  label: string;
  desc: string;
  min: number;
  max: number;
  placeholder: string;
}

const inputStyle = {
  width: 96,
  padding: "4px 8px",
  fontSize: 12,
  textAlign: "right" as const,
  color: "var(--text-primary)",
  background: "var(--bg-surface-subtle)",
  border: "1px solid var(--border-subtle)",
  borderRadius: 7,
};

function ExperimentNumberRow({
  label,
  desc,
  min,
  max,
  placeholder,
  value,
  disabled,
  onCommit,
}: {
  label: string;
  desc: string;
  min: number;
  max: number;
  placeholder: string;
  value: number | null;
  disabled: boolean;
  onCommit: (next: number | null) => void;
}) {
  const [draft, setDraft] = useState(() => (value === null ? "" : String(value)));
  // 저장이 확정되어 호스트 값이 바뀌면(되돌림 포함) 입력을 원점에 맞춘다.
  useEffect(() => {
    setDraft(value === null ? "" : String(value));
  }, [value]);
  const commit = () => {
    const trimmed = draft.trim();
    if (trimmed === "") {
      if (value !== null) onCommit(null);
      return;
    }
    const parsed = Number(trimmed);
    if (!Number.isInteger(parsed) || parsed < min || parsed > max) {
      setDraft(value === null ? "" : String(value));
      return;
    }
    if (parsed !== value) onCommit(parsed);
  };
  return (
    <div className="settings-item-row">
      <div className="settings-item-info">
        <span className="settings-item-name">{label}</span>
        <p className="settings-item-desc">{desc}</p>
      </div>
      <input
        type="number"
        min={min}
        max={max}
        value={draft}
        placeholder={placeholder}
        disabled={disabled}
        aria-label={label}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
        }}
        style={inputStyle}
      />
    </div>
  );
}

export function ExperimentsSection({ t }: { t: TranslationSchema }) {
  const { experiments, error, saving, retry, save } = useExperiments();
  // 전송 계층 A/B 노브는 사용자 설정이 아니라 개발자 값이므로 기본은 접는다(DESIGN-REVIEW X-2).
  const [expanded, setExpanded] = useState(false);

  if (!experiments) {
    if (!error) return null;
    return (
      <div className="settings-section">
        <span className="settings-section-title">{t.host.experimentSection}</span>
        <div className="settings-item-error" role="alert">
          <span>{error}</span>
          <button
            type="button"
            className={buttonVariants({ variant: "ghost", size: "sm" })}
            onClick={retry}
          >
            {t.common.retry}
          </button>
        </div>
      </div>
    );
  }

  const numericRows: NumericRowSpec[] = [
    {
      key: "maxEncodeInFlight",
      label: t.host.experimentInFlightLabel,
      desc: t.host.experimentInFlightDesc,
      min: 1,
      max: 5,
      placeholder: "3",
    },
    {
      key: "queueMaxAgeMs",
      label: t.host.experimentQueueAgeLabel,
      desc: t.host.experimentQueueAgeDesc,
      min: 0,
      max: 120000,
      placeholder: "0",
    },
    {
      key: "sndbufBytes",
      label: t.host.experimentSndbufLabel,
      desc: t.host.experimentSndbufDesc,
      min: 65536,
      max: 2097152,
      placeholder: "524288",
    },
    {
      key: "drlWindowMs",
      label: t.host.experimentDrlLabel,
      desc: t.host.experimentDrlDesc,
      min: 50,
      max: 1000,
      placeholder: "1000",
    },
    {
      key: "pacingBudgetPct",
      label: t.host.experimentPacingLabel,
      desc: t.host.experimentPacingDesc,
      min: 30,
      max: 100,
      placeholder: "80",
    },
  ];

  const update = (patch: Partial<ViewerExperiments>) => {
    void save({ ...experiments, ...patch });
  };

  return (
    <div className="settings-section">
      <div className="settings-section-header">
        <span className="settings-section-title">{t.host.experimentSection}</span>
        <button
          type="button"
          className="settings-section-toggle"
          aria-expanded={expanded}
          onClick={() => setExpanded((prev) => !prev)}
        >
          {t.viewer.advancedSettingsToggle}
          <ChevronDown
            size={12}
            style={{ transform: expanded ? "rotate(180deg)" : "none", transition: "transform 0.2s" }}
          />
        </button>
      </div>
      {expanded && (
        <>
          <div className="settings-group-container">
            {numericRows.map((spec) => (
              <ExperimentNumberRow
                key={spec.key}
                label={spec.label}
                desc={spec.desc}
                min={spec.min}
                max={spec.max}
                placeholder={spec.placeholder}
                value={experiments[spec.key]}
                disabled={saving}
                onCommit={(next) => update({ [spec.key]: next })}
              />
            ))}
            <div className="settings-item-row">
              <div className="settings-item-info">
                <span className="settings-item-name">{t.host.experimentTraceLabel}</span>
                <p className="settings-item-desc">{t.host.experimentTraceDesc}</p>
              </div>
              <button
                type="button"
                className={`ui-switch ${experiments.frameTrace ? "switch-active" : ""}`}
                onClick={() => update({ frameTrace: !experiments.frameTrace })}
                aria-pressed={experiments.frameTrace}
                aria-label={t.host.experimentTraceLabel}
              >
                <span className="ui-switch-thumb" />
              </button>
            </div>
          </div>
          <div
            style={{
              color: "var(--text-muted)",
              fontSize: 12,
              lineHeight: "16px",
            }}
          >
            {t.host.experimentApplyHint}
          </div>
        </>
      )}
      {error && (
        <div className="settings-item-error" role="alert">
          <span>{error}</span>
          <button
            type="button"
            className={buttonVariants({ variant: "ghost", size: "sm" })}
            onClick={retry}
            disabled={saving}
          >
            {t.common.retry}
          </button>
        </div>
      )}
    </div>
  );
}
