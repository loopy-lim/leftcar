import { useCallback, useState } from "react";
import {
  ActivityIndicator,
  NativeModules,
  Pressable,
  Text,
  View,
} from "react-native";
import { Ionicons } from "@expo/vector-icons";
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
import { useAppTheme } from "../theme";
import { useAppLanguage } from "../i18n";
import type { HubStyles } from "./hub-styles";

const streamLauncher = NativeModules.StreamLauncher as StreamLauncher | undefined;

async function localViewerAddresses(): Promise<string[]> {
  const discovered = streamLauncher?.getLocalIpv4Addresses
    ? await streamLauncher.getLocalIpv4Addresses().catch(() => [])
    : [];
  return discovered.filter((address) => typeof address === "string" && address.length > 0);
}

export interface RecentHostQuickConnectProps {
  item: RecentHostItem;
  styles: HubStyles;
  onFinished: () => void;
}

export function RecentHostQuickConnect({
  item,
  styles,
  onFinished,
}: RecentHostQuickConnectProps) {
  const { colors } = useAppTheme();
  const { t } = useAppLanguage();
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleConnect = useCallback(async () => {
    if (connecting) return;
    setConnecting(true);
    setError(null);
    const selection = beginHostSelection();
    let context: SessionRequestContext | null = null;
    try {
      const connectedHost = await connectHostWithFallback(
        resolveConnectCandidates(item, await localViewerAddresses()),
        item.port,
        { selection },
      );
      if (!isHostSelectionCurrent(selection)) return;
      context = captureRequestContext();
      if (!context || !isRequestContextCurrent(context)) return;
      let catalog: CatalogView;
      try {
        catalog = await context.client.request<CatalogView>("getCatalog");
      } catch (e) {
        if (isUnauthorizedError(e)) {
          await handleUnauthorized({
            context,
            markStale: true,
            navigate: { endpoint: formatHostEndpoint(connectedHost, item.port) },
          });
          return;
        }
        throw e;
      }
      if (!isRequestContextCurrent(context)) return;
      void mergeAdvertisedRoutes(
        controlTarget() ?? { host: connectedHost, port: item.port },
        catalog,
      ).catch(() => undefined);
      void saveRecentHost(connectedHost, item.port, item.name);
      router.push("/catalog");
    } catch (e) {
      if (!isHostSelectionCurrent(selection)) return;
      if (context) disconnectHost(context);
      setError(formatErrorMessage(e));
    } finally {
      setConnecting(false);
      onFinished();
    }
  }, [connecting, item, onFinished]);

  return (
    <View style={styles.recentQuickColumn}>
      <Pressable
        onPress={() => void handleConnect()}
        disabled={connecting}
        style={({ pressed }) => [
          styles.primaryActionBtn,
          { flexDirection: "row", gap: 8, marginTop: 4 },
          pressed && !connecting && styles.btnPressed,
        ]}
        accessibilityRole="button"
        accessibilityLabel={`${t.viewer.btnOpenScreen}: ${item.name || item.host}`}
      >
        {connecting ? (
          <ActivityIndicator size="small" color={colors.btnPrimaryText} />
        ) : (
          <Ionicons name="play" size={16} color={colors.btnPrimaryText} />
        )}
        <Text style={styles.primaryActionText}>
          {connecting ? t.viewer.connectingToHost : t.viewer.btnOpenScreen}
        </Text>
      </Pressable>
      {error ? <Text style={styles.recentQuickError}>{error}</Text> : null}
    </View>
  );
}

export default RecentHostQuickConnect;
