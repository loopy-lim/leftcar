import { useCallback, useEffect, useId, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  cn,
  inputVariants,
  interpolate,
  type TranslationSchema,
} from "@leftcar/ui-tokens";
import { Button, Field, Notice, Surface, Text } from "../ui/primitives";

type Mode = { width: number; height: number; scale: number };
interface VirtualDisplayStatus {
  supported: boolean;
  reason?: string | null;
  removalPending: boolean;
  suggested?: (Mode & { source: string }) | null;
  live?: {
    sourceId?: string | null;
    name: string;
    logicalWidth: number;
    logicalHeight: number;
    backingWidth: number;
    backingHeight: number;
    scale: number;
    modeVerified: boolean;
  } | null;
}
const PRESETS: Mode[] = [
  { width: 1280, height: 800, scale: 2 },
  { width: 1440, height: 900, scale: 2 },
  { width: 1600, height: 1000, scale: 2 },
  { width: 2560, height: 1600, scale: 1 },
];
const modeLabel = (mode: Mode) =>
  `${mode.width}×${mode.height} (${mode.width * mode.scale}×${mode.height * mode.scale})`;
function validDisplayMode(mode: Mode | undefined): boolean {
  return Boolean(
    mode &&
      Number.isInteger(mode.width) &&
      Number.isInteger(mode.height) &&
      mode.width >= 640 &&
      mode.width <= 4096 &&
      mode.width % 2 === 0 &&
      mode.height >= 480 &&
      mode.height <= 4096 &&
      mode.height % 2 === 0,
  );
}
function DisplayModeForm({
  t,
  suggested,
  live,
  busy,
  onApply,
}: {
  t: TranslationSchema;
  suggested: Mode;
  live: VirtualDisplayStatus["live"];
  busy: boolean;
  onApply: (mode: Mode) => void;
}) {
  const [choice, setChoice] = useState(live ? "custom" : "auto");
  const [width, setWidth] = useState(
    String(live?.logicalWidth ?? suggested.width),
  );
  const [height, setHeight] = useState(
    String(live?.logicalHeight ?? suggested.height),
  );
  const [scale, setScale] = useState(live?.scale ?? suggested.scale);
  const helpId = useId();
  const mode =
    choice === "auto"
      ? suggested
      : choice === "custom"
        ? { width: Number(width), height: Number(height), scale }
        : PRESETS[Number(choice)];
  const valid = validDisplayMode(mode);
  return (
    <div className="space-y-3">
      <select
        aria-label={t.host.extDisplayModeAria}
        className={cn(inputVariants(), "w-full max-w-full")}
        value={choice}
        disabled={busy}
        onChange={(event) => setChoice(event.target.value)}
      >
        <option value="auto">
          {t.host.extDisplayAuto} · {modeLabel(suggested)}
        </option>
        {PRESETS.map((preset, index) => (
          <option key={modeLabel(preset)} value={String(index)}>
            {modeLabel(preset)}
          </option>
        ))}
        <option value="custom">{t.host.extDisplayCustom}</option>
      </select>
      {choice === "custom" && (
        <div className="flex flex-wrap items-end gap-2">
          <label
            htmlFor={`${helpId}-width`}
            className="flex flex-col gap-1 text-caption text-muted"
          >
            {t.host.extDisplayWidth}
            <Field
              id={`${helpId}-width`}
              type="number"
              aria-label={t.host.extDisplayWidth}
              min={640}
              max={4096}
              step={2}
              value={width}
              disabled={busy}
              invalid={!valid}
              aria-describedby={helpId}
              onChange={(event) => setWidth(event.target.value)}
              className="w-28 tabular-nums"
            />
          </label>
          <span className="py-3 text-body text-muted" aria-hidden="true">
            ×
          </span>
          <label
            htmlFor={`${helpId}-height`}
            className="flex flex-col gap-1 text-caption text-muted"
          >
            {t.host.extDisplayHeight}
            <Field
              id={`${helpId}-height`}
              type="number"
              aria-label={t.host.extDisplayHeight}
              min={480}
              max={4096}
              step={2}
              value={height}
              disabled={busy}
              invalid={!valid}
              aria-describedby={helpId}
              onChange={(event) => setHeight(event.target.value)}
              className="w-28 tabular-nums"
            />
          </label>
          <label
            htmlFor={`${helpId}-scale`}
            className="flex flex-col gap-1 text-caption text-muted"
          >
            {t.host.extDisplayScale}
            <select
              id={`${helpId}-scale`}
              aria-label={t.host.extDisplayScale}
              value={scale}
              disabled={busy}
              className={inputVariants()}
              onChange={(event) => setScale(Number(event.target.value))}
            >
              <option value={2}>Retina (2×)</option>
              <option value={1}>1×</option>
            </select>
          </label>
        </div>
      )}
      <Text id={helpId} variant="caption" tone="muted" className="block">
        {t.host.extDisplaySizeHint}
      </Text>
      {!valid && (
        <Notice tone="error">
          <Text variant="caption">
            {interpolate(t.host.extDisplayValidation, {
              minWidth: 640,
              minHeight: 480,
              max: 4096,
            })}
          </Text>
        </Notice>
      )}
      <Button
        variant="secondary"
        size="compact"
        disabled={busy || !valid}
        onClick={() => {
          if (valid && mode) onApply(mode);
        }}
      >
        {live ? t.host.extDisplayApply : t.host.extDisplayCreate}
      </Button>
    </div>
  );
}

