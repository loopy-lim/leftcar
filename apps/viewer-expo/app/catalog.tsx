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
  FlatList,
  type ListRenderItemInfo,
  Pressable,
  RefreshControl,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { NativeModules } from "react-native";
import { router } from "expo-router";
import { allocPort, controlClient, controlHost, reconnectHost } from "../src/session";
import {
  isControlTransportError,
  isUnauthorizedError,
  preferredCaptureBackend,
  type CaptureBackendInfo,
  type CatalogView,
  type DisplayInfo,
  type StatusView,
} from "../src/control";
import { clearToken } from "../src/pairing";

type StreamLauncherNative = {
  openStream(port: number, host: string, width: number, height: number, fps: number): Promise<string>;
};

type NetworkInfoNative = {
  getWifiIpv4(): Promise<string>;
};

const launcher = NativeModules.StreamLauncher as StreamLauncherNative | undefined;
const networkInfo = NativeModules.NsdDiscovery as NetworkInfoNative | undefined;

interface ActiveStream {
  port: number;
  session: number;
  sourceIndex: number;
  sourceName: string;
  width: number;
  height: number;
  fps: number;
  captureBackend: CaptureBackendId;
  startedAt: number;
}

const HIDABLE_DISPLAY_LABELS = ["leftcar hub", "leftcarhub"];

function isHubDisplay(name: string): boolean {
  const normalized = name.trim().toLowerCase();
  return HIDABLE_DISPLAY_LABELS.some((label) => normalized.includes(label));
}

function catalogDisplayHost(catalogHost: string): string {
  return catalogHost.split(":")[0] ?? "";
}

const STREAM_PROFILES = [
  {
    id: "latency",
    label: "낮은 지연",
    detail: "1080p · 60fps",
    maxWidth: 1920,
    maxHeight: 1080,
    fps: 60,
    hint: "마우스·키보드 조작에 가장 적합",
  },
  {
    id: "balanced",
    label: "균형",
    detail: "1440p · 60fps",
    maxWidth: 2560,
    maxHeight: 1440,
    fps: 60,
    hint: "글자 가독성과 지연의 균형",
  },
  {
    id: "clarity",
    label: "고화질",
    detail: "4K · 30fps",
    maxWidth: 3840,
    maxHeight: 2160,
    fps: 30,
    hint: "4K 디스플레이와 안정적인 LAN에서 사용",
  },
] as const;

type StreamProfileId = (typeof STREAM_PROFILES)[number]["id"];
type StreamProfile = (typeof STREAM_PROFILES)[number];

const FALLBACK_CAPTURE_BACKENDS: CaptureBackendInfo[] = [
  { id: "screenCaptureKit", label: "ScreenCaptureKit", hint: "권장 · 최신 macOS 기본 경로" },
];
type CaptureBackendId = string;

function fitProfileToDisplay(
  display: DisplayInfo,
  profile: (typeof STREAM_PROFILES)[number],
) {
  const scale = Math.min(
    1,
    profile.maxWidth / Math.max(1, display.width),
    profile.maxHeight / Math.max(1, display.height),
  );
  return {
    width: Math.max(2, Math.floor((display.width * scale) / 2) * 2),
    height: Math.max(2, Math.floor((display.height * scale) / 2) * 2),
    fps: profile.fps,
  };
}

function catalogErrorMessage(error: unknown): string {
  const message = String(error instanceof Error ? error.message : error);
  if (message.includes("SCShareableContent timed out")) {
    return "macOS 화면 소스 조회가 지연되고 있습니다. 잠시 후 소스 새로고침을 눌러 주세요.";
  }
  if (message.includes("screen-recording permission")) {
    return "Leftcar Host의 화면 기록 권한이 없습니다. Mac 시스템 설정에서 권한을 허용해 주세요.";
  }
  if (message.includes("control request timeout")) {
    return "호스트 응답이 지연되고 있습니다. 연결은 유지되며, 소스 새로고침으로 다시 조회할 수 있습니다.";
  }
  return message;
}

async function requestWithReconnect<T>(command: string, args?: unknown): Promise<T> {
  let client = controlClient();
  if (!client) throw new Error("호스트에 연결되어 있지 않습니다");
  try {
    return await client.request<T>(command, args);
  } catch (error) {
    if (!isControlTransportError(error)) throw error;
    client = await reconnectHost();
    return client.request<T>(command, args);
  }
}

