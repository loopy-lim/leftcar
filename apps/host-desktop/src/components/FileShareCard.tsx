import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X } from "lucide-react";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import { Button, Notice, Surface, Text, Toggle } from "../ui/primitives";

interface ShareQueueEntryView {
  queueId: string;
  name: string;
  size: number;
}
interface ShareSelectionOutcome {
  entries: ShareQueueEntryView[];
  rejected: { name: string; reason: string }[];
}
type ShareAction =
  | { kind: "toggle"; enabled: boolean }
  | { kind: "add" }
  | { kind: "remove"; queueId: string };
type RejectedShareFile = ShareSelectionOutcome["rejected"][number] & {
  id: string;
};

async function executeShareAction(
  action: ShareAction,
): Promise<ShareSelectionOutcome | null> {
  if (action.kind === "add")
    return invoke<ShareSelectionOutcome>("add_share_files");
  if (action.kind === "toggle")
    await invoke("set_file_share", { enabled: action.enabled });
  else await invoke("remove_share_file", { queueId: action.queueId });
  return null;
}
function formatShareFileSize(size: number): string {
  if (size >= 1024 * 1024) return `${(size / (1024 * 1024)).toFixed(1)} MB`;
  if (size >= 1024) return `${Math.max(1, Math.round(size / 1024))} KB`;
  return `${size} B`;
}

export function FileShareCard({ t }: { t: TranslationSchema }) {
  const [enabled, setEnabled] = useState(false);
  const [entries, setEntries] = useState<ShareQueueEntryView[]>([]);
  const [ready, setReady] = useState(false);
  const [phase, setPhase] = useState<"checking" | "ready" | "saving">(
    "checking",
  );
  const loading = phase === "checking";
  const busy = phase === "saving";
  const [loadError, setLoadError] = useState<string | null>(null);
  const [rejectedFiles, setRejectedFiles] = useState<RejectedShareFile[]>([]);
  const [actionError, setActionError] = useState<string | null>(null);
  const mounted = useRef(false);
  const running = useRef(false);
  const retryAction = useRef<ShareAction | null>(null);

  const refresh = useCallback(async () => {
    setPhase("checking");
    try {
      const [shareEnabled, queue] = await Promise.all([
        invoke<boolean>("get_file_share"),
        invoke<ShareQueueEntryView[]>("list_share_queue"),
      ]);
      if (!mounted.current) return;
      setEnabled(shareEnabled);
      setEntries(queue);
      setReady(true);
      setLoadError(null);
    } catch (cause) {
      if (mounted.current) {
        setReady(false);
        setLoadError(
          `${t.host.settingsLoadError} ${String(cause instanceof Error ? cause.message : cause)}`,
        );
      }
    } finally {
      if (mounted.current) setPhase("ready");
    }
  }, [t]);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    return () => {
      mounted.current = false;
    };
  }, [refresh]);

  const runShareAction = async (action: ShareAction) => {
    if (running.current || !ready) return;
    running.current = true;
    retryAction.current = action;
    setPhase("saving");
    setActionError(null);
    try {
      const outcome = await executeShareAction(action);
      if (mounted.current && outcome)
        setRejectedFiles(
          outcome.rejected.map((file) => ({
            ...file,
            id: crypto.randomUUID(),
          })),
        );
      if (mounted.current) await refresh();
    } catch (cause) {
      if (mounted.current)
        setActionError(
          `${t.host.fileShareError} ${String(cause instanceof Error ? cause.message : cause)}`,
        );
    } finally {
      running.current = false;
      if (mounted.current) setPhase("ready");
    }
  };

  return (
    <Surface
      variant="card"
      role="region"
      aria-label={t.host.fileShareSection}
      className="space-y-3"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <Text className="block font-semibold">{t.host.fileShareSection}</Text>
          <Text variant="caption" tone="muted" className="block mt-1">
            {t.host.fileShareHint}
          </Text>
        </div>
        <div className="flex items-center gap-2">
          <Toggle
            checked={enabled}
            busy={busy || loading}
            disabled={!ready || !!loadError}
            aria-label={t.host.fileShareSection}
            onClick={() =>
              void runShareAction({ kind: "toggle", enabled: !enabled })
            }
          />
          {ready && enabled && (
            <Button
              variant="secondary"
              size="compact"
              busy={busy}
              onClick={() => void runShareAction({ kind: "add" })}
            >
              {t.host.fileShareAdd}
            </Button>
          )}
        </div>
      </div>
      {loading && (
        <Text variant="caption" tone="muted" role="status">
          {t.host.checkingSettings}
        </Text>
      )}
      {busy && (
        <Text variant="caption" tone="muted" role="status">
          {t.host.savingSettings}
        </Text>
      )}
      {loadError && (
        <Notice
          tone="error"
          className="flex flex-wrap items-center justify-between gap-2"
        >
          <Text variant="caption">{loadError}</Text>
          <Button
            variant="secondary"
            size="compact"
            busy={loading}
            onClick={() => void refresh()}
          >
            {t.common.retry}
          </Button>
        </Notice>
      )}
      {rejectedFiles.length > 0 && (
        <Notice tone="error" className="space-y-2">
          <Text variant="caption">{t.host.fileShareRejected}</Text>
          <ul className="space-y-1">
            {rejectedFiles.map((file) => (
              <li key={file.id} className="text-caption text-ink break-words">
                {file.name}: {file.reason}
              </li>
            ))}
          </ul>
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
            disabled={busy || !ready}
            onClick={() => {
              if (retryAction.current) void runShareAction(retryAction.current);
            }}
          >
            {t.common.retry}
          </Button>
        </Notice>
      )}
      {ready &&
        enabled &&
        (entries.length === 0 ? (
          <Text variant="caption" tone="muted">
            {t.host.fileShareEmpty}
          </Text>
        ) : (
          <ul className="space-y-2">
            {entries.map((entry) => (
              <li
                key={entry.queueId}
                className="flex min-w-0 items-center gap-2 border-b border-outline pb-2 last:border-0 last:pb-0"
              >
                <div className="min-w-0 flex-1">
                  <Text variant="caption" className="block break-all">
                    {entry.name}
                  </Text>
                  <Text variant="caption" tone="muted" className="tabular-nums">
                    {formatShareFileSize(entry.size)}
                  </Text>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  disabled={busy || loading}
                  onClick={() =>
                    void runShareAction({
                      kind: "remove",
                      queueId: entry.queueId,
                    })
                  }
                  aria-label={`${t.host.fileShareRemoveAria}: ${entry.name}`}
                >
                  <X size={16} />
                </Button>
              </li>
            ))}
          </ul>
        ))}
    </Surface>
  );
}
export default FileShareCard;
