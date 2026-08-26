import {
  type Dispatch,
  type SetStateAction,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ActivityIndicator,
  Alert,
  FlatList,
  type ListRenderItemInfo,
  Pressable,
  RefreshControl,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { Ionicons } from "@expo/vector-icons";
import { NativeModules } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import { router } from "expo-router";
import { allocPort, controlClient, controlHost, disconnectHost, reconnectHost } from "../src/session";
import { clearToken } from "../src/pairing";
import {
  startPreparedStream,
  type StreamLauncher,
} from "../src/launch-stream";
import {
  formatErrorMessage,
  isControlTransportError,
  isUnauthorizedError,
  preferredCaptureBackend,
  type CatalogView,
  type DisplayInfo,
  type StatusView,
} from "../src/control";
import { resolveStreamResolution } from "../src/stream-resolution";
import {
  getUsbState,
  resolveTransport,
  subscribeUsbState,
  type ResolvedTransport,
  type UsbAccessoryState,
} from "../src/usb";
import { shouldSwitchTransport } from "../src/transport-switch";
import {
  STREAM_PROFILES,
  type StreamProfile,
  type StreamProfileId,
} from "../src/stream-profile";

const launcher = NativeModules.StreamLauncher as StreamLauncher | undefined;

interface ActiveStream {
  port: number;
  session: number;
  sourceIndex: number;
  sourceName: string;
  width: number;
  height: number;
  fps: number;
  captureBackend: string;
  contentMode: StreamProfile["contentMode"];
  mediaTransport: ResolvedTransport;
  viewerIps: string[];
  startedAt: number;
}

interface RestoredStream {
  session: number;
  viewerIps: string[];
  captureBackend: string;
  mediaTransport: ResolvedTransport;
}

const HIDABLE_DISPLAY_LABELS = ["leftcar hub", "leftcarhub"];

function isHubDisplay(name: string): boolean {
  const normalized = name.trim().toLowerCase();
  return HIDABLE_DISPLAY_LABELS.some((label) => normalized.includes(label));
}

function catalogDisplayHost(catalogHost: string): string {
  return catalogHost.split(":")[0] ?? "";
}

function fitProfileToDisplay(
  display: DisplayInfo,
  profile: (typeof STREAM_PROFILES)[number],
) {
  return resolveStreamResolution(display, profile);
}

function catalogErrorMessage(error: unknown): string {
  const message = String(error instanceof Error ? error.message : error);
  if (message.includes("SCShareableContent timed out")) {
    return "화면 소스 조회가 지연되고 있습니다. 잠시 후 새로고침을 눌러 주세요.";
  }
  if (message.includes("screen-recording permission")) {
    return "컴퓨터에서 화면 공유 권한이 꺼져 있습니다. Mac 시스템 설정에서 허용해 주세요.";
  }
  return message;
}

async function requestWithReconnect<T>(command: string, args?: unknown): Promise<T> {
  let client = controlClient();
  if (!client) throw new Error("컴퓨터에 연결되어 있지 않습니다");
  try {
    return await client.request<T>(command, args);
  } catch (error) {
    if (!isControlTransportError(error)) throw error;
    client = await reconnectHost();
    return client.request<T>(command, args);
  }
}

function navigateToHostPicker() {
  router.push("/host");
}

function displayKey(display: DisplayInfo) {
  return String(display.index);
}

function DisplayAspectMiniature({ width, height }: { width: number; height: number }) {
  const aspect = width / Math.max(1, height);
  let miniW = 32;
  let miniH = 19;
  if (aspect >= 2.0) {
    miniW = 36;
    miniH = 15;
  } else if (aspect >= 1.7) {
    miniW = 34;
    miniH = 19;
  } else if (aspect >= 1.4) {
    miniW = 30;
    miniH = 20;
  } else if (aspect >= 1.1) {
    miniW = 26;
    miniH = 20;
  } else {
    miniW = 18;
    miniH = 28;
  }

  return (
    <View style={styles.miniatureBox}>
      <View style={[styles.miniatureScreen, { width: miniW, height: miniH }]}>
        <View style={styles.miniatureInner} />
      </View>
      <View style={styles.miniatureStand} />
      <View style={styles.miniatureBase} />
    </View>
  );
}

interface CatalogHeaderProps {
  error: string | null;
  host: string;
  loading: boolean;
  profileId: StreamProfileId;
  refreshing: boolean;
  onRefresh: () => void;
  onSelectProfile: (id: StreamProfileId) => void;
}

function CatalogHeader({
  error,
  host,
  loading,
  profileId,
  refreshing,
  onRefresh,
  onSelectProfile,
}: CatalogHeaderProps) {
  const refreshDisabled = loading || refreshing;
  return (
    <View style={styles.headerContainer}>
      {/* Slim Connected Host Strip */}
      <View style={styles.hostStrip}>
        <View style={styles.hostStripLeft}>
          <View style={styles.dotConnected} />
          <Text style={styles.hostStripText} numberOfLines={1}>
            연결된 컴퓨터: <Text style={styles.hostStripAddr}>{host}</Text>
          </Text>
        </View>
        <Pressable onPress={navigateToHostPicker} style={styles.btnHostChange}>
          <Text style={styles.btnHostChangeText}>컴퓨터 변경</Text>
        </Pressable>
      </View>

      <UsbTransportStatus />

      {/* Error Card */}
      {error ? (
        <View style={styles.errorCard}>
          <Ionicons name="alert-circle-outline" size={16} color="#09090B" />
          <View style={styles.errorBody}>
            <Text style={styles.errorText}>{error}</Text>
            <View style={styles.errorActions}>
              <Pressable
                onPress={onRefresh}
                style={styles.errorRetryBtn}
                disabled={refreshDisabled}
              >
                <Text style={styles.errorRetryText}>다시 조회</Text>
              </Pressable>
              <Pressable onPress={navigateToHostPicker} style={styles.errorHostBtn}>
                <Text style={styles.errorHostText}>컴퓨터 변경</Text>
              </Pressable>
            </View>
          </View>
        </View>
      ) : null}

      {/* Segmented Quality Control */}
      <View style={styles.qualitySegmentWrapper}>
        <View style={styles.qualitySegmentTabs}>
          {STREAM_PROFILES.map((p) => {
            const isSelected = p.id === profileId;
            return (
              <Pressable
                key={p.id}
                onPress={() => onSelectProfile(p.id)}
                style={[styles.qualityTab, isSelected && styles.qualityTabActive]}
              >
                <Text style={[styles.qualityTabLabel, isSelected && styles.qualityTabLabelActive]}>
                  {p.label}
                </Text>
                <Text style={[styles.qualityTabDetail, isSelected && styles.qualityTabDetailActive]}>
                  {p.detail}
                </Text>
              </Pressable>
            );
          })}
        </View>
      </View>

      {/* Section Header */}
      <View style={styles.sectionTitleRow}>
        <Text style={styles.sectionTitleText}>열어 볼 화면 선택</Text>
        <Pressable
          onPress={onRefresh}
          style={[styles.btnRefresh, refreshDisabled && styles.btnDisabled]}
          disabled={refreshDisabled}
        >
          {refreshDisabled ? (
            <ActivityIndicator color="#09090B" size="small" />
          ) : (
            <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
              <Ionicons name="refresh-outline" size={13} color="#09090B" />
              <Text style={styles.btnRefreshText}>새로고침</Text>
            </View>
          )}
        </Pressable>
      </View>
    </View>
  );
}

function UsbTransportStatus() {
  const [state, setState] = useState<UsbAccessoryState>({ attached: false, controlPort: 0 });

  useEffect(() => {
    let active = true;
    void getUsbState().then((next) => {
      if (active) setState(next);
    });
    const subscription = subscribeUsbState(setState);
    return () => {
      active = false;
      subscription.remove();
    };
  }, []);

  return (
    <View style={styles.transportStrip}>
      <View style={[styles.transportDot, state.attached && styles.transportDotUsb]} />
      <Text style={styles.transportText}>
        {state.attached
          ? "USB 액세서리 연결됨 · USB 우선"
          : state.permissionPending
            ? "USB 액세서리 권한 요청 중…"
            : state.accessoryPresent
              ? "USB 액세서리 감지됨 · Android 권한 허용 필요"
              : "USB 없음 · Wi-Fi UDP 우선"}
      </Text>
    </View>
  );
}

interface DisplayListItemProps {
  display: DisplayInfo;
  disabled: boolean;
  isLaunching: boolean;
  profile: StreamProfile;
  onOpen: (display: DisplayInfo) => void;
}

function DisplayListItem({
  display,
  disabled,
  isLaunching,
  profile,
  onOpen,
}: DisplayListItemProps) {
  const size = fitProfileToDisplay(display, profile);
  const handlePress = useCallback(() => onOpen(display), [display, onOpen]);
  return (
    <Pressable style={styles.displayCard} onPress={handlePress} disabled={disabled}>
      <DisplayAspectMiniature width={display.width} height={display.height} />

      <View style={styles.displayMain}>
        <Text style={styles.displayName} numberOfLines={1}>
          {display.name}
        </Text>
        <View style={styles.chipsRow}>
          <View style={styles.chip}>
            <Text style={styles.chipText}>
              {size.width} × {size.height}
            </Text>
          </View>
          <View style={styles.chip}>
            <Text style={styles.chipText}>{profile.fps} FPS</Text>
          </View>
        </View>
      </View>

      <View style={[styles.openBtn, isLaunching && styles.btnDisabled]}>
        {isLaunching ? (
          <ActivityIndicator color="#FFFFFF" size="small" />
        ) : (
          <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
            <Text style={styles.openBtnText}>열기</Text>
            <Ionicons name="arrow-forward" size={12} color="#FFFFFF" />
          </View>
        )}
      </View>
    </Pressable>
  );
}

function EmptyDisplayList({ loading }: { loading: boolean }) {
  return (
    <View style={styles.emptyCard}>
      {loading ? (
        <>
          <ActivityIndicator size="large" color="#09090B" />
          <Text style={styles.loadingText}>사용할 수 있는 화면을 찾는 중입니다…</Text>
        </>
      ) : (
        <Text style={styles.emptyText}>열 수 있는 화면이 없습니다.</Text>
      )}
    </View>
  );
}

function ActiveStreamItem({
  stream,
  onStop,
}: {
  stream: ActiveStream;
  onStop: (stream: ActiveStream) => void;
}) {
  const handleStop = useCallback(() => onStop(stream), [onStop, stream]);
  return (
    <View style={styles.streamCard}>
      <View style={styles.streamInfo}>
        <View style={styles.streamNameRow}>
          <View style={styles.dotActive} />
          <Text style={styles.streamName} numberOfLines={1}>
            #{stream.session} {stream.sourceName}
          </Text>
        </View>
        <Text style={styles.streamPort} numberOfLines={1}>
          {stream.width} × {stream.height} · {stream.fps} FPS
        </Text>
      </View>
      <Pressable style={styles.stopBtn} onPress={handleStop}>
        <Text style={styles.stopBtnText}>정지</Text>
      </Pressable>
    </View>
  );
}

function CatalogFooter({
  streams,
  onStop,
}: {
  streams: ActiveStream[];
  onStop: (stream: ActiveStream) => void;
}) {
  if (streams.length === 0) return null;
  return (
    <View style={styles.activeSection}>
      <View style={styles.activeSectionHeader}>
        <Text style={styles.activeSectionTitle}>현재 공유 중인 화면</Text>
        <View style={styles.activeCountBadge}>
          <Text style={styles.activeCountText}>{streams.length}</Text>
        </View>
      </View>
      {streams.map((stream) => (
        <ActiveStreamItem key={stream.session} stream={stream} onStop={onStop} />
      ))}
    </View>
  );
}

function useStreamController(
  setError: Dispatch<SetStateAction<string | null>>,
  restoreStream: (active: ActiveStream) => Promise<RestoredStream>,
) {
  const [streams, setStreams] = useState<ActiveStream[]>([]);
  const heartbeatInFlight = useRef(new Set<number>());
  const lastRestartAt = useRef(new Map<number, number>());
  const notifiedTerminations = useRef(new Set<number>());
  const transportSwitchInFlight = useRef(new Set<number>());
  const streamsRef = useRef<ActiveStream[]>([]);
  const host = controlHost();
  const queryClient = useQueryClient();
  const statusQuery = useQuery({
    queryKey: ["host-status", host],
    queryFn: () => requestWithReconnect<StatusView>("getStatus"),
    refetchInterval: 2_000,
    staleTime: 1_000,
  });
  const statusView = statusQuery.data;
  useEffect(() => {
    streamsRef.current = streams;
  }, [streams]);
  const { mutate: restartStream } = useMutation({
    mutationFn: async (active: ActiveStream) => {
      const restarted = await restoreStream(active);
      return { active, restarted };
    },
    onSuccess: ({ active, restarted }) => {
      setStreams((previous) =>
        previous.map((item) =>
          item.session === active.session
            ? {
                ...item,
                session: restarted.session,
                captureBackend: restarted.captureBackend,
                viewerIps: restarted.viewerIps,
                mediaTransport: restarted.mediaTransport,
                startedAt: Date.now(),
              }
            : item,
        ),
      );
      setError(null);
      void queryClient.invalidateQueries({ queryKey: ["host-status", host] });
    },
    onError: (error, active) => {
      setError(
        `화면을 다시 연결하지 못했습니다: ${String(error instanceof Error ? error.message : error)}`,
      );
      heartbeatInFlight.current.delete(active.session);
    },
    onSettled: (_data, _error, active) => {
      heartbeatInFlight.current.delete(active.session);
    },
  });

  const { mutate: switchTransport } = useMutation({
    mutationFn: async (active: ActiveStream) => {
      const restarted = await restoreStream(active);
      return { active, restarted };
    },
    onSuccess: ({ active, restarted }) => {
      setStreams((previous) =>
        previous.map((item) =>
          item.session === active.session
            ? {
                ...item,
                session: restarted.session,
                captureBackend: restarted.captureBackend,
                mediaTransport: restarted.mediaTransport,
                viewerIps: restarted.viewerIps,
                startedAt: Date.now(),
              }
            : item,
        ),
      );
      setError(null);
      void queryClient.invalidateQueries({ queryKey: ["host-status", host] });
    },
    onError: (error, active) => {
      setError(
        `전송 경로를 바꾸지 못했습니다: ${String(error instanceof Error ? error.message : error)}`,
      );
      transportSwitchInFlight.current.delete(active.session);
    },
    onSettled: (_data, _error, active) => {
      transportSwitchInFlight.current.delete(active.session);
    },
  });

  const switchTransportRef = useRef(switchTransport);
  useEffect(() => {
    switchTransportRef.current = switchTransport;
  }, [switchTransport]);

  useEffect(() => {
    if (!statusView) return;
    const sessionsById = new Map(
      statusView.sessions.map((session) => [session.session, session]),
    );
    const now = Date.now();
    for (const active of streams) {
      const session = sessionsById.get(active.session);
      const unhealthy =
        !session || ["error", "stopped", "unknown"].includes(session.state);
      const terminalMessage = session?.error ?? "";
      const hostTerminated =
        terminalMessage === "viewer closed stream" ||
        terminalMessage.includes("feedback timeout") ||
        terminalMessage.includes("host operator stopped");
      if (hostTerminated) {
        setStreams((previous) =>
          previous.filter((item) => item.session !== active.session),
        );
        heartbeatInFlight.current.delete(active.session);
        lastRestartAt.current.delete(active.session);
        if (!notifiedTerminations.current.has(active.session)) {
          notifiedTerminations.current.add(active.session);
          Alert.alert(
            "화면 공유가 종료되었어요",
            terminalMessage.includes("feedback timeout")
              ? "컴퓨터와의 연결이 끊어져 화면 공유를 종료했습니다."
              : "컴퓨터에서 이 화면 공유를 종료했습니다.",
          );
        }
        continue;
      }
      if (
        !unhealthy ||
        now - active.startedAt < 5_000 ||
        now - (lastRestartAt.current.get(active.session) ?? 0) < 5_000 ||
        heartbeatInFlight.current.has(active.session)
      ) {
        continue;
      }

      heartbeatInFlight.current.add(active.session);
      lastRestartAt.current.set(active.session, now);
      restartStream(active);
    }
  }, [restartStream, statusView, streams]);

  const [usbState, setUsbState] = useState<UsbAccessoryState>({
    attached: false,
    controlPort: 0,
  });

  useEffect(() => {
    let disposed = false;
    void getUsbState().then((state) => {
      if (!disposed) setUsbState(state);
    });
    const subscription = subscribeUsbState(setUsbState);
    return () => {
      disposed = true;
      subscription.remove();
    };
  }, []);

  useEffect(() => {
    const target = resolveTransport(usbState, "auto");
    const timer = setTimeout(() => {
      for (const active of streamsRef.current) {
        if (
          target === active.mediaTransport ||
          !shouldSwitchTransport(active.mediaTransport, usbState) ||
          transportSwitchInFlight.current.has(active.session)
        ) {
          continue;
        }
        transportSwitchInFlight.current.add(active.session);
        switchTransportRef.current(active);
      }
    }, 1_000);
    return () => clearTimeout(timer);
  }, [usbState]);

  const addStream = useCallback((stream: ActiveStream) => {
    setStreams((previous) => [...previous, stream]);
  }, []);
  const removeStream = useCallback((session: number) => {
    setStreams((previous) => previous.filter((stream) => stream.session !== session));
  }, []);
  return { addStream, removeStream, streams };
}

export default function Catalog() {
  const [error, setError] = useState<string | null>(null);
  const [launchingIndex, setLaunchingIndex] = useState<number | null>(null);
  const [profileId, setProfileId] = useState<StreamProfileId>("latency");
  const host = controlHost();
  const catalogQuery = useQuery({
    queryKey: ["catalog", host],
    queryFn: () => requestWithReconnect<CatalogView>("getCatalog"),
    staleTime: 30_000,
  });
  const { refetch: refetchCatalog } = catalogQuery;

  useEffect(() => {
    if (catalogQuery.error && isUnauthorizedError(catalogQuery.error)) {
      void (async () => {
        await clearToken();
        disconnectHost();
        Alert.alert(
          "연결 승인이 필요해요",
          "컴퓨터의 연결 승인이 만료되었거나 삭제되었습니다. 다시 승인해 주세요.",
        );
        router.replace("/pairing");
      })();
    }
  }, [catalogQuery.error]);

  const displays = (catalogQuery.data?.displays ?? []).filter(
    (display) => !isHubDisplay(display.name),
  );
  const loading = catalogQuery.isLoading;
  const refreshing = catalogQuery.isRefetching;
  const effectiveCaptureBackend = preferredCaptureBackend(
    catalogQuery.data,
    "",
  );
  const mediaHost =
    catalogQuery.data?.mediaHost?.trim() || catalogDisplayHost(host);

  const restoreActiveStream = useCallback(
    async (active: ActiveStream): Promise<RestoredStream> => {
      if (!launcher) {
        throw new Error("화면을 다시 연결할 기능을 시작할 수 없습니다");
      }
      const refreshed = await refetchCatalog();
      const currentCatalog = refreshed.data ?? catalogQuery.data;
      if (!currentCatalog || currentCatalog.captureBackends.length === 0) {
        throw new Error("현재 컴퓨터의 화면 공유 backend를 조회하지 못했습니다");
      }
      const captureBackend = preferredCaptureBackend(
        currentCatalog,
        active.captureBackend,
      );
      const mediaHost = currentCatalog.mediaHost?.trim()
        ? catalogDisplayHost(currentCatalog.mediaHost.trim())
        : catalogDisplayHost(host);
      await requestWithReconnect("stopStream", { session: active.session }).catch(
        () => undefined,
      );
      const control = controlClient() ?? (await reconnectHost());
      const restarted = await startPreparedStream({
        control,
        request: requestWithReconnect,
        launcher,
        host: mediaHost,
        args: {
          sourceIndex: active.sourceIndex,
          viewerPort: active.port,
          width: active.width,
          height: active.height,
          fps: active.fps,
          captureBackend,
          mediaTransport: "auto",
          contentMode: active.contentMode,
        },
      });
      return { ...restarted, captureBackend };
    },
    [catalogQuery.data, host, refetchCatalog],
  );

  const { addStream, removeStream, streams } = useStreamController(
    setError,
    restoreActiveStream,
  );

  const selectedProfile =
    STREAM_PROFILES.find((profile) => profile.id === profileId) ?? STREAM_PROFILES[0];

  const handleRefresh = useCallback(() => {
    setError(null);
    void refetchCatalog();
  }, [refetchCatalog]);

  const handleSelectProfile = useCallback((id: StreamProfileId) => {
    setProfileId(id);
  }, []);

  const openDisplay = useCallback(
    async (d: DisplayInfo) => {
      const client = controlClient();
      if (!client) {
        setError("컴퓨터와의 연결이 끊어졌습니다. 다시 연결해 주세요.");
        return;
      }
      if (!launcher) {
        setError("화면을 여는 기능을 시작할 수 없습니다. 앱을 다시 실행해 주세요.");
        return;
      }
      setLaunchingIndex(d.index);
      setError(null);
      try {
        const port = allocPort();
        const { width, height, fps } = fitProfileToDisplay(d, selectedProfile);
        const startArgs = {
          sourceIndex: d.index,
          viewerPort: port,
          width,
          height,
          fps,
          captureBackend: effectiveCaptureBackend,
          mediaTransport: "auto",
          contentMode: selectedProfile.contentMode,
        };
        const started = await startPreparedStream({
          control: client,
          request: requestWithReconnect,
          launcher,
          host: mediaHost,
          args: startArgs,
        });
        addStream({
          port,
          session: started.session,
          sourceIndex: d.index,
          sourceName: d.name,
          width,
          height,
          fps,
          captureBackend: effectiveCaptureBackend,
          contentMode: selectedProfile.contentMode,
          viewerIps: started.viewerIps,
          mediaTransport: started.mediaTransport,
          startedAt: Date.now(),
        });
      } catch (e) {
        setError(formatErrorMessage(e));
      } finally {
        setLaunchingIndex(null);
      }
    },
    [addStream, effectiveCaptureBackend, mediaHost, selectedProfile],
  );

  const stopStream = useCallback(async (a: ActiveStream) => {
    try {
      await requestWithReconnect("stopStream", { session: a.session });
    } catch {
      // best effort
    }
    removeStream(a.session);
  }, [removeStream]);

  const visibleError = error
    ? error
    : catalogQuery.error
    ? catalogErrorMessage(catalogQuery.error)
    : null;

  const renderDisplay = useCallback(
    ({ item }: ListRenderItemInfo<DisplayInfo>) => (
      <DisplayListItem
        display={item}
        disabled={launchingIndex !== null}
        isLaunching={launchingIndex === item.index}
        profile={selectedProfile}
        onOpen={openDisplay}
      />
    ),
    [launchingIndex, openDisplay, selectedProfile],
  );

  return (
    <SafeAreaView style={styles.safeArea} edges={["left", "right", "bottom"]}>
      <FlatList
        style={styles.root}
        contentContainerStyle={styles.content}
        showsVerticalScrollIndicator={false}
        refreshControl={
          <RefreshControl refreshing={refreshing} onRefresh={handleRefresh} tintColor="#09090B" />
        }
        ListHeaderComponent={
          <CatalogHeader
            error={visibleError}
            host={host || "localhost:7777"}
            loading={loading}
            profileId={profileId}
            refreshing={refreshing}
            onRefresh={handleRefresh}
            onSelectProfile={handleSelectProfile}
          />
        }
        data={displays}
        keyExtractor={displayKey}
        renderItem={renderDisplay}
        ListEmptyComponent={<EmptyDisplayList loading={loading} />}
        ListFooterComponent={
          <CatalogFooter streams={streams} onStop={stopStream} />
        }
      />
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  safeArea: {
    flex: 1,
    backgroundColor: "#FAFAFA",
  },
  root: {
    flex: 1,
    backgroundColor: "#FAFAFA",
  },
  content: {
    paddingHorizontal: 16,
    paddingTop: 12,
    paddingBottom: 32,
    gap: 10,
  },
  headerContainer: {
    gap: 10,
    marginBottom: 4,
  },
  hostStrip: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    borderRadius: 8,
    paddingHorizontal: 12,
    paddingVertical: 9,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  hostStripLeft: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    flex: 1,
    minWidth: 0,
  },
  dotConnected: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#09090B",
    flexShrink: 0,
  },
  hostStripText: {
    color: "#71717A",
    fontSize: 12,
    flex: 1,
  },
  hostStripAddr: {
    color: "#09090B",
    fontWeight: "700",
    fontFamily: "monospace",
    fontVariant: ["tabular-nums"],
  },
  transportStrip: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    borderRadius: 8,
    paddingHorizontal: 12,
    paddingVertical: 8,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  transportDot: {
    width: 7,
    height: 7,
    borderRadius: 4,
    backgroundColor: "#A1A1AA",
  },
  transportDotUsb: {
    backgroundColor: "#16A34A",
  },
  transportText: {
    color: "#52525B",
    fontSize: 12,
  },
  btnHostChange: {
    backgroundColor: "#F4F4F5",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    paddingHorizontal: 8,
    paddingVertical: 4,
    borderRadius: 6,
  },
  btnHostChangeText: {
    color: "#09090B",
    fontSize: 11,
    fontWeight: "600",
  },

  /* Error Card */
  errorCard: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#D4D4D8",
    borderRadius: 8,
    padding: 10,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  errorText: {
    color: "#09090B",
    fontSize: 12,
    lineHeight: 16,
  },
  errorBody: {
    flex: 1,
    gap: 6,
  },
  errorActions: {
    flexDirection: "row",
    gap: 8,
  },
  errorRetryBtn: {
    backgroundColor: "#09090B",
    borderRadius: 6,
    paddingHorizontal: 8,
    paddingVertical: 4,
  },
  errorRetryText: {
    color: "#FFFFFF",
    fontSize: 11,
    fontWeight: "600",
  },
  errorHostBtn: {
    backgroundColor: "#F4F4F5",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    borderRadius: 6,
    paddingHorizontal: 8,
    paddingVertical: 4,
  },
  errorHostText: {
    color: "#52525B",
    fontSize: 11,
    fontWeight: "600",
  },

  /* Segmented Quality */
  qualitySegmentWrapper: {
    backgroundColor: "#FFFFFF",
    borderRadius: 8,
    borderWidth: 1,
    borderColor: "#E4E4E7",
    padding: 3,
  },
  qualitySegmentTabs: {
    flexDirection: "row",
    gap: 3,
  },
  qualityTab: {
    flex: 1,
    paddingVertical: 6,
    paddingHorizontal: 4,
    borderRadius: 6,
    alignItems: "center",
    justifyContent: "center",
    gap: 1,
  },
  qualityTabActive: {
    backgroundColor: "#09090B",
  },
  qualityTabLabel: {
    fontSize: 11,
    fontWeight: "600",
    color: "#71717A",
  },
  qualityTabLabelActive: {
    color: "#FFFFFF",
    fontWeight: "700",
  },
  qualityTabDetail: {
    fontSize: 9,
    fontFamily: "monospace",
    fontVariant: ["tabular-nums"],
    color: "#A1A1AA",
  },
  qualityTabDetailActive: {
    color: "rgba(255, 255, 255, 0.75)",
  },

  /* Section Title */
  sectionTitleRow: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    paddingHorizontal: 2,
    marginTop: 4,
  },
  sectionTitleText: {
    fontSize: 12,
    fontWeight: "700",
    color: "#71717A",
    textTransform: "uppercase",
    letterSpacing: 0.04,
  },
  btnRefresh: {
    paddingHorizontal: 6,
    paddingVertical: 2,
  },
  btnRefreshText: {
    color: "#09090B",
    fontSize: 11,
    fontWeight: "600",
  },
  btnDisabled: {
    opacity: 0.5,
  },

  /* Miniature Display Aspect-Ratio Box */
  miniatureBox: {
    width: 44,
    height: 40,
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
  },
  miniatureScreen: {
    borderWidth: 1.5,
    borderColor: "#09090B",
    borderRadius: 3,
    backgroundColor: "#F4F4F5",
    alignItems: "center",
    justifyContent: "center",
  },
  miniatureInner: {
    width: "70%",
    height: "50%",
    backgroundColor: "#E4E4E7",
    borderRadius: 1,
  },
  miniatureStand: {
    width: 3,
    height: 3,
    backgroundColor: "#09090B",
  },
  miniatureBase: {
    width: 14,
    height: 2,
    backgroundColor: "#09090B",
    borderRadius: 1,
  },

  /* Display Cards */
  displayCard: {
    backgroundColor: "#FFFFFF",
    borderRadius: 10,
    borderWidth: 1,
    borderColor: "#E4E4E7",
    padding: 12,
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
  },
  displayMain: {
    flex: 1,
    minWidth: 0,
    gap: 3,
  },
  displayName: {
    fontSize: 13,
    fontWeight: "700",
    color: "#09090B",
  },
  chipsRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  chip: {
    backgroundColor: "#F4F4F5",
    paddingHorizontal: 6,
    paddingVertical: 1,
    borderRadius: 4,
    borderWidth: 1,
    borderColor: "#E4E4E7",
  },
  chipText: {
    fontSize: 10,
    color: "#52525B",
    fontFamily: "monospace",
    fontVariant: ["tabular-nums"],
  },
  openBtn: {
    backgroundColor: "#09090B",
    paddingHorizontal: 12,
    paddingVertical: 8,
    borderRadius: 6,
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
  },
  openBtnText: {
    color: "#FFFFFF",
    fontSize: 12,
    fontWeight: "600",
  },

  /* Empty State */
  emptyCard: {
    backgroundColor: "#FFFFFF",
    borderRadius: 10,
    borderWidth: 1,
    borderColor: "#E4E4E7",
    padding: 24,
    alignItems: "center",
    gap: 8,
  },
  loadingText: {
    color: "#71717A",
    fontSize: 12,
  },
  emptyText: {
    color: "#71717A",
    fontSize: 12,
  },

  /* Active Streams */
  activeSection: {
    marginTop: 8,
    gap: 8,
  },
  activeSectionHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  activeSectionTitle: {
    fontSize: 12,
    fontWeight: "700",
    color: "#71717A",
    textTransform: "uppercase",
    letterSpacing: 0.04,
  },
  activeCountBadge: {
    backgroundColor: "#09090B",
    paddingHorizontal: 6,
    paddingVertical: 1,
    borderRadius: 10,
  },
  activeCountText: {
    color: "#FFFFFF",
    fontSize: 10,
    fontWeight: "700",
    fontFamily: "monospace",
    fontVariant: ["tabular-nums"],
  },
  streamCard: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#D4D4D8",
    borderRadius: 8,
    padding: 10,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: 8,
  },
  streamInfo: {
    flex: 1,
    minWidth: 0,
    gap: 2,
  },
  streamNameRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  dotActive: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#09090B",
  },
  streamName: {
    color: "#09090B",
    fontSize: 12,
    fontWeight: "700",
  },
  streamPort: {
    color: "#71717A",
    fontSize: 10,
    fontFamily: "monospace",
    fontVariant: ["tabular-nums"],
  },
  stopBtn: {
    backgroundColor: "#F4F4F5",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    paddingHorizontal: 8,
    paddingVertical: 4,
    borderRadius: 6,
    flexShrink: 0,
  },
  stopBtnText: {
    color: "#09090B",
    fontSize: 11,
    fontWeight: "600",
  },
});
