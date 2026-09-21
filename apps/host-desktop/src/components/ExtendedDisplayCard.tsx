import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import { buttonVariants } from "../lib/variants";

interface VirtualDisplaySuggested {
  width: number;
  height: number;
  scale: number;
  source: "viewerMetrics" | "fallback";
}

interface VirtualDisplayLive {
  displayId: number;
  sourceId?: string | null;
  name: string;
  logicalWidth: number;
  logicalHeight: number;
  scale: number;
  backingWidth: number;
  backingHeight: number;
  modeVerified: boolean;
}

interface VirtualDisplayStatus {
  supported: boolean;
  reason?: string | null;
  live?: VirtualDisplayLive | null;
  suggested?: VirtualDisplaySuggested | null;
  removalPending: boolean;
}

const EXT_DISPLAY_PRESETS: ReadonlyArray<{ width: number; height: number; scale: number }> = [
  { width: 1280, height: 800, scale: 2 },
  { width: 1440, height: 900, scale: 2 },
  { width: 1600, height: 1000, scale: 2 },
  { width: 2560, height: 1600, scale: 1 },
];

function extDisplayPresetLabel(preset: { width: number; height: number; scale: number }): string {
  return `${preset.width}×${preset.height} (${preset.width * preset.scale}×${preset.height * preset.scale})`;
}

function extDisplayUnavailableText(
  status: VirtualDisplayStatus | null,
  t: TranslationSchema,
): string | null {
  if (!status || status.supported) return null;
  switch (status.reason) {
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

function ExtendedDisplayCreateControls({
  t,
  suggested,
  busy,
  onCreate,
}: {
  t: TranslationSchema;
  suggested: VirtualDisplaySuggested | null;
  busy: boolean;
  onCreate: (choice: string) => void;
}) {
  const [modeChoice, setModeChoice] = useState<string>("");
  const suggestedLabel =
    suggested &&
    `${suggested.width}×${suggested.height} (${suggested.width * suggested.scale}×${suggested.height * suggested.scale})`;
  return (
    <>
      <select
        className="ext-display-mode-select"
        aria-label={t.host.extDisplayModeAria}
        value={modeChoice || suggestedLabel || ""}
        disabled={busy}
        onChange={(event) => {
          setModeChoice(event.target.value);
          onCreate(event.target.value);
        }}
      >
        {suggestedLabel && (
          <option value={suggestedLabel}>
            {suggestedLabel} ·{" "}
            {suggested?.source === "viewerMetrics"
              ? t.host.extDisplaySuggestedViewer
              : t.host.extDisplaySuggestedFallback}
          </option>
        )}
        {EXT_DISPLAY_PRESETS.map((preset) => {
          const label = extDisplayPresetLabel(preset);
          if (label === suggestedLabel) return null;
          return <option key={label} value={label}>{label}</option>;
        })}
      </select>
      <button
        className={buttonVariants({ variant: "ghost", size: "sm" })}
        disabled={busy}
        onClick={() => onCreate(modeChoice || suggestedLabel || "")}
        aria-label={t.host.extDisplayCreate}
      >
        {t.host.extDisplayCreate}
      </button>
    </>
  );
}

export function ExtendedDisplayCard({ t }: { t: TranslationSchema }) {
  const [status, setStatus] = useState<VirtualDisplayStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<VirtualDisplayStatus>("virtual_display_status"));
    } catch {
      // Keep quiet if service error
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const runAction = async (action: () => Promise<unknown>, errorText: string) => {
    setBusy(true);
    try {
      await action();
      setActionError(null);
      await refresh();
    } catch {
      setActionError(errorText);
    } finally {
      setBusy(false);
    }
  };

  const live = status?.live ?? null;
  const unavailableReason = extDisplayUnavailableText(status, t);

  const createWithChoice = (choice: string) => {
    const preset = EXT_DISPLAY_PRESETS.find(
      (candidate) => extDisplayPresetLabel(candidate) === choice,
    );
    if (!preset) return;
    void runAction(
      () =>
        invoke("virtual_display_create", {
          width: preset.width,
          height: preset.height,
          scale: preset.scale,
        }),
      t.host.extDisplayCreateError,
    );
  };

  return (
    <section className="file-share-card" aria-label={t.host.extDisplaySection}>
      <div className="file-share-header">
        <div className="file-share-meta">
          <span className="file-share-title">{t.host.extDisplaySection}</span>
          <span className="file-share-hint">{t.host.extDisplayHint}</span>
        </div>
        {!unavailableReason && (
          <div className="file-share-actions" style={{ display: "flex", alignItems: "center", gap: 10 }}>
            {live ? (
              <button
                className={buttonVariants({ variant: "ghost", size: "sm" })}
                disabled={busy || status?.removalPending}
                onClick={() => void runAction(() => invoke("virtual_display_remove"), t.host.extDisplayRemoveError)}
                aria-label={t.host.extDisplayRemove}
              >
                {status?.removalPending ? t.host.extDisplayRemoving : t.host.extDisplayRemove}
              </button>
            ) : (
              <ExtendedDisplayCreateControls
                t={t}
                suggested={status?.suggested ?? null}
                busy={busy}
                onCreate={createWithChoice}
              />
            )}
          </div>
        )}
      </div>
      {actionError && <span className="file-share-error" role="alert">{actionError}</span>}
      {unavailableReason ? (
        <span className="file-share-empty">{unavailableReason}</span>
      ) : live ? (
        <span className="ext-display-status">
          {live.name} · {live.logicalWidth}×{live.logicalHeight} ({live.backingWidth}×
          {live.backingHeight}){live.modeVerified ? "" : " · HiDPI 미확인"}
        </span>
      ) : null}
    </section>
  );
}

export default ExtendedDisplayCard;
