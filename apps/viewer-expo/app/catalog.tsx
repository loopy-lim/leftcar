import { useCallback, useEffect, useState } from "react";
import {
  ActivityIndicator,
  FlatList,
  StyleSheet,
  Text,
  TouchableOpacity,
  View,
} from "react-native";
import { NativeModules } from "react-native";
import { allocPort, controlClient, controlHost } from "../src/session";
import type { CatalogView, DisplayInfo, SessionView, StatusView } from "../src/control";

type StreamLauncherNative = {
  openStream(port: number, title: string): Promise<string>;
};

const launcher = NativeModules.StreamLauncher as StreamLauncherNative | undefined;

interface ActiveStream {
  port: number;
  session: number;
  sourceName: string;
}

/**
 * Source catalog: list displays from the host, open each in its own OS
 * window (StreamLauncher.openStream), then push the video via startStream.
 */
export default function Catalog() {
  const [displays, setDisplays] = useState<DisplayInfo[]>([]);
  const [streams, setStreams] = useState<ActiveStream[]>([]);
  const [status, setStatus] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    (async () => {
      const client = controlClient();
      if (!client) {
        setError("호스트에 연결되어 있지 않습니다");
        setLoading(false);
        return;
      }
      try {
        const catalog = await client.request<CatalogView>("getCatalog");
        setDisplays(catalog.displays);
      } catch (e) {
        setError(String(e instanceof Error ? e.message : e));
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  // poll session status while mounted
  useEffect(() => {
    const t = setInterval(async () => {
      const client = controlClient();
      if (!client) return;
      try {
        const st = await client.request<StatusView>("getStatus");
        setStatus(
          st.sessions.length
            ? st.sessions
                .map((s) => `${s.sourceName}: ${s.state} ${s.fps}fps ${s.kbps}kbps`)
                .join("\n")
            : "활성 세션 없음",
        );
      } catch {
        setStatus("상태 조회 실패");
      }
    }, 2000);
    return () => clearInterval(t);
  }, []);

  const openDisplay = useCallback(async (d: DisplayInfo) => {
    const client = controlClient();
    if (!client || !launcher) {
      setError(launcher ? "제어 연결 없음" : "네이티브 모듈 없음 (dev build 필요)");
      return;
    }
    try {
      const port = allocPort();
      // 1) open the OS window — its Rust listener binds the port
      await launcher.openStream(port, d.name);
      // 2) wait for the listener, then tell the host to push
      await new Promise((r) => setTimeout(r, 300));
      const out = await client.request<{ session: number }>("startStream", {
        sourceIndex: d.index,
        viewerPort: port,
        width: d.width,
        height: d.height,
        fps: 90,
      });
      setStreams((prev) => [
        ...prev,
        { port, session: out.session, sourceName: d.name },
      ]);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  }, []);

  const stopStream = useCallback(async (a: ActiveStream) => {
    const client = controlClient();
    if (!client) return;
    try {
      await client.request("stopStream", { session: a.session });
      setStreams((prev) => prev.filter((x) => x.session !== a.session));
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
    }
  }, []);

  if (loading) {
    return (
      <View style={[s.container, { justifyContent: "center" }]}>
        <ActivityIndicator />
      </View>
    );
  }

  return (
    <View style={s.container}>
      <Text style={s.title}>소스 카탈로그</Text>
      <Text style={s.hint}>host: {controlHost()}</Text>
      {error && <Text style={s.error}>{error}</Text>}
      <FlatList
        data={displays}
        keyExtractor={(d) => String(d.index)}
        renderItem={({ item }) => (
          <TouchableOpacity style={s.row} onPress={() => openDisplay(item)}>
            <View style={{ flex: 1 }}>
              <Text style={s.name}>{item.name}</Text>
              <Text style={s.meta}>
                {item.width}x{item.height}
              </Text>
            </View>
            <Text style={s.open}>이 창으로 열기 →</Text>
          </TouchableOpacity>
        )}
      />
      {streams.length > 0 && (
        <>
          <Text style={s.subtitle}>활성 스트림</Text>
          {streams.map((a) => (
            <View key={a.session} style={s.streamRow}>
              <Text style={s.name}>
                #{a.session} {a.sourceName} (:{a.port})
              </Text>
              <TouchableOpacity onPress={() => stopStream(a)}>
                <Text style={s.stop}>정지</Text>
              </TouchableOpacity>
            </View>
          ))}
        </>
      )}
      <Text style={s.status}>{status}</Text>
    </View>
  );
}

const s = StyleSheet.create({
  container: { flex: 1, padding: 20, gap: 8, backgroundColor: "#111" },
  title: { color: "#fff", fontSize: 20, fontWeight: "700" },
  subtitle: { color: "#fff", fontSize: 15, fontWeight: "600", marginTop: 12 },
  hint: { color: "#888", fontSize: 12 },
  row: {
    flexDirection: "row",
    alignItems: "center",
    backgroundColor: "#1d1d1d",
    borderRadius: 8,
    padding: 14,
  },
  streamRow: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    backgroundColor: "#1d1d1d",
    borderRadius: 8,
    padding: 10,
  },
  name: { color: "#fff", fontSize: 15 },
  meta: { color: "#888", fontSize: 12 },
  open: { color: "#66bb6a", fontSize: 13, fontWeight: "600" },
  stop: { color: "#ef5350", fontSize: 13, fontWeight: "600" },
  status: { color: "#888", fontSize: 12, marginTop: 8 },
  error: { color: "#ef5350", fontSize: 12 },
});
