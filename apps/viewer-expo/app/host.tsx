import { useCallback, useEffect, useRef, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  NativeEventEmitter,
  NativeModules,
  ScrollView,
  View,
} from "react-native";
import { router, useFocusEffect } from "expo-router";
import {
  beginHostSelection,
  isHostSelectionCurrent,
  isRequestContextCurrent,
  type SessionRequestContext,
  captureRequestContext,
  connectHost,
  controlTarget,
  disconnectHost,
} from "../src/session";
import {
  formatErrorMessage,
  isUnauthorizedError,
  type CatalogView,
} from "../src/control";
import { DEFAULT_CONTROL_PORT } from "../src/defaults";
import { handleUnauthorized } from "../src/connect-flow";
import {
  clearStoredCredential,
  clearToken,
  formatHostEndpoint,
  getStoredToken,
  isTrustedHost,
  parseHostEndpoint,
} from "../src/pairing";
import {
  clearRecentHosts,
  getRecentHosts,
  removeRecentHost,
  saveRecentHostStrict,
  type RecentHostItem,
} from "../src/recent-hosts";
import {
  DISCOVERY_HINT_DELAY_MS,
  shouldShowDiscoveryHint,
} from "../src/discovery-hint";
import { useAppLanguage } from "../src/i18n";
import { interpolate } from "@leftcar/ui-tokens";
import {
  SafeArea,
  Action,
  Field,
  Label,
  Notice,
  Surface,
} from "../src/ui/primitives";

interface HostConnectionAction {
  controller: AbortController;
  context: SessionRequestContext | null;
  completed: boolean;
}

type NsdNative = {
  startDiscovery(): void;
  stopDiscovery(): void;
};

const nsd = NativeModules.NsdDiscovery as NsdNative | undefined;

interface FoundHost {
  name: string;
  host: string;
  port: number;
}

function formatRelativeTime(timestamp: number, language: "ko" | "en"): string {
  const diffSecs = Math.max(0, Math.floor((Date.now() - timestamp) / 1000));
  if (diffSecs < 60) {
    return language === "ko" ? "방금 전" : "Just now";
  }
  const diffMins = Math.floor(diffSecs / 60);
  if (diffMins < 60) {
    return language === "ko" ? `${diffMins}분 전` : `${diffMins}m ago`;
  }
  const diffHours = Math.floor(diffMins / 60);
  if (diffHours < 24) {
    return language === "ko" ? `${diffHours}시간 전` : `${diffHours}h ago`;
  }
  const diffDays = Math.floor(diffHours / 24);
  return language === "ko" ? `${diffDays}일 전` : `${diffDays}d ago`;
}

