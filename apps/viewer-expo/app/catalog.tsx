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
import { SafeAreaView } from "react-native-safe-area-context";
import { router } from "expo-router";
import { allocPort, controlClient, controlHost, reconnectHost } from "../src/session";
import {
  isControlTransportError,
  preferredCaptureBackend,
  type CaptureBackendInfo,
  type CatalogView,
  type DisplayInfo,
  type StatusView,
} from "../src/control";

type StreamLauncherNative = {
  openStream(port: number, host: string, width: number, height: number, fps: number): Promise<string>;
};

const launcher = NativeModules.StreamLauncher as StreamLauncherNative | undefined;

interface ActiveStream {
  port: number;
  session: number;
  sourceIndex: number;
  sourceName: string;
  width: number;
  height: number;
  fps: number;
  captureBackend: string;
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
    detail: "1080p 60fps",
    maxWidth: 1920,
    maxHeight: 1080,
    fps: 60,
    hint: "마우스·키보드 조작에 최적",
  },
  {
    id: "balanced",
    label: "균형",
    detail: "1440p 60fps",
    maxWidth: 2560,
    maxHeight: 1440,
    fps: 60,
    hint: "글자 가독성과 반응속도 균형",
  },
  {
    id: "clarity",
    label: "고화질",
    detail: "4K 60fps",
    maxWidth: 3840,
    maxHeight: 2160,
    fps: 60,
    hint: "Wi-Fi 7 또는 100Mbps급 LAN",
  },
] as const;

type StreamProfileId = (typeof STREAM_PROFILES)[number]["id"];
type StreamProfile = (typeof STREAM_PROFILES)[number];

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
    return "macOS 화면 소스 조회가 지연되고 있습니다. 잠시 후 새로고침을 눌러 주세요.";
  }
  if (message.includes("screen-recording permission")) {
    return "Leftcar Host의 화면 기록 권한이 없습니다. Mac 시스템 설정에서 허용해 주세요.";
  }
  if (message.includes("control request timeout")) {
    return "호스트 응답이 지연되고 있습니다. 새로고침으로 다시 조회할 수 있습니다.";
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
      <Text style={[styles.profileLabel, selected && styles.profileLabelSelected]} numberOfLines={1}>
        {profile.label}
      </Text>
      <Text style={[styles.profileDetail, selected && styles.profileDetailSelected]} numberOfLines={1}>
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
  refreshing: boolean;
  selectedProfile: StreamProfile;
  onRefresh: () => void;
  onSelectProfile: (id: StreamProfileId) => void;
}

