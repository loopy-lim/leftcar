import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X } from "lucide-react";
import type { TranslationSchema } from "@leftcar/ui-tokens";
import { buttonVariants } from "../lib/variants";

interface ShareQueueEntryView {
  queueId: string;
  name: string;
  size: number;
}

function formatShareFileSize(size: number): string {
  if (size >= 1024 * 1024) return `${(size / (1024 * 1024)).toFixed(1)} MB`;
  if (size >= 1024) return `${Math.max(1, Math.round(size / 1024))} KB`;
  return `${size} B`;
}

export function FileShareCard({ t }: { t: TranslationSchema }) {
  const [enabled, setEnabled] = useState(false);
  const [entries, setEntries] = useState<ShareQueueEntryView[]>([]);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [shareEnabled, queue] = await Promise.all([
        invoke<boolean>("get_file_share"),
        invoke<ShareQueueEntryView[]>("list_share_queue"),
      ]);
      setEnabled(shareEnabled);
      setEntries(queue);
    } catch {
      // Keep quiet if service error
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const runShareAction = async (action: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await action();
      setActionError(null);
      await refresh();
    } catch {
      setActionError(t.host.fileShareError);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="file-share-card" aria-label={t.host.fileShareSection}>
      <div className="file-share-header">
        <div className="file-share-meta">
          <span className="file-share-title">{t.host.fileShareSection}</span>
          <span className="file-share-hint">{t.host.fileShareHint}</span>
        </div>
        <div className="file-share-actions" style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <button
            type="button"
            role="switch"
            className={`ui-switch ${enabled ? "switch-active" : ""}`}
            disabled={busy}
            aria-label={`${t.host.fileShareSection} ${enabled ? t.host.fileShareToggleOn : t.host.fileShareToggleOff}`}
            onClick={() => void runShareAction(() => invoke("set_file_share", { enabled: !enabled }))}
            aria-checked={enabled}
          >
            <span className="ui-switch-thumb" />
          </button>
          {enabled && (
            <button
              className={buttonVariants({ variant: "ghost", size: "sm" })}
              disabled={busy}
              onClick={() => void runShareAction(() => invoke("add_share_files"))}
            >
              {t.host.fileShareAdd}
            </button>
          )}
        </div>
      </div>
      {actionError && (
        <span className="file-share-error" role="alert">{actionError}</span>
      )}
      {enabled && (
        entries.length === 0 ? (
          <span className="file-share-empty">{t.host.fileShareEmpty}</span>
        ) : (
          <ul className="file-share-list">
            {entries.map((entry) => (
              <li key={entry.queueId} className="file-share-list-item">
                <span className="file-share-item-name">
                  {entry.name} <span className="file-share-item-size">({formatShareFileSize(entry.size)})</span>
                </span>
                <button
                  className={buttonVariants({ variant: "close" })}
                  onClick={() => void runShareAction(() => invoke("remove_share_file", { queueId: entry.queueId }))}
                  aria-label={`${t.host.fileShareRemoveAria}: ${entry.name}`}
                >
                  <X size={13} />
                </button>
              </li>
            ))}
          </ul>
        )
      )}
    </section>
  );
}

export default FileShareCard;