function formatStatus(statusView: StatusView | undefined, streams: ActiveStream[]): string {
  if (!statusView) return "텔레메트리 수집 중…";
  const trackedSessions = new Set(streams.map((stream) => stream.session));
  const currentSessions = statusView.sessions.filter((session) =>
    trackedSessions.has(session.session),
  );
  if (!currentSessions.length) return "활성 스트림 세션 없음";
  return currentSessions
    .map(
      (session) =>
        `#${session.session} ${session.sourceName}: ${session.fps}/${session.fpsTarget || 60}fps · ${session.kbps}kbps ` +
        `· ${session.captureBackend}/${session.mediaTransport.toUpperCase()} · first=${session.firstSendMs}ms ` +
        `· drop=${session.dropped} · cap-p95=${(session.captureToEncodeP95Us / 1000).toFixed(1)}ms ` +
        (session.captureQueueWaitUs !== undefined
          ? `· q=${(session.captureQueueWaitUs / 1000).toFixed(1)}ms `
          : "") +
        (session.encodeOutputUs !== undefined
          ? `· enc=${(session.encodeOutputUs / 1000).toFixed(1)}ms `
          : "") +
        `· send-p95=${(session.sendBlockP95Us / 1000).toFixed(1)}ms` +
        (session.error ? ` · ${session.error}` : ""),
    )
    .join("\n");
}

function navigateToHostPicker() {
  router.push("/host");
}

function displayKey(display: DisplayInfo) {
  return String(display.index);
}

interface ProfileButtonProps {
  profile: StreamProfile;
  selected: boolean;
  onSelect: (id: StreamProfileId) => void;
}

function ProfileButton({ profile, selected, onSelect }: ProfileButtonProps) {
  const handlePress = useCallback(() => onSelect(profile.id), [onSelect, profile.id]);
  return (
    <Pressable
      style={[styles.profileButton, selected && styles.profileButtonSelected]}
      onPress={handlePress}
    >
      <Text style={[styles.profileLabel, selected && styles.profileLabelSelected]}>
        {profile.label}
      </Text>
      <Text style={[styles.profileDetail, selected && styles.profileDetailSelected]}>
        {profile.detail}
      </Text>
    </Pressable>
  );
}

interface CatalogHeaderProps {
  error: string | null;
  host: string;
  loading: boolean;
  profileId: StreamProfileId;
  captureBackend: CaptureBackendId;
  captureBackends: CaptureBackendInfo[];
  refreshing: boolean;
  selectedProfile: StreamProfile;
  onRefresh: () => void;
  onSelectProfile: (id: StreamProfileId) => void;
  onSelectCaptureBackend: (id: CaptureBackendId) => void;
}