function CatalogHeader({
  error,
  host,
  loading,
  profileId,
  refreshing,
  selectedProfile,
  onRefresh,
  onSelectProfile,
}: CatalogHeaderProps) {
  const refreshDisabled = loading || refreshing;
  return (
    <View style={styles.headerContainer}>
      <View style={styles.hostHeader}>
        <View style={styles.hostHeaderLeft}>
          <View style={styles.statusDot} />
          <View style={styles.hostHeaderTextGroup}>
            <Text style={styles.hostTitle}>호스트 연결됨</Text>
            <Text style={styles.hostChipText} numberOfLines={1}>{host}</Text>
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

      <View style={styles.qualityCard}>
        <View style={styles.qualityHeader}>
          <Text style={styles.qualityTitle}>스트림 품질</Text>
          <Text style={styles.qualitySub}>{selectedProfile.hint}</Text>
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
      </View>

      <View style={styles.sourceSectionHeader}>
        <Text style={styles.sectionTitle}>사용 가능한 화면 목록</Text>
        <Pressable
          onPress={onRefresh}
          style={[styles.sourceRefreshBtn, refreshDisabled && styles.btnDisabled]}
          disabled={refreshDisabled}
        >
          {refreshDisabled ? (
            <ActivityIndicator color="#2563EB" size="small" />
          ) : (
            <Text style={styles.sourceRefreshText}>새로고침</Text>
          )}
        </Pressable>
      </View>
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
      <View style={styles.displayIconBox}>
        <Text style={styles.displayIcon}>🖥️</Text>
      </View>
      <View style={styles.displayInfo}>
        <View style={styles.displayNameRow}>
          <Text style={styles.displayName} numberOfLines={1}>
            {display.name}
          </Text>
          {display.index === 0 && (
            <View style={styles.primaryBadge}>
              <Text style={styles.primaryBadgeText}>메인</Text>
            </View>
          )}
        </View>
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
          <ActivityIndicator color="#FFFFFF" size="small" />
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
          <ActivityIndicator size="large" color="#2563EB" />
          <Text style={styles.loadingText}>화면 소스를 조회하는 중…</Text>
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
          <Text style={styles.streamName} numberOfLines={1}>
            #{stream.session} {stream.sourceName}
          </Text>
        </View>
        <Text style={styles.streamPort} numberOfLines={1}>
          {stream.width} × {stream.height} · {stream.fps}fps
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
        <Text style={styles.activeSectionTitle}>활성 XR 스트림</Text>
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
) {
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
        `스트림 재연결 실패: ${String(error instanceof Error ? error.message : error)}`,
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
  const displays = (catalogQuery.data?.displays ?? []).filter(
    (display) => !isHubDisplay(display.name),
  );
  const loading = catalogQuery.isLoading;
  const refreshing = catalogQuery.isRefetching;
  const effectiveCaptureBackend = preferredCaptureBackend(
    catalogQuery.data,
    "",
  );

  const { addStream, removeStream, streams } = useStreamController(setError);

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
        setError("호스트 연결이 끊어졌습니다. 다시 연결해 주세요.");
        return;
      }
      if (!launcher) {
        setError("네이티브 스트림 런처를 사용할 수 없습니다.");
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
          captureBackend: effectiveCaptureBackend,
          startedAt: Date.now(),
        });
      } catch (e) {
        setError(String(e instanceof Error ? e.message : e));
      } finally {
        setLaunchingIndex(null);
      }
    },
    [addStream, effectiveCaptureBackend, host, selectedProfile],
  );

  const stopStream = useCallback(async (a: ActiveStream) => {
    const client = controlClient();
    if (!client) return;
    try {
      await client.request("stopStream", { session: a.session });
    } catch {
      // best-effort stop
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
        refreshControl={
          <RefreshControl refreshing={refreshing} onRefresh={handleRefresh} tintColor="#2563EB" />
        }
        ListHeaderComponent={
          <CatalogHeader
            error={visibleError}
            host={host || "localhost:7777"}
            loading={loading}
            profileId={profileId}
            refreshing={refreshing}
            selectedProfile={selectedProfile}
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
    backgroundColor: "#F8FAFC",
  },
  root: {
    flex: 1,
    backgroundColor: "#F8FAFC",
  },
  content: {
    padding: 16,
    gap: 10,
    paddingBottom: 36,
  },
  headerContainer: {
    gap: 12,
    marginBottom: 4,
  },
  hostHeader: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    borderRadius: 12,
    padding: 12,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    shadowColor: "#000000",
    shadowOffset: { width: 0, height: 1 },
    shadowOpacity: 0.04,
    shadowRadius: 2,
    elevation: 1,
  },
  hostHeaderLeft: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    flex: 1,
    minWidth: 0,
    marginRight: 8,
  },
  statusDot: {
    width: 7,
    height: 7,
    borderRadius: 3.5,
    backgroundColor: "#059669",
    flexShrink: 0,
  },
  hostHeaderTextGroup: {
    flex: 1,
    minWidth: 0,
  },
  hostTitle: {
    color: "#0F172A",
    fontSize: 13,
    fontWeight: "600",
  },
  hostChipText: {
    color: "#64748B",
    fontSize: 11,
    fontFamily: "monospace",
  },
  disconnectBtn: {
    backgroundColor: "#F1F5F9",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    paddingHorizontal: 8,
    paddingVertical: 5,
    borderRadius: 6,
    flexShrink: 0,
  },
  disconnectBtnText: {
    color: "#475569",
    fontSize: 11,
    fontWeight: "600",
  },
  errorCard: {
    backgroundColor: "#FEF2F2",
    borderWidth: 1,
    borderColor: "#FECACA",
    borderRadius: 10,
    padding: 10,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  errorIcon: {
    fontSize: 14,
  },
  errorText: {
    color: "#DC2626",
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
    backgroundColor: "#DC2626",
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
    backgroundColor: "#F1F5F9",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    borderRadius: 6,
    paddingHorizontal: 8,
    paddingVertical: 4,
  },
  errorHostText: {
    color: "#475569",
    fontSize: 11,
    fontWeight: "600",
  },
  qualityCard: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    borderRadius: 12,
    padding: 12,
    gap: 8,
    shadowColor: "#000000",
    shadowOffset: { width: 0, height: 1 },
    shadowOpacity: 0.04,
    shadowRadius: 2,
    elevation: 1,
  },
  qualityHeader: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  qualityTitle: {
    color: "#0F172A",
    fontSize: 13,
    fontWeight: "600",
  },
  qualitySub: {
    color: "#64748B",
    fontSize: 11,
  },
  profileRow: {
    flexDirection: "row",
    gap: 6,
  },
  profileButton: {
    flex: 1,
    minHeight: 46,
    backgroundColor: "#F8FAFC",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    borderRadius: 6,
    paddingHorizontal: 4,
    paddingVertical: 6,
    justifyContent: "center",
    alignItems: "center",
  },
  profileButtonSelected: {
    backgroundColor: "#EFF6FF",
    borderColor: "#3B82F6",
  },
  profileLabel: {
    color: "#475569",
    fontSize: 11,
    fontWeight: "600",
  },
  profileLabelSelected: {
    color: "#1D4ED8",
    fontWeight: "700",
  },
  profileDetail: {
    color: "#94A3B8",
    fontSize: 9,
    fontFamily: "monospace",
    marginTop: 1,
  },
  profileDetailSelected: {
    color: "#2563EB",
  },
  sourceSectionHeader: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    marginTop: 4,
  },
  sectionTitle: {
    color: "#0F172A",
    fontSize: 13,
    fontWeight: "600",
  },
  sourceRefreshBtn: {
    paddingHorizontal: 8,
    paddingVertical: 4,
    borderRadius: 6,
  },
  sourceRefreshText: {
    color: "#2563EB",
    fontSize: 11,
    fontWeight: "600",
  },
  displayCard: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    borderRadius: 10,
    padding: 12,
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    shadowColor: "#000000",
    shadowOffset: { width: 0, height: 1 },
    shadowOpacity: 0.04,
    shadowRadius: 2,
    elevation: 1,
  },
  displayIconBox: {
    width: 36,
    height: 36,
    borderRadius: 8,
    backgroundColor: "#EFF6FF",
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
  },
  displayIcon: {
    fontSize: 18,
  },
  displayInfo: {
    flex: 1,
    minWidth: 0,
    gap: 3,
  },
  displayNameRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  displayName: {
    color: "#0F172A",
    fontSize: 13,
    fontWeight: "600",
    flex: 1,
  },
  primaryBadge: {
    backgroundColor: "#EFF6FF",
    borderWidth: 1,
    borderColor: "#BFDBFE",
    paddingHorizontal: 4,
    paddingVertical: 1,
    borderRadius: 4,
  },
  primaryBadgeText: {
    color: "#1D4ED8",
    fontSize: 9,
    fontWeight: "700",
  },
  chipsRow: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 6,
  },
  chip: {
    backgroundColor: "#F1F5F9",
    paddingHorizontal: 6,
    paddingVertical: 2,
    borderRadius: 4,
    borderWidth: 1,
    borderColor: "#E2E8F0",
  },
  chipText: {
    color: "#64748B",
    fontSize: 10,
    fontFamily: "monospace",
  },
  chipTextSuccess: {
    color: "#059669",
    fontSize: 10,
    fontFamily: "monospace",
  },
  openBtn: {
    backgroundColor: "#2563EB",
    paddingHorizontal: 10,
    paddingVertical: 7,
    borderRadius: 6,
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
  },
  btnDisabled: {
    opacity: 0.6,
  },
  openBtnText: {
    color: "#FFFFFF",
    fontSize: 12,
    fontWeight: "600",
  },
  emptyCard: {
    backgroundColor: "#FFFFFF",
    padding: 24,
    borderRadius: 10,
    borderWidth: 1,
    borderColor: "#E2E8F0",
    alignItems: "center",
    gap: 10,
  },
  loadingText: {
    color: "#64748B",
    fontSize: 12,
  },
  emptyText: {
    color: "#64748B",
    fontSize: 12,
  },
  activeSection: {
    marginTop: 10,
    gap: 8,
  },
  activeSectionHeader: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  activeSectionTitle: {
    color: "#0F172A",
    fontSize: 13,
    fontWeight: "600",
  },
  activeCountBadge: {
    backgroundColor: "#059669",
    paddingHorizontal: 6,
    paddingVertical: 1,
    borderRadius: 10,
  },
  activeCountText: {
    color: "#FFFFFF",
    fontSize: 10,
    fontWeight: "700",
    fontFamily: "monospace",
  },
  streamCard: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#A7F3D0",
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
  liveDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#059669",
    flexShrink: 0,
  },
  streamName: {
    color: "#0F172A",
    fontSize: 12,
    fontWeight: "600",
  },
  streamPort: {
    color: "#64748B",
    fontSize: 10,
    fontFamily: "monospace",
  },
  stopBtn: {
    backgroundColor: "#FEF2F2",
    borderWidth: 1,
    borderColor: "#FECACA",
    paddingHorizontal: 8,
    paddingVertical: 4,
    borderRadius: 6,
    flexShrink: 0,
  },
  stopBtnText: {
    color: "#DC2626",
    fontSize: 11,
    fontWeight: "600",
  },
});
