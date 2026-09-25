import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Pressable,
  ScrollView,
  Text,
  View,
  useWindowDimensions,
} from "react-native";
import { Ionicons } from "@expo/vector-icons";
import { SafeAreaView } from "react-native-safe-area-context";
import { router, useFocusEffect } from "expo-router";
import { applyPanelDensity, panelDensityScale } from "../src/panel-density";
import {
  beginHostSelection,
  captureRequestContext,
  connectHostWithFallback,
  controlClient,
  controlHost,
  controlTarget,
  disconnectHost,
  isHostSelectionCurrent,
  isRequestContextCurrent,
  reconnectHost,
  type SessionRequestContext,
} from "../src/session";
import {
  markPairingStale,
  markUserDisconnected,
  noteAutoReconnectAttempt,
  shouldAutoReconnectFromGate,
  shouldReconnectRetainedContext,
} from "../src/auto-reconnect";
import { handleUnauthorized } from "../src/connect-flow";
import { formatHostEndpoint } from "../src/pairing";
import { isUnauthorizedError, type CatalogView } from "../src/control";
import {
  getRecentHosts,
  mergeAdvertisedRoutes,
  resolveConnectCandidates,
  saveRecentHost,
  type RecentHostItem,
} from "../src/recent-hosts";
import { useAppTheme } from "../src/theme";
import { useAppLanguage } from "../src/i18n";
import { useConnectionLost } from "../src/use-connection-lost";
import { createHubStyles } from "../src/components/hub-styles";
import { ConnectedHeroCard } from "../src/components/ConnectedHeroCard";
import { StandbyHeroCard } from "../src/components/StandbyHeroCard";
import { SetupGuideCard } from "../src/components/SetupGuideCard";
import { FeatureCardsGrid } from "../src/components/FeatureCardsGrid";

let hubAutoAdvancedOnce = false;

function openCatalog() {
  router.push("/catalog");
}

function openHostPicker() {
  router.push("/host");
}

function openPairing() {
  router.push("/pairing");
}

