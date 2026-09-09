import { useCallback, useState } from "react";
import { ActivityIndicator, Pressable, Text, View } from "react-native";
import { interpolate } from "@leftcar/ui-tokens";
import type { ThemeTokens } from "./theme";
import { useAppLanguage } from "./i18n";
import { currentTranslation } from "./language-store";
import { formatErrorMessage } from "./control";
import { requestWithReconnect } from "./catalog-helpers";
import {
  base64ToBytes,
  bytesToBase64,
  listShareQueue,
  mapFileTransferError,
  receiveFile,
  sendFile,
  type FileTransferClient,
  type ShareQueueEntry,
} from "./file-transfer";
import { getFileIo } from "./file-io";

/**
 * 파일 전송 카드(카탈로그 화면): 호스트 공유 대기열 받기 + 문서 선택기로
 * 작은 파일 보내기. 게이트는 호스트 설정이므로 꺼져 있으면 명령이 오류로
 * 답하고, 그 문구를 여기서 번역한다.
 */

/** requestWithReconnect를 file-transfer의 클라이언트 인터페이스로 맞춘다. */
const reconnectClient: FileTransferClient = {
  request: <T,>(command: string, args?: unknown) => requestWithReconnect<T>(command, args),
};

function formatFileBytes(size: number): string {
  if (size >= 1024 * 1024) return `${(size / (1024 * 1024)).toFixed(1)} MB`;
  if (size >= 1024) return `${Math.max(1, Math.round(size / 1024))} KB`;
  return `${size} B`;
}

function fileTransferErrorText(error: unknown): string {
  const key = mapFileTransferError(error);
  if (key) {
    const viewer = currentTranslation().viewer as Record<string, string>;
    const template = viewer[key];
    if (template) return template.replace("{detail}", String(error));
  }
  return formatErrorMessage(error);
}

export function FileTransferCard({ colors }: { colors: ThemeTokens }) {
  const { t } = useAppLanguage();
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [queue, setQueue] = useState<ReadonlyArray<ShareQueueEntry> | null>(null);

  const beginAction = useCallback(() => {
    setBusy(true);
    setError(null);
    setStatus(null);
  }, []);

  const handleSend = useCallback(async () => {
    beginAction();
    try {
      const picked = await getFileIo().pickSendFile();
      if (!picked) {
        setBusy(false);
        return;
      }
      const bytes = base64ToBytes(picked.base64);
      setStatus(interpolate(t.viewer.fileProgress, { percent: 0 }));
      const sent = await sendFile(
        reconnectClient,
        { name: picked.name, bytes },
        (progress) =>
          setStatus(interpolate(t.viewer.fileProgress, { percent: progress.percent })),
      );
      setStatus(interpolate(t.viewer.fileSendDone, { name: sent.name }));
    } catch (cause) {
      setError(fileTransferErrorText(cause));
    } finally {
      setBusy(false);
    }
  }, [beginAction, t]);

  const handleReceive = useCallback(async () => {
    beginAction();
    try {
      const entries = await listShareQueue(reconnectClient);
      setBusy(false);
      if (entries.length === 0) {
        setQueue([]);
        setStatus(t.viewer.fileShareEmpty);
        return;
      }
      setQueue(entries);
    } catch (cause) {
      setError(fileTransferErrorText(cause));
      setBusy(false);
    }
  }, [beginAction, t]);

  const handleFetchEntry = useCallback(
    async (entry: ShareQueueEntry) => {
      beginAction();
      try {
        const received = await receiveFile(
          reconnectClient,
          entry,
          (progress) =>
            setStatus(interpolate(t.viewer.fileProgress, { percent: progress.percent })),
        );
        await getFileIo().saveReceivedFile(
          received.name,
          bytesToBase64(received.bytes),
        );
        setStatus(interpolate(t.viewer.fileReceiveDone, { name: received.name }));
        setQueue(null);
      } catch (cause) {
        setError(fileTransferErrorText(cause));
      } finally {
        setBusy(false);
      }
    },
    [beginAction, t],
  );

  const cardStyle = {
    gap: 10,
    borderRadius: 12,
    borderWidth: 1,
    borderColor: colors.borderSubtle,
    backgroundColor: colors.bgSurface,
    padding: 12,
  };
  const actionButtonStyle = {
    minHeight: 44,
    flex: 1,
    borderRadius: 8,
    borderWidth: 1,
    borderColor: colors.borderSubtle,
    backgroundColor: colors.bgSubtle,
    alignItems: "center" as const,
    justifyContent: "center" as const,
    paddingHorizontal: 12,
    paddingVertical: 8,
    flexDirection: "row" as const,
    gap: 6,
  };

  return (
    <View style={cardStyle}>
      <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>
        {t.viewer.fileTitle}
      </Text>
      <View style={{ flexDirection: "row", gap: 8 }}>
        <Pressable
          style={({ pressed }) => [actionButtonStyle, pressed && !busy && { opacity: 0.7 }]}
          onPress={() => void handleReceive()}
          disabled={busy}
          accessibilityRole="button"
          accessibilityLabel={t.viewer.fileReceive}
        >
          <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>
            {t.viewer.fileReceive}
          </Text>
        </Pressable>
        <Pressable
          style={({ pressed }) => [actionButtonStyle, pressed && !busy && { opacity: 0.7 }]}
          onPress={() => void handleSend()}
          disabled={busy}
          accessibilityRole="button"
          accessibilityLabel={t.viewer.fileSend}
        >
          <Text style={{ fontSize: 13, fontWeight: "700", color: colors.textPrimary }}>
            {t.viewer.fileSend}
          </Text>
        </Pressable>
      </View>

      {busy ? <ActivityIndicator size="small" color={colors.textPrimary} /> : null}
      {status ? (
        <Text style={{ fontSize: 11, lineHeight: 15, color: colors.textSecondary }} numberOfLines={2}>
          {status}
        </Text>
      ) : null}
      {error ? (
        <Text style={{ fontSize: 11, lineHeight: 15, color: colors.textPrimary }} role="alert">
          {error}
        </Text>
      ) : null}
      {queue && queue.length > 0 ? (
        <View style={{ gap: 6 }}>
          {queue.map((entry) => (
            <Pressable
              key={entry.queueId}
              style={({ pressed }) => [
                actionButtonStyle,
                { justifyContent: "space-between" },
                pressed && !busy && { opacity: 0.7 },
              ]}
              onPress={() => void handleFetchEntry(entry)}
              disabled={busy}
              accessibilityRole="button"
              accessibilityLabel={`${t.viewer.fileReceive}: ${entry.name}`}
            >
              <Text style={{ fontSize: 12, color: colors.textPrimary, flexShrink: 1 }} numberOfLines={1}>
                {entry.name}
              </Text>
              <Text style={{ fontSize: 11, color: colors.textMuted }}>
                {formatFileBytes(entry.size)}
              </Text>
            </Pressable>
          ))}
        </View>
      ) : null}
    </View>
  );
}
