import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getTranslation, type SupportedLanguage } from "@leftcar/ui-tokens";

export interface SourceGrantView {
  credentialId: string;
  stateRevision: number;
  sourceIds: string[];
  revision: number;
  reviewRequired: boolean;
  persistenceError?: string | null;
}
interface Source { sourceId?: string; index: number; name: string; width: number; height: number }

/** The only writer is a Host-local Tauri command. Viewer cannot approve itself. */
export default function SourceGrantEditor({ deviceId, grants, language, onSaved, onFailure, onSaving }: {
  deviceId: string; grants?: SourceGrantView; language: SupportedLanguage; onSaved: (deviceId: string, grants: SourceGrantView, startedAtEpoch?: number) => Promise<void>;
  onSaving?: (credentialId: string) => number;
  onFailure?: (deviceId: string, credentialId: string, error: string) => void;
}) {
  const t = getTranslation(language).host;
  const [editor, setEditor] = useState<{ sources: Source[]; selected: string[] } | null>(null);
  const [busy, setBusy] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const open = async () => {
    setBusy(true); setError(null);
    try {
      const sources = await invoke<Source[]>("list_host_sources");
      setEditor({ sources, selected: grants?.sourceIds ?? [] });
    } catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  };
  const save = async (sourceIds: string[]) => {
    setBusy(true); setError(null);
    try {
      if (!grants?.credentialId) throw new Error("Review the current paired device before saving");
      const startedAtEpoch = onSaving?.(grants.credentialId);
      const confirmed = await invoke<SourceGrantView>("set_source_grants", { deviceId, sourceIds, credentialId: grants.credentialId });
      await onSaved(deviceId, confirmed, startedAtEpoch);
      setEditor(null);
      setUncertain(false);
    } catch (cause) { setUncertain(true); setError(String(cause)); if (grants) onFailure?.(deviceId, grants.credentialId, String(cause)); }
    finally { setBusy(false); }
  };
  const selectedIds = new Set(editor?.selected ?? []);
  const availableIds = new Set(editor?.sources.map(source => source.sourceId) ?? []);
  const toggle = (id: string) => setEditor(current => {
    if (!current) return current;
    const selected = new Set(current.selected);
    if (selected.has(id)) selected.delete(id); else selected.add(id);
    return { ...current, selected: [...selected] };
  });
  return <div>
    <p>{uncertain || grants?.reviewRequired !== false ? t.sourceReviewRequired : `${t.sourceAccess}: ${grants.sourceIds.length}`}</p>
    <p>{t.sourceApprovalPolicy}</p>
    {(error || grants?.persistenceError) && <p role="alert">{error || grants?.persistenceError}</p>}
    <button type="button" disabled={busy} onClick={open}>{t.sourceManage}</button>
    <button type="button" disabled={busy} onClick={() => save([])}>{t.sourceRemoveAll}</button>
    {editor && <fieldset disabled={busy}>
      <legend>{t.sourceAccess}</legend>
      {editor.sources.map(source => <label key={source.sourceId ?? `unavailable-${source.index}`} style={{ display: "block" }}>
        <input type="checkbox" disabled={!source.sourceId} checked={Boolean(source.sourceId && selectedIds.has(source.sourceId))}
          onChange={() => { const id = source.sourceId; if (!id) return;
            toggle(id);
          }} /> {source.name} ({source.width} × {source.height})
      </label>)}
      {editor.selected.filter(id => !availableIds.has(id)).map(id => <label key={id} style={{ display: "block" }}>
        <input type="checkbox" checked onChange={() => setEditor(current => current && ({ ...current, selected: current.selected.filter(selected => selected !== id) }))} /> {t.sourceUnavailable}: {id}
      </label>)}
      <button type="button" onClick={() => save(editor.selected)}>{t.sourceApprove}</button>
      <button type="button" onClick={() => setEditor(null)}>{getTranslation(language).common.cancel}</button>
    </fieldset>}
  </div>;
}
