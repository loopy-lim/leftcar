import { useCallback, useState } from "react";
import { NativeModules, View } from "react-native";
import { router } from "expo-router";
import type { StreamLauncher } from "../launch-stream";
import {
  beginHostSelection,
  captureRequestContext,
  connectHostWithFallback,
  controlTarget,
  disconnectHost,
  isHostSelectionCurrent,
  isRequestContextCurrent,
  type SessionRequestContext,
} from "../session";
import { handleUnauthorized } from "../connect-flow";
import { formatHostEndpoint } from "../pairing";
import {
  formatErrorMessage,
  isUnauthorizedError,
  type CatalogView,
} from "../control";
import {
  mergeAdvertisedRoutes,
  resolveConnectCandidates,
  saveRecentHost,
  type RecentHostItem,
} from "../recent-hosts";
import { useAppLanguage } from "../i18n";
import { Action, Label, Notice } from "../ui/primitives";

const streamLauncher = NativeModules.StreamLauncher as
  StreamLauncher | undefined;

async function localViewerAddresses(): Promise<string[]> {
  const discovered = streamLauncher?.getLocalIpv4Addresses
    ? await streamLauncher.getLocalIpv4Addresses().catch(() => [])
    : [];
  return discovered.filter(
    (address) => typeof address === "string" && address.length > 0,
  );
}

export interface RecentHostQuickConnectProps {
  signal: AbortSignal | undefined;
  item: RecentHostItem;
  onFinished: () => void;
}

export function RecentHostQuickConnect({
  signal,
  item,
  onFinished,
}: RecentHostQuickConnectProps) {
  const { t } = useAppLanguage();
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleConnect = useCallback(async () => {
    if (connecting || !signal || signal.aborted) return;
    setConnecting(true);
    setError(null);
    const selection = beginHostSelection();
    let context: SessionRequestContext | null = null;
    let completed = false;
    const cancel = () => {
      if (context && !completed) disconnectHost(context);
    };
    signal.addEventListener("abort", cancel, { once: true });
    try {
      const connectedHost = await connectHostWithFallback(
        resolveConnectCandidates(item, await localViewerAddresses()),
        item.port,
        { selection, signal },
      );
      if (signal.aborted || !isHostSelectionCurrent(selection)) return;
      context = captureRequestContext();
      if (!context || !isRequestContextCurrent(context)) return;
      let catalog: CatalogView;
      try {
        catalog = await context.client.request<CatalogView>("getCatalog");
      } catch (e) {
        if (isUnauthorizedError(e)) {
          await handleUnauthorized({
            context,
            signal,
            markStale: true,
            navigate: {
              endpoint: formatHostEndpoint(connectedHost, item.port),
            },
          });
          return;
        }
        throw e;
      }
      if (signal.aborted || !isRequestContextCurrent(context)) return;
      void mergeAdvertisedRoutes(
        controlTarget() ?? { host: connectedHost, port: item.port },
        catalog,
      ).catch(() => undefined);
      void saveRecentHost(connectedHost, item.port, item.name);
      completed = true;
      router.push("/catalog");
    } catch (e) {
      if (signal.aborted || !isHostSelectionCurrent(selection)) return;
      if (context) disconnectHost(context);
      setError(formatErrorMessage(e));
    } finally {
      signal.removeEventListener("abort", cancel);
      if (signal.aborted && context && !completed) disconnectHost(context);
      if (!signal.aborted) {
        setConnecting(false);
        onFinished();
      }
    }
  }, [connecting, item, onFinished, signal]);

  return (
    <View className="gap-3">
      <Action
        onPress={() => void handleConnect()}
        busy={connecting}
        label={connecting ? t.viewer.connectingToHost : t.viewer.btnOpenScreen}
        accessibilityLabel={`${t.viewer.btnOpenScreen}: ${item.name || item.host}`}
      />
      {error ? (
        <Notice tone="error">
          <Label>{error}</Label>
          <Action
            variant="secondary"
            onPress={() => void handleConnect()}
            label={t.common.retry}
          />
        </Notice>
      ) : null}
    </View>
  );
}
export default RecentHostQuickConnect;
