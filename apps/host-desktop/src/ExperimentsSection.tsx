import { useId, useState } from "react";
import { ChevronDown } from "lucide-react";
import { cn, interpolate, type TranslationSchema } from "@leftcar/ui-tokens";
import { Button, Field, Notice, Surface, Text, Toggle } from "./ui/primitives";
import { useExperiments, type ViewerExperiments } from "./Privacy";

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

function ExperimentNumberRow({
  label,
  desc,
  min,
  max,
  placeholder,
  value,
  disabled,
  validationMessage,
  onCommit,
}: {
  label: string;
  desc: string;
  min: number;
  max: number;
  placeholder: string;
  value: number | null;
  disabled: boolean;
  validationMessage: string;
  onCommit: (next: number | null) => void;
}) {
  const id = useId();
  const [draft, setDraft] = useState(() =>
    value === null ? "" : String(value),
  );
  const [invalid, setInvalid] = useState(false);
  const commit = () => {
    if (disabled) return;
    const trimmed = draft.trim();
    const parsed = trimmed === "" ? null : Number(trimmed);
    if (
      parsed !== null &&
      (!Number.isInteger(parsed) || parsed < min || parsed > max)
    ) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    if (parsed !== value) onCommit(parsed);
  };
  return (
    <div className="space-y-2 p-4">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <label htmlFor={id} className="text-body text-ink font-semibold">
            {label}
          </label>
          <p id={`${id}-help`} className="text-caption text-muted mt-1">
            {desc}
          </p>
        </div>
        <Field
          id={id}
          type="number"
          min={min}
          max={max}
          value={draft}
          placeholder={placeholder}
          disabled={disabled}
          invalid={invalid}
          aria-describedby={`${id}-help${invalid ? ` ${id}-error` : ""}`}
          onChange={(event) => {
            setDraft(event.target.value);
            setInvalid(false);
          }}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
          className="w-28 shrink-0 text-right tabular-nums"
        />
      </div>
      {invalid && (
        <Text id={`${id}-error`} variant="caption" role="alert">
          {validationMessage}
        </Text>
      )}
    </div>
  );
}

export function ExperimentsSection({ t }: { t: TranslationSchema }) {
  const { experiments, error, saving, retry, save } = useExperiments();
  const [expanded, setExpanded] = useState(false);
  const panelId = useId();
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
    if (experiments && !saving) void save({ ...experiments, ...patch });
  };
  return (
    <section className="space-y-2" aria-label={t.host.experimentSection}>
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-caption text-muted font-semibold">
          {t.host.experimentSection}
        </h3>
        <Button
          variant="ghost"
          size="compact"
          aria-expanded={expanded}
          aria-controls={panelId}
          onClick={() => setExpanded((previous) => !previous)}
        >
          {t.viewer.advancedSettingsToggle}
          <ChevronDown
            size={16}
            className={cn(
              "motion-reduce:transition-none",
              expanded && "rotate-180",
            )}
          />
        </Button>
      </div>
      {expanded && (
        <div id={panelId} className="space-y-2">
          {!experiments && !error && <Notice>{t.host.checkingSettings}</Notice>}
          {experiments && (
            <Surface variant="card" className="divide-y divide-outline p-0">
              {numericRows.map(({ key, ...spec }) => (
                <ExperimentNumberRow
                  key={`${key}:${experiments[key] ?? "default"}`}
                  {...spec}
                  value={experiments[key]}
                  disabled={saving}
                  validationMessage={interpolate(t.host.experimentValueError, {
                    min: spec.min,
                    max: spec.max,
                  })}
                  onCommit={(next) => update({ [key]: next })}
                />
              ))}
              <div className="flex items-start gap-3 p-4">
                <div className="min-w-0 flex-1">
                  <Text className="block font-semibold">
                    {t.host.experimentTraceLabel}
                  </Text>
                  <p className="text-caption text-muted mt-1">
                    {t.host.experimentTraceDesc}
                  </p>
                </div>
                <Toggle
                  checked={experiments.frameTrace}
                  busy={saving}
                  aria-label={t.host.experimentTraceLabel}
                  onClick={() =>
                    update({ frameTrace: !experiments.frameTrace })
                  }
                />
              </div>
            </Surface>
          )}
          <Text variant="caption" tone="muted">
            {t.host.experimentApplyHint}
          </Text>
          {saving && (
            <Text variant="caption" tone="muted" role="status">
              {t.host.savingSettings}
            </Text>
          )}
        </div>
      )}
      {error && (
        <Notice
          tone="error"
          className="flex flex-wrap items-center justify-between gap-2"
        >
          <Text variant="caption">
            {error.operation === "load"
              ? t.host.settingsLoadError
              : t.host.settingsSaveError}
          </Text>
          <Button
            variant="secondary"
            size="compact"
            busy={saving}
            onClick={retry}
          >
            {t.common.retry}
          </Button>
          <Text variant="caption" className="basis-full break-words">
            {error.detail}
          </Text>
        </Notice>
      )}
    </section>
  );
}