function useHostModel() {
  const { t, language } = useAppLanguage();

  const [ip, setIp] = useState("");
  const [connectingTarget, setConnectingTarget] = useState<string | null>(null);
  const busy = connectingTarget !== null;
  const [error, setError] = useState<string | null>(null);
  const [found, setFound] = useState<Record<string, FoundHost>>({});
  const [hasStoredToken, setHasStoredToken] = useState(false);
  const [recentHosts, setRecentHosts] = useState<RecentHostItem[]>([]);
  const [showTroubleshoot, setShowTroubleshoot] = useState(false);
  const [discoverySettled, setDiscoverySettled] = useState(false);
  const [discoveryError, setDiscoveryError] = useState(false);
  const discoveryTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [manualOpen, setManualOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const connectionAction = useRef<HostConnectionAction | null>(null);
  useFocusEffect(
    useCallback(() => {
      setConnectingTarget(null);
      return () => {
        const action = connectionAction.current;
        connectionAction.current = null;
        action?.controller.abort();
        if (action?.context && !action.completed)
          disconnectHost(action.context);
      };
    }, []),
  );

  useEffect(() => {
    discoveryTimer.current = setTimeout(
      () => setDiscoverySettled(true),
      DISCOVERY_HINT_DELAY_MS,
    );
    return () => {
      if (discoveryTimer.current) clearTimeout(discoveryTimer.current);
    };
  }, []);

  useEffect(() => {
    // 검증된 세션은 stable identity 자격 증명을 우선 본다. USB/레거시처럼
    // identity가 없을 때만 엔드포인트 저장소를 조회한다.
    const context = captureRequestContext();
    const target = controlTarget();
    let active = true;
    if (context?.credential) setHasStoredToken(true);
    else if (target)
      void getStoredToken(target)
        .then((token) => {
          if (active) setHasStoredToken(!!token);
        })
        .catch((cause) => {
          if (active) setError(formatErrorMessage(cause));
        });
    void getRecentHosts()
      .then((hosts) => {
        if (active) setRecentHosts(hosts);
      })
      .catch((cause) => {
        if (active) setError(formatErrorMessage(cause));
      });
    return () => {
      active = false;
    };
  }, []);

  // 파괴적 동작은 실행 전에 한 번 확인한다 — 뒤늦은 "삭제했습니다" 알림은
  // 되돌릴 수 없다. 지우기는 확인 다이얼로그의 [지우기]에서만 일어난다.
  const handleClearToken = useCallback(() => {
    const origin = captureRequestContext();
    const originalTarget = controlTarget();
    Alert.alert(t.viewer.clearTokenAlertTitle, t.viewer.clearTokenAlertDesc, [
      { text: t.common.cancel, style: "cancel" },
      {
        text: t.viewer.deleteHost,
        style: "destructive",
        onPress: () => {
          const context = origin;
          const target = originalTarget;
          if (context && !isRequestContextCurrent(context)) return;
          const clear = context?.credential
            ? clearStoredCredential(context.credential)
            : target
              ? clearToken(target)
              : Promise.resolve();
          void clear
            .then(() => {
              disconnectHost(context ?? undefined);
              setHasStoredToken(false);
            })
            .catch((cause) => {
              if (!context || isRequestContextCurrent(context)) {
                setError(formatErrorMessage(cause));
              }
            });
        },
      },
    ]);
  }, [t]);

  const handleRemoveRecentHost = useCallback(
    async (hostItem: RecentHostItem) => {
      if (deleting) return;
      setDeleting(true);
      try {
        setRecentHosts(await removeRecentHost(hostItem.host, hostItem.port));
      } catch (cause) {
        setError(formatErrorMessage(cause));
      } finally {
        setDeleting(false);
      }
    },
    [deleting],
  );
  const handleClearAllRecent = useCallback(async () => {
    if (deleting) return;
    setDeleting(true);
    try {
      await clearRecentHosts();
      setRecentHosts([]);
    } catch (cause) {
      setError(formatErrorMessage(cause));
    } finally {
      setDeleting(false);
    }
  }, [deleting]);

  useFocusEffect(
    useCallback(() => {
      if (!nsd) return;
      const emitter = new NativeEventEmitter(nsd as never);
      const sub1 = emitter.addListener("leftcar:host-found", (raw) => {
        const h = raw as FoundHost;
        setFound((prev) => ({ ...prev, [h.host]: h }));
      });
      const sub2 = emitter.addListener("leftcar:host-lost", (serviceName) => {
        setFound((prev) =>
          Object.fromEntries(
            Object.entries(prev).filter(
              ([, host]) => host.name !== String(serviceName),
            ),
          ),
        );
      });
      const failed = emitter.addListener("leftcar:discovery-failed", () =>
        setDiscoveryError(true),
      );
      setDiscoveryError(false);
      nsd.startDiscovery();
      return () => {
        nsd.stopDiscovery();
        sub1.remove();
        sub2.remove();
        failed.remove();
      };
    }, []),
  );

  useEffect(() => {
    setError(null);
  }, [ip]);

  const doConnect = useCallback(
    async (target: string, port = DEFAULT_CONTROL_PORT) => {
      connectionAction.current?.controller.abort();
      const action: HostConnectionAction = {
        controller: new AbortController(),
        context: null,
        completed: false,
      };
      connectionAction.current = action;
      const selection = beginHostSelection();
      const cancelSelection = () => action.controller.abort();
      selection.signal.addEventListener("abort", cancelSelection, {
        once: true,
      });
      const isCurrent = () =>
        connectionAction.current === action &&
        !action.controller.signal.aborted &&
        isHostSelectionCurrent(selection);
      setConnectingTarget(target);
      setError(null);
      try {
        if (!isTrustedHost(target)) throw new Error(t.viewer.trustedHostError);
        for (let attempt = 0; attempt < 3; attempt += 1) {
          if (!isCurrent()) return;
          try {
            await connectHost(target, port, {
              selection,
              signal: action.controller.signal,
            });
            break;
          } catch (e) {
            if (!isCurrent() || (e instanceof Error && e.name === "AbortError"))
              return;
            if (attempt === 2) throw e;
            await new Promise((resolve) =>
              setTimeout(resolve, 300 * (attempt + 1)),
            );
          }
        }
        if (!isCurrent()) return;
        const context = captureRequestContext();
        if (!context || !isRequestContextCurrent(context)) return;
        action.context = context;
        try {
          await context.client.request<CatalogView>("getCatalog");
        } catch (e) {
          if (!isCurrent() || !isRequestContextCurrent(context)) return;
          if (isUnauthorizedError(e)) {
            await handleUnauthorized({
              context,
              signal: action.controller.signal,
              beforeNavigate: () => setHasStoredToken(false),
              navigate: { endpoint: formatHostEndpoint(target, port) },
            });
            return;
          }
          throw e;
        }
        if (!isCurrent() || !isRequestContextCurrent(context)) return;
        const matched = Object.values(found).find((h) => h.host === target);
        // Recent-host persistence is best effort, but its late UI publication and
        // navigation still belong to this exact action and authenticated context.
        const recent = await saveRecentHostStrict(
          target,
          port,
          matched?.name,
          undefined,
          action.controller.signal,
        ).catch(() => null);
        if (!isCurrent() || !isRequestContextCurrent(context)) return;
        if (recent) setRecentHosts(recent);
        setHasStoredToken(true);
        action.completed = true;
        router.push("/catalog");
      } catch (e) {
        if (isCurrent()) setError(formatErrorMessage(e));
      } finally {
        selection.signal.removeEventListener("abort", cancelSelection);
        if (action.context && !action.completed) disconnectHost(action.context);
        if (connectionAction.current === action) {
          connectionAction.current = null;
          setConnectingTarget(null);
        }
      }
    },
    [found, t],
  );

  const hosts = Object.values(found);

  const showDiscoveryHint = shouldShowDiscoveryHint({
    hostCount: hosts.length,
    elapsedMs: discoverySettled ? DISCOVERY_HINT_DELAY_MS : 0,
  });

  const connectManual = useCallback(() => {
    const endpoint = parseHostEndpoint(ip);
    if (!endpoint) {
      setError(t.viewer.invalidHostError);
      return;
    }
    void doConnect(endpoint.host, endpoint.port);
  }, [doConnect, ip, t]);

  const retryDiscovery = () => {
    setDiscoveryError(false);
    setDiscoverySettled(false);
    nsd?.stopDiscovery();
    nsd?.startDiscovery();
    if (discoveryTimer.current) clearTimeout(discoveryTimer.current);
    discoveryTimer.current = setTimeout(
      () => setDiscoverySettled(true),
      DISCOVERY_HINT_DELAY_MS,
    );
  };
  const cancelConnection = () => {
    const action = connectionAction.current;
    connectionAction.current = null;
    action?.controller.abort();
    if (action?.context && !action.completed) disconnectHost(action.context);
    setConnectingTarget(null);
  };
  const discoveredKeys = new Set(hosts.map((h) => `${h.host}:${h.port}`));
  return {
    t,
    language,
    hosts,
    recentHosts,
    busy,
    connectingTarget,
    error,
    discoveryError,
    showDiscoveryHint,
    discoverySettled,
    manualOpen,
    ip,
    deleting,
    showTroubleshoot,
    hasStoredToken,
    discoveredKeys,
    doConnect,
    cancelConnection,
    retryDiscovery,
    handleClearAllRecent,
    handleRemoveRecentHost,
    setManualOpen,
    setIp,
    connectManual,
    setShowTroubleshoot,
    handleClearToken,
  };
}
type HostModel = ReturnType<typeof useHostModel>;

function HostRow({
  item,
  recent,
  model,
}: {
  item: FoundHost | RecentHostItem;
  recent: boolean;
  model: HostModel;
}) {
  const {
    t,
    language,
    connectingTarget,
    busy,
    deleting,
    doConnect,
    handleRemoveRecentHost,
    discoveredKeys,
  } = model;
  return (
    <Surface
      variant="inset"
      key={`${item.host}:${item.port}`}
      className="gap-2"
    >
      <View className="flex-row items-center gap-2">
        <Action
          variant="secondary"
          className="flex-1"
          label={item.name || t.common.myComputer}
          accessibilityLabel={`${t.common.connect}: ${item.name || item.host}, ${formatHostEndpoint(item.host, item.port)}`}
          busy={connectingTarget === item.host}
          disabled={busy}
          onPress={() => void doConnect(item.host, item.port)}
        />
        {recent ? (
          <Action
            variant="ghost"
            size="icon"
            label="×"
            disabled={deleting || busy}
            accessibilityLabel={`${t.viewer.deleteHost}: ${item.name || item.host}`}
            onPress={() => void handleRemoveRecentHost(item as RecentHostItem)}
          />
        ) : null}
      </View>
      <Label variant="code" tone="muted" selectable>
        {formatHostEndpoint(item.host, item.port)}
      </Label>
      {recent && "lastConnected" in item ? (
        <Label variant="caption" tone="muted">
          {interpolate(t.viewer.lastConnected, {
            time: formatRelativeTime(item.lastConnected, language),
          })}
          {discoveredKeys.has(`${item.host}:${item.port}`)
            ? ` · ${t.viewer.discoveredNow}`
            : ""}
        </Label>
      ) : null}
    </Surface>
  );
}

function DiscoverySection({ model }: { model: HostModel }) {
  const {
    t,
    discoveryError,
    hosts,
    discoverySettled,
    showDiscoveryHint,
    retryDiscovery,
  } = model;
  if (discoveryError || !nsd)
    return (
      <Surface className="gap-3">
        <Label variant="title">{t.viewer.searchTitle}</Label>
        <Notice>
          <Label>
            {discoveryError
              ? t.viewer.discoveryFailed
              : t.viewer.discoveryUnavailable}
          </Label>
          {nsd ? (
            <Action
              variant="secondary"
              onPress={retryDiscovery}
              label={t.common.retry}
            />
          ) : null}
        </Notice>
      </Surface>
    );
  return (
    <Surface className="gap-3">
      <Label variant="title">{t.viewer.searchTitle}</Label>
      {hosts.length ? (
        hosts.map((item) => (
          <HostRow
            key={`${item.host}:${item.port}`}
            item={item}
            recent={false}
            model={model}
          />
        ))
      ) : (
        <View className="gap-3 py-4">
          {!discoverySettled ? (
            <ActivityIndicator accessibilityLabel={t.viewer.searching} />
          ) : null}
          <Label accessibilityLiveRegion="polite">
            {showDiscoveryHint ? t.viewer.discoveryEmpty : t.viewer.searching}
          </Label>
          <Label tone="muted">
            {showDiscoveryHint
              ? t.viewer.connectHintBody
              : t.viewer.emptyHostsDesc}
          </Label>
          {showDiscoveryHint ? (
            <Action
              variant="secondary"
              onPress={retryDiscovery}
              label={t.common.refresh}
            />
          ) : null}
        </View>
      )}
    </Surface>
  );
}
function RecentSection({ model }: { model: HostModel }) {
  const { t, recentHosts, deleting, handleClearAllRecent } = model;
  if (!recentHosts.length) return null;
  return (
    <Surface className="gap-3">
      <View className="flex-row items-center justify-between gap-2">
        <Label variant="title" className="flex-1">
          {t.viewer.recentHostsTitle}
        </Label>
        <Action
          variant="ghost"
          size="compact"
          busy={deleting}
          onPress={() => void handleClearAllRecent()}
          label={t.viewer.clearRecentHosts}
        />
      </View>
      {recentHosts.map((item) => (
        <HostRow
          key={`${item.host}:${item.port}`}
          item={item}
          recent
          model={model}
        />
      ))}
    </Surface>
  );
}
function ManualSection({ model }: { model: HostModel }) {
  const { t, ip, busy, manualOpen, setManualOpen, setIp, connectManual } =
    model;
  return (
    <Surface className="gap-3">
      <Action
        variant="secondary"
        onPress={() => router.push("/pairing")}
        label={t.viewer.btnQrConnect}
      />
      <Action
        variant="ghost"
        accessibilityState={{ expanded: manualOpen }}
        onPress={() => setManualOpen(!manualOpen)}
        label={t.viewer.manualTitle}
      />
      {manualOpen ? (
        <View className="gap-3">
          <Label tone="muted">{t.viewer.manualDesc}</Label>
          <View className="flex-row items-center gap-2">
            <Field
              className="flex-1"
              value={ip}
              onChangeText={setIp}
              accessibilityLabel={t.viewer.manualTitle}
              placeholder={t.viewer.manualPlaceholder}
              keyboardType="url"
              autoCapitalize="none"
              autoCorrect={false}
              editable={!busy}
              onSubmitEditing={connectManual}
              returnKeyType="go"
            />
            {ip ? (
              <Action
                variant="ghost"
                size="icon"
                label="×"
                accessibilityLabel={t.common.cancel}
                onPress={() => setIp("")}
              />
            ) : null}
          </View>
          <Action
            onPress={connectManual}
            busy={busy}
            disabled={!ip.trim()}
            label={t.viewer.btnConnectAction}
          />
        </View>
      ) : null}
    </Surface>
  );
}
function ConnectionHelp({ model }: { model: HostModel }) {
  const { t, showTroubleshoot, setShowTroubleshoot } = model;
  return (
    <Surface className="gap-3">
      <Action
        variant="ghost"
        accessibilityState={{ expanded: showTroubleshoot }}
        onPress={() => setShowTroubleshoot(!showTroubleshoot)}
        label={t.viewer.troubleshootTitle}
      />
      {showTroubleshoot ? (
        <View className="gap-4">
          {[
            [t.viewer.troubleshootWifi, t.viewer.troubleshootWifiDesc],
            [t.viewer.troubleshootAp, t.viewer.troubleshootApDesc],
            [t.viewer.troubleshootManual, t.viewer.troubleshootManualDesc],
            [
              t.viewer.troubleshootHiddenWindow,
              t.viewer.troubleshootHiddenWindowDesc,
            ],
          ].map(([title, description]) => (
            <View key={title} className="gap-1">
              <Label>{title}</Label>
              <Label variant="caption" tone="muted">
                {description}
              </Label>
            </View>
          ))}
        </View>
      ) : null}
    </Surface>
  );
}
export default function Host() {
  const model = useHostModel();
  const { t } = model;
  return (
    <SafeArea className="flex-1 bg-canvas" edges={["left", "right", "bottom"]}>
      <ScrollView
        className="flex-1"
        contentContainerClassName="mx-auto w-full max-w-3xl gap-5 px-4 pb-8 pt-4"
        keyboardShouldPersistTaps="handled"
      >
        {model.busy ? (
          <Notice>
            <View className="flex-row items-center gap-3">
              <ActivityIndicator />
              <Label className="flex-1" accessibilityLiveRegion="polite">
                {t.viewer.connectingToHost}
              </Label>
            </View>
            <Action
              variant="secondary"
              onPress={model.cancelConnection}
              label={t.common.cancel}
            />
          </Notice>
        ) : null}
        {model.error ? (
          <Notice tone="error">
            <Label>{model.error}</Label>
            <Action
              variant="secondary"
              onPress={() => model.setShowTroubleshoot(true)}
              label={t.viewer.troubleshootTitle}
            />
          </Notice>
        ) : null}
        <DiscoverySection model={model} />
        <RecentSection model={model} />
        <ManualSection model={model} />
        <ConnectionHelp model={model} />
        {model.hasStoredToken ? (
          <Surface className="gap-2">
            <Label variant="title">{t.viewer.rememberTitle}</Label>
            <Action
              variant="ghost"
              onPress={model.handleClearToken}
              label={t.viewer.btnClearToken}
            />
          </Surface>
        ) : null}
      </ScrollView>
    </SafeArea>
  );
}
