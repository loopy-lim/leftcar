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
import {
  readUdpStabilitySelection,
  UDP_STABILITY_KEY,
  writeUdpStabilitySelection,
  type UdpStabilitySelection,
} from "./udp-stability";

export type PreferencePersistenceIssue =
  | "viewer-load"
  | "viewer-save"
  | "clipboard-load"
  | "clipboard-save"
  | "udp-load"
  | "udp-save";

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

const UDP_ISSUES: Record<PreferencePersistenceStatus, PreferencePersistenceIssue | null> = {
  loading: null,
  ready: null,
  saving: null,
  "load-error": "udp-load",
  "save-error": "udp-save",
};

const DEFAULT_UDP_STABILITY: UdpStabilitySelection = { profile: "auto" };

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

function createUdpPreferenceController() {
  return new PreferencePersistenceController({
    initialValue: DEFAULT_UDP_STABILITY,
    load: () => readUdpStabilitySelection(SecureStore),
    save: (value: UdpStabilitySelection) => writeUdpStabilitySelection(SecureStore, value),
    storageKey: UDP_STABILITY_KEY,
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
  const udp = usePreferencePersistence(createUdpPreferenceController, DEFAULT_UDP_STABILITY);
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
    udp.retry();
  }, [clipboard.retry, viewer.retry, udp.retry]);

  return {
    preferenceLoading: [viewer.state.status, clipboard.state.status, udp.state.status].some(status => status === "loading"),
    preferenceSaving: [viewer.state.status, clipboard.state.status, udp.state.status].some(status => status === "saving"),
    clipboardPreferenceControlDisabled: preferenceLocked(clipboard.state.status),
    clipboardShare: clipboard.state.value,
    preferencePersistenceIssue:
      VIEWER_ISSUES[viewer.state.status] ?? CLIPBOARD_ISSUES[clipboard.state.status] ?? UDP_ISSUES[udp.state.status],
    preferences: viewer.state.value,
    retryPersistence,
    updateClipboardPreference: clipboard.update,
    updateViewerPreferences: viewer.update,
    udpPreferenceControlsDisabled: preferenceLocked(udp.state.status),
    udpStability: udp.state.value,
    updateUdpPreference: udp.update,
    viewerPreferenceControlsDisabled: preferenceLocked(viewer.state.status),
  };
}
