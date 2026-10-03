import { useCallback, useEffect, useRef, useState } from "react";
import { View } from "react-native";
import { interpolate } from "@leftcar/ui-tokens";
import type { ThemeTokens } from "./theme";
import { useAppLanguage } from "./i18n";
import { currentTranslation } from "./language-store";
import { formatErrorMessage } from "./control";
import { requestForCurrentSelection } from "./catalog-helpers";
import {
  captureRequestContext,
  isRequestContextCurrent,
  type SessionRequestContext,
} from "./session";
import {
  listShareQueue,
  mapFileTransferError,
  receiveFile,
  sendFile,
  type FileTransferClient,
  type ShareQueueEntry,
} from "./file-transfer";
import { getFileIo } from "./file-io";
import { Action, Label, Notice, Surface } from "./ui/primitives";

type TransferAction = {
  controller: AbortController;
  origin: SessionRequestContext;
  client: FileTransferClient;
};
function formatFileBytes(size: number): string {
  if (size >= 1024 * 1024) return `${(size / (1024 * 1024)).toFixed(1)} MB`;
  if (size >= 1024) return `${Math.max(1, Math.round(size / 1024))} KB`;
  return `${size} B`;
}
function fileTransferErrorText(error: unknown): string {
  const key = mapFileTransferError(error);
  const viewer = currentTranslation().viewer as Record<string, string>;
  return key && viewer[key]
    ? viewer[key].replace("{detail}", String(error))
    : formatErrorMessage(error);
}
export function FileTransferCard({ colors: _colors }: { colors: ThemeTokens }) {
  const { t } = useAppLanguage();
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [destination, setDestination] = useState<string | null>(null);
  const [percent, setPercent] = useState<number | null>(null);
  const [queue, setQueue] = useState<ReadonlyArray<ShareQueueEntry> | null>(
    null,
  );
  const queueOrigin = useRef<SessionRequestContext | null>(null);
  const retryAction = useRef<(() => Promise<void>) | null>(null);
  const actionRef = useRef<TransferAction | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      actionRef.current?.controller.abort();
    };
  }, []);
  const beginAction = useCallback((): TransferAction | null => {
    if (actionRef.current) return null;
    const origin = captureRequestContext();
    if (!origin) {
      setError(t.viewer.notConnectedError);
      return null;
    }
    const action = {
      controller: new AbortController(),
      origin,
      client: {
        request: requestForCurrentSelection(
          `${origin.target.host}:${origin.target.port}`,
        ),
      },
    };
    actionRef.current = action;
    setBusy(true);
    setError(null);
    setStatus(null);
    setPercent(null);
    setDestination(null);
    return action;
  }, [t]);
  const current = (action: TransferAction) => {
    const context = captureRequestContext();
    return (
      mounted.current &&
      actionRef.current === action &&
      !action.controller.signal.aborted &&
      context !== null &&
      isRequestContextCurrent(context) &&
      context.selectionGeneration === action.origin.selectionGeneration
    );
  };
  const assertCurrent = (action: TransferAction) => {
    if (
      !current(action) ||
      captureRequestContext()?.selectionGeneration !==
        action.origin.selectionGeneration
    ) {
      const failure = new Error(t.viewer.fileHostChanged);
      failure.name = "AbortError";
      throw failure;
    }
  };
  const finish = (action: TransferAction) => {
    if (actionRef.current === action) {
      actionRef.current = null;
      if (mounted.current) setBusy(false);
    }
  };
  const failed = (action: TransferAction, cause: unknown) => {
    if (
      mounted.current &&
      actionRef.current === action &&
      !action.controller.signal.aborted
    ) {
      setPercent(null);
      setError(fileTransferErrorText(cause));
    }
  };
  const handleSend = async () => {
    const action = beginAction();
    if (!action) return;
    retryAction.current = handleSend;
    try {
      setStatus(t.viewer.fileChoosing);
      const file = await getFileIo().pickSendFile();
      assertCurrent(action);
      if (!file) {
        setStatus(null);
        return;
      }
      setStatus(
        `${t.viewer.fileSending}: ${file.name} · ${formatFileBytes(file.size)}`,
      );
      const sent = await sendFile(
        action.client,
        file,
        (progress) => {
          if (current(action)) setPercent(progress.percent);
        },
        { signal: action.controller.signal },
      );
      assertCurrent(action);
      setStatus(interpolate(t.viewer.fileSendDone, { name: sent.name }));
      setDestination(sent.path);
      setPercent(null);
    } catch (cause) {
      failed(action, cause);
    } finally {
      finish(action);
    }
  };
  const handleReceive = async () => {
    const action = beginAction();
    if (!action) return;
    retryAction.current = handleReceive;
    try {
      setStatus(t.viewer.fileLoading);
      const entries = await listShareQueue(action.client);
      assertCurrent(action);
      queueOrigin.current = action.origin;
      setQueue(entries);
      setStatus(entries.length ? null : t.viewer.fileShareEmpty);
    } catch (cause) {
      failed(action, cause);
    } finally {
      finish(action);
    }
  };
  const handleFetchEntry = async (entry: ShareQueueEntry) => {
    if (
      !queueOrigin.current ||
      captureRequestContext()?.selectionGeneration !==
        queueOrigin.current.selectionGeneration
    ) {
      setQueue(null);
      setError(t.viewer.fileHostChanged);
      return;
    }
    const action = beginAction();
    if (!action) return;
    retryAction.current = () => handleFetchEntry(entry);
    try {
      setStatus(`${t.viewer.fileReceiving}: ${entry.name}`);
      setPercent(0);
      const received = await receiveFile(action.client, entry, {
        signal: action.controller.signal,
        createSink: async (name) => {
          assertCurrent(action);
          return getFileIo().createReceivedSink(name);
        },
        onProgress: (progress) => {
          if (current(action)) setPercent(progress.percent);
        },
      });
      assertCurrent(action);
      setStatus(interpolate(t.viewer.fileReceiveDone, { name: received.name }));
      setDestination(received.path);
      setPercent(null);
      setQueue(
        (items) =>
          items?.filter((item) => item.queueId !== entry.queueId) ?? null,
      );
    } catch (cause) {
      failed(action, cause);
    } finally {
      finish(action);
    }
  };
  const cancel = () => {
    actionRef.current?.controller.abort();
    setStatus(t.viewer.fileCancelled);
    setPercent(null);
  };
  const host = captureRequestContext()?.target;
  return (
    <Surface className="gap-4">
      {host ? (
        <Label variant="code" tone="muted">
          {host.host}:{host.port}
        </Label>
      ) : null}
      <Label tone="muted">{t.viewer.fileHostGateHint}</Label>
      <View className="flex-row flex-wrap gap-2">
        <Action
          variant="secondary"
          className="grow"
          label={t.viewer.fileReceive}
          disabled={busy}
          onPress={() => void handleReceive()}
        />
        <Action
          variant="secondary"
          className="grow"
          label={t.viewer.fileSend}
          disabled={busy}
          onPress={() => void handleSend()}
        />
      </View>
      {status ? <Label accessibilityLiveRegion="polite">{status}</Label> : null}
      {percent !== null ? (
        <View
          className="gap-2"
          accessibilityRole="progressbar"
          accessibilityValue={{ min: 0, max: 100, now: percent }}
          accessibilityLabel={t.viewer.fileTitle}
        >
          <View className="h-1 overflow-hidden rounded bg-subtle">
            <View className="h-full bg-ink" style={{ width: `${percent}%` }} />
          </View>
          <Label variant="code">
            {interpolate(t.viewer.fileProgress, { percent })}
          </Label>
        </View>
      ) : null}
      {busy ? (
        <Action variant="ghost" onPress={cancel} label={t.common.cancel} />
      ) : null}
      {error ? (
        <Notice tone="error">
          <Label>{error}</Label>
          <Action
            variant="secondary"
            disabled={busy}
            onPress={() => {
              if (retryAction.current) void retryAction.current();
            }}
            label={t.common.retry}
          />
        </Notice>
      ) : null}
      {destination ? (
        <Label variant="caption" tone="muted" selectable>
          {interpolate(t.viewer.fileSavedAt, { path: destination })}
        </Label>
      ) : null}
      {queue?.map((entry) => (
        <Surface variant="inset" key={entry.queueId}>
          <Label>{entry.name}</Label>
          <Label variant="code" tone="muted">
            {formatFileBytes(entry.size)}
          </Label>
          <Action
            variant="secondary"
            label={t.viewer.fileReceive}
            accessibilityLabel={`${t.viewer.fileReceive}: ${entry.name}`}
            disabled={busy}
            onPress={() => void handleFetchEntry(entry)}
          />
        </Surface>
      ))}
    </Surface>
  );
}