function CatalogHeader({
  error,
  host,
  loading,
  profileId,
  captureBackend,
  captureBackends,
  refreshing,
  selectedProfile,
  onRefresh,
  onSelectProfile,
  onSelectCaptureBackend,
}: CatalogHeaderProps) {
  const refreshDisabled = loading || refreshing;
  return (
    <>
      <View style={styles.hostHeader}>
        <View style={styles.hostHeaderLeft}>
          <View style={styles.statusDot} />
          <View>
            <Text style={styles.hostTitle}>호스트 연결됨</Text>
            <Text style={styles.hostChipText}>{host}</Text>
          </View>
        </View>
        <Pressable onPress={navigateToHostPicker} style={styles.disconnectBtn}>
          <Text style={styles.disconnectBtnText}>호스트 변경</Text>
        </Pressable>
      </View>

      {error ? (
        <View style={styles.errorCard}>
          <Text style={styles.errorIcon}>⚠️</Text>
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
                <Text style={styles.errorHostText}>호스트 변경</Text>
              </Pressable>
            </View>
          </View>
        </View>
      ) : null}

      <View style={styles.sourceSectionHeader}>
        <View style={styles.sectionHeaderText}>
          <Text style={styles.sectionTitle}>사용 가능한 디스플레이</Text>
          <Text style={styles.sectionSub}>XR 윈도우로 독립 스트리밍할 디스플레이를 선택하세요</Text>
        </View>
        <Pressable
          onPress={onRefresh}
          style={[styles.sourceRefreshBtn, refreshDisabled && styles.btnDisabled]}
          disabled={refreshDisabled}
        >
          {refreshDisabled ? (
            <ActivityIndicator color="#c7d2fe" size="small" />
          ) : (
            <Text style={styles.sourceRefreshText}>소스 새로고침</Text>
          )}
        </Pressable>
      </View>

      <View style={styles.qualityCard}>
        <View style={styles.qualityHeader}>
          <View>
            <Text style={styles.qualityTitle}>스트림 품질</Text>
            <Text style={styles.qualitySub}>{selectedProfile.hint}</Text>
          </View>
          <Text style={styles.qualitySelected}>{selectedProfile.detail}</Text>
        </View>
        <View style={styles.profileRow}>
          {STREAM_PROFILES.map((profile) => (
            <ProfileButton
              key={profile.id}
              profile={profile}
              selected={profile.id === profileId}
              onSelect={onSelectProfile}
            />
          ))}
        </View>
        <View style={styles.profileRow}>
          {captureBackends.map((backend) => (
            <Pressable
              key={backend.id}
              style={[
                styles.profileButton,
                backend.id === captureBackend && styles.profileButtonSelected,
              ]}
              onPress={() => onSelectCaptureBackend(backend.id)}
            >
              <Text
                style={[
                  styles.profileLabel,
                  backend.id === captureBackend && styles.profileLabelSelected,
                ]}
              >
                {backend.label}
              </Text>
              <Text
                style={[
                  styles.profileDetail,
                  backend.id === captureBackend && styles.profileDetailSelected,
                ]}
              >
                {backend.hint}
              </Text>
            </Pressable>
          ))}
        </View>
      </View>
    </>
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
      <View style={styles.displayIconBox}>
        <Text style={styles.displayIcon}>🖥️</Text>
      </View>
      <View style={styles.displayInfo}>
        <Text style={styles.displayName}>{display.name}</Text>
        <View style={styles.chipsRow}>
          <View style={styles.chip}>
            <Text style={styles.chipText}>
              {size.width} × {size.height}
            </Text>
          </View>
          <View style={styles.chip}>
            <Text style={styles.chipTextSuccess}>{profile.fps} fps</Text>
          </View>
        </View>
      </View>
      <View style={[styles.openBtn, isLaunching && styles.btnDisabled]}>
        {isLaunching ? (
          <ActivityIndicator color="#ffffff" size="small" />
        ) : (
          <Text style={styles.openBtnText}>XR 창 열기 →</Text>
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
          <ActivityIndicator size="large" color="#6366f1" />
          <Text style={styles.loadingText}>호스트 화면 소스를 조회하는 중…</Text>
        </>
      ) : (
        <Text style={styles.emptyText}>사용 가능한 디스플레이가 없습니다.</Text>
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
          <View style={styles.liveDot} />
          <Text style={styles.streamName}>
            #{stream.session} {stream.sourceName}
          </Text>
        </View>
        <Text style={styles.streamPort}>
          {stream.width} × {stream.height} · {stream.fps}fps · Port :{stream.port}
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
  status,
  onStop,
}: {
  streams: ActiveStream[];
  status: string;
  onStop: (stream: ActiveStream) => void;
}) {
  return (
    <>
      {streams.length > 0 ? (
        <View style={styles.activeSection}>
          <View style={styles.sectionHeader}>
            <View style={styles.activeTitleRow}>
              <Text style={styles.sectionTitle}>활성 XR 스트림</Text>
              <View style={styles.activeCountBadge}>
                <Text style={styles.activeCountText}>{streams.length}</Text>
              </View>
            </View>
          </View>
          {streams.map((stream) => (
            <ActiveStreamItem key={stream.session} stream={stream} onStop={onStop} />
          ))}
        </View>
      ) : null}

      <View style={styles.telemetryCard}>
        <Text style={styles.telemetryTitle}>📊 실시간 파이프라인 텔레메트리</Text>
        <Text style={styles.telemetryText}>{status || "텔레메트리 수집 중…"}</Text>
      </View>
    </>
  );
}

interface StreamController {
  addStream: (stream: ActiveStream) => void;
  removeStream: (session: number) => void;
  status: string;
  streams: ActiveStream[];
}

function useStreamController(
  setError: Dispatch<SetStateAction<string | null>>,
): StreamController {
  const [streams, setStreams] = useState<ActiveStream[]>([]);
  const heartbeatInFlight = useRef(new Set<number>());
  const lastRestartAt = useRef(new Map<number, number>());
  const host = controlHost();
  const queryClient = useQueryClient();
  const statusQuery = useQuery({
    queryKey: ["host-status", host],
    queryFn: () => requestWithReconnect<StatusView>("getStatus"),
    refetchInterval: 2_000,
    staleTime: 1_000,
  });
  const statusView = statusQuery.data;
  const { mutate: restartStream } = useMutation({
    mutationFn: async (active: ActiveStream) => {
      await requestWithReconnect("stopStream", { session: active.session }).catch(
        () => undefined,
      );
      const restarted = await requestWithReconnect<{ session: number }>("startStream", {
        sourceIndex: active.sourceIndex,
        viewerPort: active.port,
        width: active.width,
        height: active.height,
        fps: active.fps,
        captureBackend: active.captureBackend,
      });
      return { active, restarted };
    },
    onSuccess: ({ active, restarted }) => {
      setStreams((previous) =>
        previous.map((item) =>
          item.session === active.session
            ? { ...item, session: restarted.session, startedAt: Date.now() }
            : item,
        ),
      );
      setError(null);
      void queryClient.invalidateQueries({ queryKey: ["host-status", host] });
    },
    onError: (error, active) => {
      setError(
        `스트림 heartbeat 재연결 실패: ${String(error instanceof Error ? error.message : error)}`,
      );
      heartbeatInFlight.current.delete(active.session);
    },
    onSettled: (_data, _error, active) => {
      heartbeatInFlight.current.delete(active.session);
    },
  });

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
      if (session?.error === "viewer closed stream") {
        setStreams((previous) =>
          previous.filter((item) => item.session !== active.session),
        );
        heartbeatInFlight.current.delete(active.session);
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

  const addStream = useCallback((stream: ActiveStream) => {
    setStreams((previous) => [...previous, stream]);
  }, []);
  const removeStream = useCallback((session: number) => {
    setStreams((previous) => previous.filter((stream) => stream.session !== session));
  }, []);
  const status = statusQuery.error
    ? "상태 조회 실패 · 자동 재연결 대기 중"
    : formatStatus(statusView, streams);
  return { addStream, removeStream, status, streams };
}

/**
 * Source catalog: list displays from the host, open each in its own OS
 * window after the control socket confirms its startStream write. Waiting for
 * that write callback prevents XR Activity pause from stranding the request,
 * while the host's bounded connect retry covers native listener startup.
 */
export default function Catalog() {
  const [error, setError] = useState<string | null>(null);
  const [launchingIndex, setLaunchingIndex] = useState<number | null>(null);
  const [profileId, setProfileId] = useState<StreamProfileId>("latency");
  const [captureBackend, setCaptureBackend] =
    useState<CaptureBackendId>("screenCaptureKit");
  const host = controlHost();
  const catalogQuery = useQuery({
    queryKey: ["catalog", host],
    queryFn: () => requestWithReconnect<CatalogView>("getCatalog"),
    staleTime: 30_000,
  });
  const { refetch: refetchCatalog } = catalogQuery;
  const displays = (catalogQuery.data?.displays ?? []).filter(
    (display) => !isHubDisplay(display.name),
  );
  const loading = catalogQuery.isLoading;
  const refreshing = catalogQuery.isRefetching;
  const captureBackends = catalogQuery.data?.captureBackends?.length
    ? catalogQuery.data.captureBackends
    : FALLBACK_CAPTURE_BACKENDS;
  useEffect(() => {
    const preferred = preferredCaptureBackend(catalogQuery.data, captureBackend);
    if (preferred !== captureBackend) {
      setCaptureBackend(preferred);
    }
  }, [captureBackend, catalogQuery.data]);
  // Revoked/expired token: drop it and start the pairing flow again.
  useEffect(() => {
    if (!catalogQuery.error || !isUnauthorizedError(catalogQuery.error)) return;
    void clearToken();
    setError("페어링이 만료되었습니다. 호스트와 다시 페어링해 주세요.");
    router.replace("/pairing");
  }, [catalogQuery.error]);
  const visibleError = error ?? (catalogQuery.error ? catalogErrorMessage(catalogQuery.error) : null);
  const selectedProfile =
    STREAM_PROFILES.find((profile) => profile.id === profileId) ?? STREAM_PROFILES[0];
  const { addStream, removeStream, status, streams } = useStreamController(setError);

  const openDisplay = useCallback(async (d: DisplayInfo) => {
    let client = controlClient();
    if (!client || !launcher) {
      setError(launcher ? "제어 연결 없음" : "네이티브 모듈 없음 (Galaxy XR dev build 필요)");
      return;
    }
    setLaunchingIndex(d.index);
    setError(null);
    try {
      // Verify the existing control socket before opening StreamActivity. If
      // the host was restarted, reconnect while the catalog JS Activity is
      // still active so we never leave an orphan black native window behind.
      try {
        await client.request<StatusView>("getStatus");
      } catch (connectionError) {
        if (!isControlTransportError(connectionError)) throw connectionError;
        client = await reconnectHost();
      }
      const port = allocPort();
      const selectedSize = fitProfileToDisplay(d, selectedProfile);
      const { width, height, fps } = selectedSize;
      const viewerIps = await networkInfo?.getWifiIpv4().then((address) => [address]).catch(() => []);

      const startArgs = {
        sourceIndex: d.index,
        viewerPort: port,
        width,
        height,
        fps,
        captureBackend,
        viewerIps: viewerIps ?? [],
      };

      let confirmWritten!: () => void;
      let rejectWritten!: (error: unknown) => void;
      const written = new Promise<void>((resolve, reject) => {
        confirmWritten = resolve;
        rejectWritten = reject;
      });
      const startPromise = client.request<{ session: number }>(
        "startStream",
        startArgs,
        confirmWritten,
      );
      void startPromise.catch(rejectWritten);

      // The react-native-tcp-socket callback confirms the native write has
      // completed. Only then may StreamActivity pause the catalog Activity.
      await written;
      await launcher.openStream(port, catalogDisplayHost(host), width, height, fps);
      const { session } = await startPromise;
      addStream({
        port,
        session,
        sourceIndex: d.index,
        sourceName: d.name,
        width,
        height,
        fps,
        captureBackend,
        startedAt: Date.now(),
      });
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    } finally {
      setLaunchingIndex(null);
    }
  }, [addStream, captureBackend, host, selectedProfile]);

  const stopStream = useCallback(async (a: ActiveStream) => {
    const client = controlClient();
    if (!client) return;
    try {
      await client.request("stopStream", { session: a.session });
      removeStream(a.session);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  }, [removeStream]);

  const handleRefresh = useCallback(() => {
    setError(null);
    void refetchCatalog();
  }, [refetchCatalog]);
  const handleSelectProfile = useCallback((id: StreamProfileId) => {
    setProfileId(id);
  }, []);
  const handleSelectCaptureBackend = useCallback((id: CaptureBackendId) => {
    setCaptureBackend(id);
  }, []);
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
    <View style={styles.root}>
      <FlatList
        contentContainerStyle={styles.content}
        refreshControl={
          <RefreshControl
            refreshing={refreshing}
            onRefresh={handleRefresh}
            tintColor="#a5b4fc"
          />
        }
        ListHeaderComponent={
          <CatalogHeader
            error={visibleError}
            host={host || "localhost:7777"}
            loading={loading}
            profileId={profileId}
            captureBackend={captureBackend}
            captureBackends={captureBackends}
            refreshing={refreshing}
            selectedProfile={selectedProfile}
            onRefresh={handleRefresh}
            onSelectProfile={handleSelectProfile}
            onSelectCaptureBackend={handleSelectCaptureBackend}
          />
        }
        data={displays}
        keyExtractor={displayKey}
        renderItem={renderDisplay}
        ListEmptyComponent={<EmptyDisplayList loading={loading} />}
        ListFooterComponent={
          <CatalogFooter streams={streams} status={status} onStop={stopStream} />
        }
      />
    </View>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
    backgroundColor: "#080b11",
  },
  loadingText: {
    color: "#94a3b8",
    fontSize: 13,
  },
  content: {
    padding: 20,
    gap: 14,
    paddingBottom: 40,
  },
  hostHeader: {
    backgroundColor: "#0f172a",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.08)",
    borderRadius: 12,
    padding: 14,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  hostHeaderLeft: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
  },
  statusDot: {
    width: 8,
    height: 8,
    borderRadius: 4,
    backgroundColor: "#10b981",
  },
  hostTitle: {
    color: "#f8fafc",
    fontSize: 13,
    fontWeight: "700",
  },
  hostChipText: {
    color: "#38bdf8",
    fontSize: 11,
    fontFamily: "monospace",
    marginTop: 2,
  },
  disconnectBtn: {
    backgroundColor: "#1e293b",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.08)",
    paddingHorizontal: 10,
    paddingVertical: 6,
    borderRadius: 6,
  },
  disconnectBtnText: {
    color: "#94a3b8",
    fontSize: 11,
    fontWeight: "600",
  },
  errorCard: {
    backgroundColor: "rgba(239, 68, 68, 0.15)",
    borderWidth: 1,
    borderColor: "rgba(239, 68, 68, 0.35)",
    borderRadius: 12,
    padding: 12,
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    marginTop: 8,
  },
  errorIcon: {
    fontSize: 16,
  },
  errorText: {
    color: "#fca5a5",
    fontSize: 13,
    lineHeight: 18,
  },
  errorBody: {
    flex: 1,
    gap: 10,
  },
  errorActions: {
    flexDirection: "row",
    gap: 8,
  },
  errorRetryBtn: {
    backgroundColor: "#dc2626",
    borderRadius: 6,
    paddingHorizontal: 10,
    paddingVertical: 7,
  },
  errorRetryText: {
    color: "#ffffff",
    fontSize: 11,
    fontWeight: "700",
  },
  errorHostBtn: {
    backgroundColor: "rgba(255, 255, 255, 0.06)",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.12)",
    borderRadius: 6,
    paddingHorizontal: 10,
    paddingVertical: 7,
  },
  errorHostText: {
    color: "#cbd5e1",
    fontSize: 11,
    fontWeight: "700",
  },
  sectionHeader: {
    gap: 4,
    marginTop: 10,
    marginBottom: 4,
  },
  sectionTitle: {
    color: "#f8fafc",
    fontSize: 16,
    fontWeight: "700",
    letterSpacing: -0.3,
  },
  sectionSub: {
    color: "#94a3b8",
    fontSize: 12,
  },
  sourceSectionHeader: {
    marginTop: 10,
    marginBottom: 4,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: 12,
  },
  sectionHeaderText: {
    flex: 1,
    gap: 4,
  },
  sourceRefreshBtn: {
    minWidth: 96,
    minHeight: 34,
    backgroundColor: "rgba(99, 102, 241, 0.18)",
    borderWidth: 1,
    borderColor: "rgba(129, 140, 248, 0.55)",
    borderRadius: 8,
    paddingHorizontal: 11,
    alignItems: "center",
    justifyContent: "center",
  },
  sourceRefreshText: {
    color: "#c7d2fe",
    fontSize: 11,
    fontWeight: "700",
  },
  qualityCard: {
    backgroundColor: "#0f172a",
    borderWidth: 1,
    borderColor: "rgba(99, 102, 241, 0.35)",
    borderRadius: 12,
    padding: 14,
    gap: 12,
  },
  qualityHeader: {
    flexDirection: "row",
    alignItems: "flex-start",
    justifyContent: "space-between",
    gap: 8,
  },
  qualityTitle: {
    color: "#f8fafc",
    fontSize: 13,
    fontWeight: "700",
  },
  qualitySub: {
    color: "#94a3b8",
    fontSize: 11,
    marginTop: 3,
  },
  qualitySelected: {
    color: "#a5b4fc",
    fontSize: 11,
    fontFamily: "monospace",
    fontWeight: "700",
  },
  profileRow: {
    flexDirection: "row",
    gap: 8,
  },
  profileButton: {
    flex: 1,
    minHeight: 54,
    backgroundColor: "#161f33",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.08)",
    borderRadius: 8,
    paddingHorizontal: 8,
    paddingVertical: 8,
    justifyContent: "center",
  },
  profileButtonSelected: {
    backgroundColor: "rgba(99, 102, 241, 0.22)",
    borderColor: "#818cf8",
  },
  profileLabel: {
    color: "#cbd5e1",
    fontSize: 11,
    fontWeight: "700",
    textAlign: "center",
  },
  profileLabelSelected: {
    color: "#ffffff",
  },
  profileDetail: {
    color: "#64748b",
    fontSize: 9,
    fontFamily: "monospace",
    textAlign: "center",
    marginTop: 3,
  },
  profileDetailSelected: {
    color: "#c7d2fe",
  },
  displayCard: {
    backgroundColor: "#0f172a",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.08)",
    borderRadius: 12,
    padding: 14,
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    marginBottom: 10,
  },
  displayIconBox: {
    width: 40,
    height: 40,
    borderRadius: 10,
    backgroundColor: "#161f33",
    alignItems: "center",
    justifyContent: "center",
  },
  displayIcon: {
    fontSize: 20,
  },
  displayInfo: {
    flex: 1,
    gap: 4,
  },
  displayName: {
    color: "#f8fafc",
    fontSize: 14,
    fontWeight: "600",
  },
  chipsRow: {
    flexDirection: "row",
    gap: 6,
  },
  chip: {
    backgroundColor: "#161f33",
    paddingHorizontal: 6,
    paddingVertical: 2,
    borderRadius: 4,
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.06)",
  },
  chipText: {
    color: "#94a3b8",
    fontSize: 10,
    fontFamily: "monospace",
  },
  chipTextSuccess: {
    color: "#34d399",
    fontSize: 10,
    fontFamily: "monospace",
  },
  openBtn: {
    backgroundColor: "#6366f1",
    paddingHorizontal: 12,
    paddingVertical: 8,
    borderRadius: 8,
    minWidth: 90,
    alignItems: "center",
    justifyContent: "center",
  },
  btnDisabled: {
    opacity: 0.6,
  },
  openBtnText: {
    color: "#ffffff",
    fontSize: 12,
    fontWeight: "700",
  },
  emptyCard: {
    backgroundColor: "#0f172a",
    padding: 24,
    borderRadius: 12,
    alignItems: "center",
    gap: 12,
  },
  emptyText: {
    color: "#64748b",
    fontSize: 13,
  },
  activeSection: {
    marginTop: 12,
    gap: 8,
  },
  activeTitleRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  activeCountBadge: {
    backgroundColor: "#10b981",
    paddingHorizontal: 6,
    paddingVertical: 1,
    borderRadius: 10,
  },
  activeCountText: {
    color: "#ffffff",
    fontSize: 11,
    fontWeight: "700",
    fontFamily: "monospace",
  },
  streamCard: {
    backgroundColor: "#0f172a",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.08)",
    borderRadius: 12,
    padding: 12,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  streamInfo: {
    gap: 3,
  },
  streamNameRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  liveDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#10b981",
  },
  streamName: {
    color: "#f8fafc",
    fontSize: 13,
    fontWeight: "600",
  },
  streamPort: {
    color: "#38bdf8",
    fontSize: 11,
    fontFamily: "monospace",
  },
  stopBtn: {
    backgroundColor: "rgba(239, 68, 68, 0.15)",
    borderWidth: 1,
    borderColor: "rgba(239, 68, 68, 0.35)",
    paddingHorizontal: 12,
    paddingVertical: 6,
    borderRadius: 6,
  },
  stopBtnText: {
    color: "#fca5a5",
    fontSize: 12,
    fontWeight: "700",
  },
  telemetryCard: {
    backgroundColor: "rgba(15, 23, 42, 0.5)",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.06)",
    borderRadius: 12,
    padding: 14,
    gap: 8,
    marginTop: 14,
  },
  telemetryTitle: {
    color: "#f8fafc",
    fontSize: 12,
    fontWeight: "600",
  },
  telemetryText: {
    color: "#94a3b8",
    fontSize: 11,
    fontFamily: "monospace",
    lineHeight: 18,
  },
});
