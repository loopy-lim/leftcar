import { useCallback, useRef, useState } from "react";
import { ScrollView, View } from "react-native";
import { router, useFocusEffect } from "expo-router";
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
  markUserDisconnected,
  noteAutoReconnectAttempt,
  shouldAutoReconnectFromGate,
  shouldReconnectRetainedContext,
} from "../src/auto-reconnect";
import { handleUnauthorized } from "../src/connect-flow";
import {
  formatErrorMessage,
  isUnauthorizedError,
  type CatalogView,
} from "../src/control";
import {
  getRecentHosts,
  mergeAdvertisedRoutes,
  resolveConnectCandidates,
  saveRecentHost,
  type RecentHostItem,
} from "../src/recent-hosts";
import { useAppLanguage } from "../src/i18n";
import { useConnectionLost } from "../src/use-connection-lost";
import { ConnectedHeroCard } from "../src/components/ConnectedHeroCard";
import { StandbyHeroCard } from "../src/components/StandbyHeroCard";
import { SetupGuideCard } from "../src/components/SetupGuideCard";
import { SafeArea, Action, Label, Notice } from "../src/ui/primitives";

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
  const { t, language, toggleLanguage } = useAppLanguage();

  const [hostAddr, setHostAddr] = useState<string>("");
  const [isConnected, setIsConnected] = useState<boolean>(false);
  const [lastHost, setLastHost] = useState<RecentHostItem | null>(null);
  const [autoConnecting, setAutoConnecting] = useState<boolean>(false);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const focusController = useRef<AbortController | null>(null);
  const [focusSignal, setFocusSignal] = useState<AbortSignal | undefined>();

  const checkConnection = useCallback(() => {
    const client = controlClient();
    const addr = controlHost();
    setIsConnected(!!client);
    setHostAddr(addr);
  }, []);

  const attemptAutoReconnect = useCallback(
    async (target: RecentHostItem | null) => {
      const signal = focusController.current?.signal;
      if (!signal || signal.aborted) return;
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
      let completed = false;
      const cancel = () => {
        if (context && !completed) disconnectHost(context);
      };
      signal.addEventListener("abort", cancel, { once: true });
      try {
        const retained = captureRequestContext();
        const useRetained =
          retained !== null &&
          isRequestContextCurrent(retained) &&
          shouldReconnectRetainedContext(retained, target);
        if (useRetained && retained) {
          context = retained;
          await reconnectHost(retained, signal);
        } else {
          const selection = beginHostSelection();
          await connectHostWithFallback(
            resolveConnectCandidates(target, []),
            target.port,
            { selection, signal },
          );
          if (!isHostSelectionCurrent(selection)) return;
        }
        context = captureRequestContext();
        if (signal.aborted || !context || !isRequestContextCurrent(context))
          return;
        let catalog: CatalogView;
        try {
          catalog = await context.client.request<CatalogView>("getCatalog");
        } catch (e) {
          if (isUnauthorizedError(e)) {
            await handleUnauthorized({
              context,
              signal,
              markStale: true,
              beforeNavigate: checkConnection,
            });
            return;
          }
          throw e;
        }
        if (signal.aborted || !isRequestContextCurrent(context)) return;
        void mergeAdvertisedRoutes(
          controlTarget() ?? { host: target.host, port: target.port },
          catalog,
        ).catch(() => undefined);
        void saveRecentHost(
          controlTarget()?.host ?? target.host,
          target.port,
          target.name,
        );
        completed = true;
        checkConnection();
        if (!hubAutoAdvancedOnce) {
          hubAutoAdvancedOnce = true;
          router.push("/catalog");
        }
      } catch (cause) {
        if (!signal.aborted) setConnectionError(formatErrorMessage(cause));
        if (context && disconnectHost(context)) checkConnection();
      } finally {
        signal.removeEventListener("abort", cancel);
        if (signal.aborted && context && !completed) disconnectHost(context);
        if (!signal.aborted) setAutoConnecting(false);
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
      const controller = new AbortController();
      focusController.current = controller;
      setFocusSignal(controller.signal);
      setAutoConnecting(false);
      setConnectionError(null);
      checkConnection();
      void getRecentHosts().then((hosts) => {
        if (controller.signal.aborted) return;
        const target = hosts[0] ?? null;
        setLastHost(target);
        if (!controlClient()) void attemptAutoReconnect(target);
      });
      const client = controlClient();
      if (client) {
        const context = captureRequestContext();
        context?.client.request<CatalogView>("getCatalog").catch((e) => {
          if (controller.signal.aborted) return;
          if (isUnauthorizedError(e)) {
            void handleUnauthorized({
              context,
              signal: controller.signal,
              beforeNavigate: checkConnection,
              navigate: { endpoint: controlHost() },
            }).catch((cause) => {
              if (
                !controller.signal.aborted &&
                isRequestContextCurrent(context)
              ) {
                setConnectionError(formatErrorMessage(cause));
              }
            });
          } else if (disconnectHost(context)) {
            checkConnection();
          }
        });
      }
      return () => {
        focusController.current?.abort();
        focusController.current = null;
        controller.abort();
      };
    }, [attemptAutoReconnect, checkConnection]),
  );

  useConnectionLost(
    useCallback(
      (target: RecentHostItem | null) => {
        setLastHost(target);
        if (!controlClient()) void attemptAutoReconnect(target);
      },
      [attemptAutoReconnect],
    ),
    checkConnection,
  );

  const cancelReconnect = () => {
    focusController.current?.abort();
    const controller = new AbortController();
    focusController.current = controller;
    setFocusSignal(controller.signal);
    markUserDisconnected();
    setAutoConnecting(false);
    checkConnection();
  };

  return (
    <SafeArea
      className="flex-1 bg-canvas"
      edges={["top", "left", "right", "bottom"]}
    >
      <ScrollView
        className="flex-1"
        contentContainerClassName="mx-auto w-full max-w-2xl gap-6 px-5 pb-8 pt-4"
      >
        <View className="flex-row items-center justify-between gap-3">
          <Label variant="title">Leftcar</Label>
          <Action
            variant="ghost"
            size="compact"
            onPress={toggleLanguage}
            accessibilityLabel={t.common.toggleLanguage}
            label={language === "ko" ? "EN" : "한국어"}
          />
        </View>
        {isConnected ? (
          <ConnectedHeroCard
            hostAddr={hostAddr}
            hostName={
              lastHost && lastHost.host === controlTarget()?.host
                ? lastHost.name
                : undefined
            }
            t={t}
            onOpenCatalog={openCatalog}
            onOpenHostPicker={openHostPicker}
            onDisconnect={handleDisconnect}
          />
        ) : (
          <>
            <StandbyHeroCard
              connectionSignal={focusSignal}
              lastHost={lastHost}
              autoConnecting={autoConnecting}
              t={t}
              onCheckConnection={checkConnection}
              onOpenHostPicker={openHostPicker}
              onOpenPairing={openPairing}
              onCancelReconnect={cancelReconnect}
            />
            {!lastHost && <SetupGuideCard t={t} />}
          </>
        )}
        {connectionError ? (
          <Notice tone="error">
            <Label variant="body">{connectionError}</Label>
            <Action
              variant="secondary"
              onPress={openHostPicker}
              label={t.common.retry}
            />
          </Notice>
        ) : null}
      </ScrollView>
    </SafeArea>
  );
}
