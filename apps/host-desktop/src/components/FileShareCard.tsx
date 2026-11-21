import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X } from "lucide-react";
import { cn, type TranslationSchema } from "@leftcar/ui-tokens";
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

function useFileShareState(t: TranslationSchema) {
  const [enabled, setEnabled] = useState(false);
  const [entries, setEntries] = useState<ShareQueueEntryView[]>([]);
  const [ready, setReady] = useState(false);
  const readyRef = useRef(false);
  const [phase, setPhase] = useState<"checking" | "ready" | "saving">(
    "checking",
  );
  const [loadError, setLoadError] = useState<string | null>(null);
  const [rejectedFiles, setRejectedFiles] = useState<RejectedShareFile[]>([]);
  const [actionError, setActionError] = useState<string | null>(null);
  const mounted = useRef(false);
  const running = useRef(false);
  const retryAction = useRef<ShareAction | null>(null);

  const refresh = useCallback(async () => {
    if (!readyRef.current) setPhase("checking");
    try {
      const [shareEnabled, queue] = await Promise.all([
        invoke<boolean>("get_file_share"),
        invoke<ShareQueueEntryView[]>("list_share_queue"),
      ]);
      if (!mounted.current) return;
      setEnabled(shareEnabled);
      setEntries(queue);
      readyRef.current = true;
      setReady(true);
      setLoadError(null);
    } catch (cause) {
      if (mounted.current) {
        readyRef.current = false;
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
    if (action.kind === "toggle" && !action.enabled) {
      setRejectedFiles([]);
    }
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

  return {
    enabled,
    entries,
    ready,
    loading: phase === "checking",
    busy: phase === "saving",
    loadError,
    rejectedFiles,
    actionError,
    clearRejected: () => setRejectedFiles([]),
    refresh,
    runShareAction,
    retryLastAction: () => {
      if (retryAction.current) void runShareAction(retryAction.current);
    },
  };
}

function RejectedShareFilesNotice({
  rejectedFiles,
  t,
  onDismiss,
}: {
  rejectedFiles: RejectedShareFile[];
  t: TranslationSchema;
  onDismiss: () => void;
}) {
  if (rejectedFiles.length === 0) return null;
  return (
    <Notice tone="error" className="space-y-2">
      <div className="flex items-start justify-between gap-2">
        <Text variant="caption" className="font-semibold">
          {t.host.fileShareRejected}
        </Text>
        <Button
          variant="ghost"
          size="icon"
          onClick={onDismiss}
          aria-label={t.common.close}
        >
          <X size={16} />
        </Button>
      </div>
      <ul className="space-y-1">
        {rejectedFiles.map((file) => (
          <li key={file.id} className="text-caption text-ink break-words">
            {file.name}: {file.reason}
          </li>
        ))}
      </ul>
    </Notice>
  );
}

function ShareQueueList({
  entries,
  busy,
  loading,
  t,
  onRemove,
}: {
  entries: ShareQueueEntryView[];
  busy: boolean;
  loading: boolean;
  t: TranslationSchema;
  onRemove: (queueId: string) => void;
}) {
  if (entries.length === 0) {
    return (
      <Text variant="caption" tone="muted">
        {t.host.fileShareEmpty}
      </Text>
    );
  }
  return (
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
            onClick={() => onRemove(entry.queueId)}
            aria-label={`${t.host.fileShareRemoveAria}: ${entry.name}`}
          >
            <X size={16} />
          </Button>
        </li>
      ))}
    </ul>
  );
}

export function FileShareCard({
  t,
  hideHeading = false,
}: {
  t: TranslationSchema;
  hideHeading?: boolean;
}) {
  const {
    enabled,
    entries,
    ready,
    loading,
    busy,
    loadError,
    rejectedFiles,
    actionError,
    clearRejected,
    refresh,
    runShareAction,
    retryLastAction,
  } = useFileShareState(t);

  return (
    <Surface
      variant="card"
      role="region"
      aria-label={t.host.fileShareSection}
      className="space-y-3"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          {!hideHeading && (
            <Text className="block font-semibold">{t.host.fileShareSection}</Text>
          )}
          <Text
            variant="caption"
            tone="muted"
            className={cn("block", !hideHeading && "mt-1")}
          >
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
      <RejectedShareFilesNotice
        rejectedFiles={rejectedFiles}
        t={t}
        onDismiss={clearRejected}
      />
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
            onClick={retryLastAction}
          >
            {t.common.retry}
          </Button>
        </Notice>
      )}
      {ready && enabled && (
        <ShareQueueList
          entries={entries}
          busy={busy}
          loading={loading}
          t={t}
          onRemove={(queueId) => void runShareAction({ kind: "remove", queueId })}
        />
      )}
    </Surface>
  );
}
export default FileShareCard;