export default function Hub() {
  const { colors, isDark } = useAppTheme();
  const { t, language, toggleLanguage } = useAppLanguage();
  const { width } = useWindowDimensions();
  const density = panelDensityScale(width);
  const styles = useMemo(
    () => applyPanelDensity(createHubStyles(colors, isDark), density),
    [colors, isDark, density],
  );

  const [hostAddr, setHostAddr] = useState<string>("");
  const [isConnected, setIsConnected] = useState<boolean>(false);
  const [lastHost, setLastHost] = useState<RecentHostItem | null>(null);
  const [autoConnecting, setAutoConnecting] = useState<boolean>(false);

  const checkConnection = useCallback(() => {
    const client = controlClient();
    const addr = controlHost();
    setIsConnected(!!client);
    setHostAddr(addr);
  }, []);

  useEffect(() => {
    if (!isConnected || hubAutoAdvancedOnce) return;
    hubAutoAdvancedOnce = true;
    const timer = setTimeout(() => router.push("/catalog"), 400);
    return () => clearTimeout(timer);
  }, [isConnected]);

  const attemptAutoReconnect = useCallback(
    async (target: RecentHostItem | null) => {
      const now = Date.now();
      if (
        target === null ||
        !shouldAutoReconnectFromGate(!!controlClient(), true, now)
      ) {
        return;
      }
      noteAutoReconnectAttempt(now);
      setAutoConnecting(true);
      let context: SessionRequestContext | null = null;
      try {
        const retained = captureRequestContext();
        const useRetained =
          retained !== null && isRequestContextCurrent(retained) &&
          shouldReconnectRetainedContext(retained, target);
        if (useRetained && retained) {
          await reconnectHost(retained);
        } else {
          const selection = beginHostSelection();
          await connectHostWithFallback(
            resolveConnectCandidates(target, []),
            target.port,
            { selection },
          );
          if (!isHostSelectionCurrent(selection)) return;
        }
        context = captureRequestContext();
        if (!context || !isRequestContextCurrent(context)) return;
        let catalog: CatalogView;
        try {
          catalog = await context.client.request<CatalogView>("getCatalog");
        } catch (e) {
          if (isUnauthorizedError(e)) {
            await handleUnauthorized({ context, markStale: true, beforeNavigate: checkConnection });
            return;
          }
          throw e;
        }
        if (!isRequestContextCurrent(context)) return;
        void mergeAdvertisedRoutes(
          controlTarget() ?? { host: target.host, port: target.port },
          catalog,
        ).catch(() => undefined);
        void saveRecentHost(controlTarget()?.host ?? target.host, target.port, target.name);
        checkConnection();
      } catch {
        if (context && disconnectHost(context)) checkConnection();
      } finally {
        setAutoConnecting(false);
      }
    },
    [checkConnection],
  );

  const handleDisconnect = useCallback(() => {
    markUserDisconnected();
    disconnectHost();
    checkConnection();
  }, [checkConnection]);

  useFocusEffect(
    useCallback(() => {
      checkConnection();
      void getRecentHosts().then((hosts) => {
        const target = hosts[0] ?? null;
        setLastHost(target);
        if (!controlClient()) void attemptAutoReconnect(target);
      });
      const client = controlClient();
      if (client) {
        const context = captureRequestContext();
        context?.client.request<CatalogView>("getCatalog").catch((e) => {
          if (isUnauthorizedError(e)) {
            void handleUnauthorized({
              context,
              beforeNavigate: checkConnection,
              navigate: { endpoint: controlHost() },
            });
          } else if (disconnectHost(context)) {
            checkConnection();
          }
        });
      }
    }, [attemptAutoReconnect, checkConnection])
  );

  useConnectionLost(
    useCallback((target: RecentHostItem | null) => {
      setLastHost(target);
      if (!controlClient()) void attemptAutoReconnect(target);
    }, [attemptAutoReconnect]),
    checkConnection,
  );

  return (
    <SafeAreaView style={styles.safeArea} edges={["top", "left", "right", "bottom"]}>
      <ScrollView
        style={styles.root}
        contentContainerStyle={styles.content}
        showsVerticalScrollIndicator={false}
      >
        {/* Subtle, unpretentious top navigation bar */}
        <View style={styles.topBar}>
          <View style={styles.topBarLeft}>
            <View style={styles.topBarDot} />
            <Text style={styles.topBarTitle}>Leftcar</Text>
          </View>
          <Pressable
            onPress={toggleLanguage}
            style={({ pressed }) => [styles.langToggleBtn, pressed && styles.btnPressed]}
            accessibilityRole="button"
            accessibilityLabel={t.common.toggleLanguage}
          >
            <Ionicons name="globe-outline" size={13} color={colors.textSecondary} />
            <Text style={styles.langToggleText}>{language === "ko" ? "EN" : "한국어"}</Text>
          </Pressable>
        </View>

        {/* Hero Connection Card */}
        {isConnected ? (
          <ConnectedHeroCard
            hostAddr={hostAddr}
            styles={styles}
            t={t}
            onOpenCatalog={openCatalog}
            onOpenHostPicker={openHostPicker}
            onDisconnect={handleDisconnect}
          />
        ) : (
          <>
            <StandbyHeroCard
              lastHost={lastHost}
              autoConnecting={autoConnecting}
              colors={colors}
              styles={styles}
              t={t}
              onCheckConnection={checkConnection}
              onOpenHostPicker={openHostPicker}
              onOpenPairing={openPairing}
            />
            {!lastHost && <SetupGuideCard styles={styles} t={t} />}
            {!lastHost && <FeatureCardsGrid styles={styles} t={t} />}
          </>
        )}
      </ScrollView>
    </SafeAreaView>
  );
}