type RunDisplayAction = (
  command: string,
  args: Record<string, unknown>,
  failure: string,
) => Promise<void>;
function unavailableReason(
  reason: string | null | undefined,
  t: TranslationSchema,
): string {
  switch (reason) {
    case "unsupported-classes":
      return t.host.extDisplayReasonUnsupportedClasses;
    case "no-gui-session":
      return t.host.extDisplayReasonNoGuiSession;
    case "no-active-display":
      return t.host.extDisplayReasonNoActiveDisplay;
    default:
      return t.host.extDisplayUnavailable;
  }
}
function DisplayContents({
  t,
  status,
  busy,
  run,
}: {
  t: TranslationSchema;
  status: VirtualDisplayStatus;
  busy: boolean;
  run: RunDisplayAction;
}) {
  if (!status.supported)
    return (
      <Text variant="caption" tone="muted">
        {unavailableReason(status.reason, t)}
      </Text>
    );
  if (status.removalPending)
    return <Notice>{t.host.extDisplayRemoving}</Notice>;
  const live = status.live;
  const suggested = status.suggested ?? PRESETS[0]!;
  return (
    <>
      {live && (
        <>
          <Text variant="code" tone="muted" className="block break-words">
            {live.name} · {live.logicalWidth}×{live.logicalHeight} (
            {live.backingWidth}×{live.backingHeight})
            {live.modeVerified ? "" : ` · ${t.host.extDisplayModeUnverified}`}
          </Text>
          <div
            role="group"
            className="flex flex-wrap gap-2"
            aria-label={t.host.extDisplayPosition}
          >
            {(
              [
                ["left", t.host.extDisplayLeft],
                ["right", t.host.extDisplayRight],
                ["above", t.host.extDisplayAbove],
                ["below", t.host.extDisplayBelow],
              ] as const
            ).map(([position, label]) => (
              <Button
                key={position}
                variant="secondary"
                size="compact"
                disabled={busy}
                onClick={() =>
                  void run(
                    "virtual_display_arrange",
                    { position },
                    t.host.extDisplayArrangeError,
                  )
                }
              >
                {label}
              </Button>
            ))}
          </div>
          <Text variant="caption" tone="muted" className="block">
            {t.host.extDisplayResizeHint}
          </Text>
        </>
      )}
      <DisplayModeForm
        key={
          live
            ? `${live.sourceId}:${live.logicalWidth}:${live.logicalHeight}:${live.scale}`
            : "create"
        }
        t={t}
        suggested={suggested}
        live={live}
        busy={busy}
        onApply={({ width, height, scale }) =>
          void run(
            live ? "virtual_display_resize" : "virtual_display_create",
            { width, height, scale },
            live ? t.host.extDisplayResizeError : t.host.extDisplayCreateError,
          )
        }
      />
      {live && (
        <Button
          variant="ghost"
          size="compact"
          disabled={busy}
          onClick={() =>
            void run("virtual_display_remove", {}, t.host.extDisplayRemoveError)
          }
        >
          {t.host.extDisplayRemove}
        </Button>
      )}
    </>
  );
}
export function ExtendedDisplayCard({ t }: { t: TranslationSchema }) {
  const [status, setStatus] = useState<VirtualDisplayStatus | null>(null);
  const [phase, setPhase] = useState<"idle" | "updating">("idle");
  const busy = phase === "updating";
  const [actionError, setActionError] = useState<string | null>(null);
  const [statusError, setStatusError] = useState(false);
  const operation = useRef(0);
  const running = useRef(false);
  const mounted = useRef(false);
  const lastAction = useRef<{
    command: string;
    args: Record<string, unknown>;
    failure: string;
  } | null>(null);
  const refresh = useCallback(async () => {
    const revision = operation.current;
    try {
      const next = await invoke<VirtualDisplayStatus>("virtual_display_status");
      if (mounted.current && revision === operation.current) {
        setStatus(next);
        setStatusError(false);
      }
    } catch {
      if (mounted.current && revision === operation.current)
        setStatusError(true);
    }
  }, []);
  useEffect(() => {
    mounted.current = true;
    void refresh();
    const timer = setInterval(() => {
      if (!running.current) void refresh();
    }, 2000);
    return () => {
      mounted.current = false;
      clearInterval(timer);
      operation.current += 1;
    };
  }, [refresh]);
  const run: RunDisplayAction = async (command, args, failure) => {
    if (running.current || statusError) return;
    running.current = true;
    operation.current += 1;
    lastAction.current = { command, args, failure };
    setPhase("updating");
    setActionError(null);
    try {
      await invoke(command, args);
    } catch {
      if (mounted.current) setActionError(failure);
    } finally {
      await refresh();
      running.current = false;
      if (mounted.current) setPhase("idle");
    }
  };
  return (
    <Surface
      variant="card"
      role="region"
      aria-label={t.host.extDisplaySection}
      className="space-y-3"
    >
      <div>
        <Text className="block font-semibold">{t.host.extDisplaySection}</Text>
        <Text variant="caption" tone="muted" className="block mt-1">
          {t.host.extDisplayHint}
        </Text>
      </div>
      {statusError && (
        <Notice
          tone="error"
          className="flex flex-wrap items-center justify-between gap-2"
        >
          <Text variant="caption">{t.host.extDisplayStatusError}</Text>
          <Button
            variant="secondary"
            size="compact"
            onClick={() => void refresh()}
          >
            {t.common.retry}
          </Button>
        </Notice>
      )}
      {actionError && (
        <Notice
          tone="error"
          className="flex flex-wrap items-center justify-between gap-2"
        >
          <Text variant="caption">{actionError}</Text>
          <Button
            variant="secondary"
            size="compact"
            disabled={busy || statusError}
            onClick={() => {
              if (lastAction.current) {
                const { command, args, failure } = lastAction.current;
                void run(command, args, failure);
              }
            }}
          >
            {t.common.retry}
          </Button>
        </Notice>
      )}
      {busy && <Notice>{t.host.extDisplayWorking}</Notice>}
      {!status && !statusError && <Notice>{t.host.extDisplayLoading}</Notice>}
      {status && (
        <DisplayContents
          t={t}
          status={status}
          busy={busy || statusError}
          run={run}
        />
      )}
    </Surface>
  );
}
export default ExtendedDisplayCard;
