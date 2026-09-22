import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import { buttonVariants } from "../lib/variants";

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
const modeLabel = (mode: Mode) => `${mode.width}×${mode.height} (${mode.width * mode.scale}×${mode.height * mode.scale})`;

function validDisplayMode(mode: { width: number; height: number } | undefined): boolean {
  return Boolean(mode && Number.isInteger(mode.width) && Number.isInteger(mode.height)
    && mode.width >= 640 && mode.width <= 4096 && mode.width % 2 === 0
    && mode.height >= 480 && mode.height <= 4096 && mode.height % 2 === 0);
}

function DisplayModeForm({ t, suggested, live, busy, onApply }: {
  t: TranslationSchema; suggested: Mode; live: VirtualDisplayStatus["live"];
  busy: boolean; onApply: (mode: Mode) => void;
}) {
  const [choice, setChoice] = useState(live ? "custom" : "auto");
  const [width, setWidth] = useState(String(live?.logicalWidth ?? suggested.width));
  const [height, setHeight] = useState(String(live?.logicalHeight ?? suggested.height));
  const [scale, setScale] = useState(live?.scale ?? suggested.scale);
  const mode = choice === "auto" ? suggested : choice === "custom"
    ? { width: Number(width), height: Number(height), scale }
    : PRESETS[Number(choice)];
  const valid = validDisplayMode(mode);
  return <div className="ext-display-controls">
    <select aria-label={t.host.extDisplayModeAria} className="ext-display-mode-select"
      value={choice} disabled={busy} onChange={event => setChoice(event.target.value)}>
      <option value="auto">{t.host.extDisplayAuto} · {modeLabel(suggested)}</option>
      {PRESETS.map((preset, index) => <option key={modeLabel(preset)} value={String(index)}>{modeLabel(preset)}</option>)}
      <option value="custom">{t.host.extDisplayCustom}</option>
    </select>
    {choice === "custom" && <div className="file-share-actions">
      <input type="number" aria-label={t.host.extDisplayWidth} min={640} max={4096} step={2}
        value={width} disabled={busy} onChange={event => setWidth(event.target.value)} />
      <span>×</span>
      <input type="number" aria-label={t.host.extDisplayHeight} min={480} max={4096} step={2}
        value={height} disabled={busy} onChange={event => setHeight(event.target.value)} />
      <select aria-label={t.host.extDisplayScale} value={scale} disabled={busy} onChange={event => setScale(Number(event.target.value))}>
        <option value={2}>Retina (2×)</option><option value={1}>1×</option>
      </select>
    </div>}
    <span className="file-share-hint">{t.host.extDisplaySizeHint}</span>
    <button className={buttonVariants({ variant: "ghost", size: "sm" })} disabled={busy || !valid}
      onClick={() => { if (valid && mode) onApply(mode); }}>
      {live ? t.host.extDisplayApply : t.host.extDisplayCreate}
    </button>
  </div>;
}

type RunDisplayAction = (command: string, args: Record<string, unknown>, failure: string) => Promise<void>;
function unavailableReason(reason: string | null | undefined, t: TranslationSchema): string {
  switch (reason) {
    case "unsupported-classes": return t.host.extDisplayReasonUnsupportedClasses;
    case "no-gui-session": return t.host.extDisplayReasonNoGuiSession;
    case "no-active-display": return t.host.extDisplayReasonNoActiveDisplay;
    default: return t.host.extDisplayUnavailable;
  }
}
function DisplayContents({ t, status, busy, run }: {
  t: TranslationSchema; status: VirtualDisplayStatus | null; busy: boolean; run: RunDisplayAction;
}) {
  if (!status) return <span role="status">{t.host.extDisplayLoading}</span>;
  if (!status.supported) return <span className="file-share-empty">{unavailableReason(status.reason, t)}</span>;
  if (status.removalPending) return <span role="status">{t.host.extDisplayRemoving}</span>;
  const live = status.live;
  const suggested = status.suggested ?? PRESETS[0]!;
  return <>
        {live && <>
          <span className="ext-display-status">{live.name} · {live.logicalWidth}×{live.logicalHeight} ({live.backingWidth}×{live.backingHeight}){live.modeVerified ? "" : ` · ${t.host.extDisplayModeUnverified}`}</span>
          <div className="file-share-actions" aria-label={t.host.extDisplayPosition}>
            {([["left", t.host.extDisplayLeft], ["right", t.host.extDisplayRight], ["above", t.host.extDisplayAbove], ["below", t.host.extDisplayBelow]] as const).map(([position, label]) => <button key={position}
              className={buttonVariants({ variant: "ghost", size: "sm" })} disabled={busy}
              onClick={() => void run("virtual_display_arrange", { position }, t.host.extDisplayArrangeError)}>
              {label}
            </button>)}
          </div>
          <span className="file-share-hint">{t.host.extDisplayResizeHint}</span>
        </>}
        <DisplayModeForm key={live ? `${live.sourceId}:${live.logicalWidth}:${live.logicalHeight}:${live.scale}` : "create"}
          t={t} suggested={suggested} live={live} busy={busy}
          onApply={({ width, height, scale }) => void run(live ? "virtual_display_resize" : "virtual_display_create", { width, height, scale }, live ? t.host.extDisplayResizeError : t.host.extDisplayCreateError)} />
        {live && <button className={buttonVariants({ variant: "ghost", size: "sm" })} disabled={busy}
          onClick={() => void run("virtual_display_remove", {}, t.host.extDisplayRemoveError)}>{t.host.extDisplayRemove}</button>}
  </>;
}

export function ExtendedDisplayCard({ t }: { t: TranslationSchema }) {
  const [status, setStatus] = useState<VirtualDisplayStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const operation = useRef(0);
  const running = useRef(false);
  const mounted = useRef(false);
  const refresh = useCallback(async () => {
    const revision = operation.current;
    try {
      const next = await invoke<VirtualDisplayStatus>("virtual_display_status");
      if (mounted.current && revision === operation.current) {
        setStatus(next);
        setActionError(previous => previous === "status" ? null : previous);
      }
    } catch {
      if (mounted.current && revision === operation.current) setActionError("status");
    }
  }, []);
  useEffect(() => {
    mounted.current = true;
    void refresh();
    const timer = setInterval(() => { if (!running.current) void refresh(); }, 2000);
    return () => { mounted.current = false; clearInterval(timer); operation.current += 1; };
  }, [refresh]);

  const run = async (command: string, args: Record<string, unknown>, failure: string) => {
    if (running.current) return;
    running.current = true;
    operation.current += 1;
    setBusy(true);
    setActionError(null);
    try { await invoke(command, args); }
    catch { if (mounted.current) setActionError(failure); }
    finally {
      await refresh();
      running.current = false;
      setBusy(false);
    }
  };

  return <section className="file-share-card" aria-label={t.host.extDisplaySection}>
    <div className="file-share-header"><div className="file-share-meta">
      <span className="file-share-title">{t.host.extDisplaySection}</span>
      <span className="file-share-hint">{t.host.extDisplayHint}</span>
    </div></div>
    {actionError && <span className="file-share-error" role="alert">{actionError === "status" ? t.host.extDisplayStatusError : actionError}</span>}
    <DisplayContents t={t} status={status} busy={busy} run={run} />
  </section>;
}
export default ExtendedDisplayCard;
