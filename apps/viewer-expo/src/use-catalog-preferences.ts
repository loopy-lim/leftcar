import { useCallback, useEffect, useRef } from "react";
import * as SecureStore from "expo-secure-store";
import {
  deviceClipboardIo,
  loadClipboardShare,
  saveClipboardShare,
  startClipboardSync,
  CLIPBOARD_SHARE_KEY,
  type ClipboardSyncLoop,
} from "./clipboard-sync";
import { PreferencePersistenceController, type PreferencePersistenceStatus } from "./preference-persistence";
import { controlClient } from "./session";
import { usePreferencePersistence } from "./use-preference-persistence";
import {
  DEFAULT_VIEWER_PREFERENCES,
  readViewerPreferences,
  VIEWER_PREFERENCES_KEY,
  writeViewerPreferences,
  type ViewerPreferences,
} from "./viewer-preferences";

export type PreferencePersistenceIssue =
  | "viewer-load"
  | "viewer-save"
  | "clipboard-load"
  | "clipboard-save";

const VIEWER_ISSUES: Record<PreferencePersistenceStatus, PreferencePersistenceIssue | null> = {
  loading: null,
  ready: null,
  saving: null,
  "load-error": "viewer-load",
  "save-error": "viewer-save",
};

const CLIPBOARD_ISSUES: Record<PreferencePersistenceStatus, PreferencePersistenceIssue | null> = {
  loading: null,
  ready: null,
  saving: null,
  "load-error": "clipboard-load",
  "save-error": "clipboard-save",
};

function createViewerPreferencesController() {
  return new PreferencePersistenceController({
    initialValue: DEFAULT_VIEWER_PREFERENCES,
    load: () => readViewerPreferences(SecureStore),
    save: (value: ViewerPreferences) => writeViewerPreferences(SecureStore, value),
    storageKey: VIEWER_PREFERENCES_KEY,
    storageOwner: SecureStore,
  });
}

function createClipboardPreferenceController() {
  return new PreferencePersistenceController({
    initialValue: false,
    load: () => loadClipboardShare(SecureStore),
    save: (value: boolean) => saveClipboardShare(SecureStore, value),
    storageKey: CLIPBOARD_SHARE_KEY,
    storageOwner: SecureStore,
  });
}

function preferenceLocked(status: PreferencePersistenceStatus): boolean {
  return status === "loading" || status === "load-error";
}

function clipboardSyncEnabled(status: PreferencePersistenceStatus, value: boolean): boolean {
  return !preferenceLocked(status) && value;
}

export function useCatalogPreferences() {
  const viewer = usePreferencePersistence(
    createViewerPreferencesController,
    DEFAULT_VIEWER_PREFERENCES,
  );
  const clipboard = usePreferencePersistence(createClipboardPreferenceController, false);
  const clipboardSyncRef = useRef<ClipboardSyncLoop | null>(null);

  useEffect(() => {
    const sync = startClipboardSync({
      getClient: () => controlClient(),
      ...deviceClipboardIo,
    });
    clipboardSyncRef.current = sync;
    return () => {
      sync.stop();
      clipboardSyncRef.current = null;
    };
  }, []);

  useEffect(() => {
    clipboardSyncRef.current?.setEnabled(
      clipboardSyncEnabled(clipboard.state.status, clipboard.state.value),
    );
  }, [clipboard.state.status, clipboard.state.value]);

  const retryPersistence = useCallback(() => {
    viewer.retry();
    clipboard.retry();
  }, [clipboard.retry, viewer.retry]);

  return {
    clipboardPreferenceControlDisabled: preferenceLocked(clipboard.state.status),
    clipboardShare: clipboard.state.value,
    preferencePersistenceIssue:
      VIEWER_ISSUES[viewer.state.status] ?? CLIPBOARD_ISSUES[clipboard.state.status],
    preferences: viewer.state.value,
    retryPersistence,
    updateClipboardPreference: clipboard.update,
    updateViewerPreferences: viewer.update,
    viewerPreferenceControlsDisabled: preferenceLocked(viewer.state.status),
  };
}
