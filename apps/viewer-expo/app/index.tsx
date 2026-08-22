import { useCallback, useState } from "react";
import {
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { router, useFocusEffect } from "expo-router";
import { controlClient, controlHost } from "../src/session";

function openCatalog() {
  router.push("/catalog");
}

function openHostPicker() {
  router.push("/host");
}

/**
 * Leftcar XR Viewer Hub (docs/03 §3.2, Expo/RN 구현).
 *
 * Provides control plane connection (via Rustra JNI), source catalog,
 * multi-window document task launcher, and real-time stream status.
 */

export default function Hub() {
  const [hostAddr, setHostAddr] = useState<string>("");
  const [isConnected, setIsConnected] = useState<boolean>(false);

  const checkConnection = useCallback(() => {
    const client = controlClient();
    const addr = controlHost();
    setIsConnected(!!client);
    setHostAddr(addr);
  }, []);

  useFocusEffect(
    useCallback(() => {
      checkConnection();
    }, [checkConnection])
  );

  return (
    <ScrollView style={styles.root} contentContainerStyle={styles.content}>
      {/* Brand Header */}
      <View style={styles.header}>
        <View style={styles.headerTop}>
          <View style={styles.logoBadge}>
            <Text style={styles.logoIcon}>⚡</Text>
          </View>
          <View style={styles.headerTitles}>
            <View style={styles.titleRow}>
              <Text style={styles.title}>Leftcar XR</Text>
              <View style={styles.versionBadge}>
                <Text style={styles.versionText}>v0.1</Text>
              </View>
            </View>
            <Text style={styles.sub}>Galaxy XR · Low-Latency Desktop Viewer</Text>
          </View>
        </View>

        <View style={[styles.badge, isConnected ? styles.badgeConnected : styles.badgeDisconnected]}>
          <View style={[styles.badgeDot, isConnected ? styles.dotConnected : styles.dotDisconnected]} />
          <Text style={[styles.badgeText, isConnected ? styles.badgeTextConnected : styles.badgeTextDisconnected]}>
            {isConnected ? `호스트 연결됨 (${hostAddr})` : "호스트 연결 대기 중"}
          </Text>
        </View>
      </View>

      {/* Main Flow Action Banner */}
      {isConnected ? (
        <View style={styles.connectedBanner}>
          <View style={styles.bannerInfo}>
            <Text style={styles.bannerTitle}>호스트와 통신 중</Text>
            <Text style={styles.bannerSub}>디스플레이 목록을 확인하고 독립 XR 창을 엽니다</Text>
          </View>
          <View style={styles.bannerActions}>
            <Pressable
              onPress={openCatalog}
              style={styles.primaryActionBtn}
            >
              <Text style={styles.primaryActionText}>소스 카탈로그 열기 →</Text>
            </Pressable>
            <Pressable
              onPress={openHostPicker}
              style={styles.disconnectBtn}
            >
              <Text style={styles.disconnectBtnText}>호스트 바꾸기</Text>
            </Pressable>
          </View>
        </View>
      ) : (
        <Pressable
          onPress={openHostPicker}
          style={styles.actionBanner}
        >
          <View style={styles.actionBannerLeft}>
            <Text style={styles.actionBannerTitle}>호스트 연결 및 디스플레이 탐색</Text>
            <Text style={styles.actionBannerSub}>mDNS 자동 검색 또는 수동 IP로 Mac/PC 연결</Text>
          </View>
          <View style={styles.actionBannerArrow}>
            <Text style={styles.actionBannerArrowText}>→</Text>
          </View>
        </Pressable>
      )}

      {/* Quick Step Guide for User UX */}
      <View style={styles.card}>
        <Text style={styles.cardTitle}>🚀 시작하기 UX 가이드</Text>
        <View style={styles.stepList}>
          <View style={styles.stepItem}>
            <View style={styles.stepNumBadge}>
              <Text style={styles.stepNum}>1</Text>
            </View>
            <View style={styles.stepContent}>
              <Text style={styles.stepTitle}>Mac/PC에서 Host 앱 실행</Text>
              <Text style={styles.stepDesc}>Leftcar Host Studio를 실행하면 로컬 포트 7777로 대기합니다.</Text>
            </View>
          </View>

          <View style={styles.stepItem}>
            <View style={styles.stepNumBadge}>
              <Text style={styles.stepNum}>2</Text>
            </View>
            <View style={styles.stepContent}>
              <Text style={styles.stepTitle}>호스트 탐색 및 연결</Text>
              <Text style={styles.stepDesc}>[호스트 연결] 버튼을 눌러 동일 Wi-Fi의 기기를 자동 선택합니다.</Text>
            </View>
          </View>

          <View style={styles.stepItem}>
            <View style={styles.stepNumBadge}>
              <Text style={styles.stepNum}>3</Text>
            </View>
            <View style={styles.stepContent}>
              <Text style={styles.stepTitle}>독립 XR 윈도우 배치</Text>
              <Text style={styles.stepDesc}>원하는 모니터를 [XR 창 열기]하여 공간 어디든 자유롭게 배치하세요.</Text>
            </View>
          </View>
        </View>
      </View>

      {/* Metric Highlights */}
      <View style={styles.metricRow}>
        <View style={styles.metricCard}>
          <Text style={styles.metricLabel}>Target Latency</Text>
          <Text style={[styles.metricValue, { color: "#34d399" }]}>p50 &lt;28ms</Text>
        </View>
        <View style={styles.metricCard}>
          <Text style={styles.metricLabel}>Hardware Decode</Text>
          <Text style={[styles.metricValue, { color: "#38bdf8" }]}>AMediaCodec</Text>
        </View>
        <View style={styles.metricCard}>
          <Text style={styles.metricLabel}>Transport</Text>
          <Text style={[styles.metricValue, { color: "#a5b4fc" }]}>Direct UDP/NDK</Text>
        </View>
      </View>

      {/* Pipeline Info Card */}
      <View style={styles.infoCard}>
        <Text style={styles.infoTitle}>⚡ Zero-Copy Hardware Pipeline</Text>
        <Text style={styles.footnote}>
          Video plane은 React Native/Rustra 런타임을 거치지 않고 Android NDK AMediaCodec ➡️ Surface 직결 경로로 초저지연 디코딩 및 렌더링됩니다.
        </Text>
      </View>
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
    backgroundColor: "#080b11",
  },
  content: {
    padding: 20,
    paddingTop: 48,
    gap: 16,
    paddingBottom: 40,
  },
  header: {
    gap: 12,
  },
  headerTop: {
    flexDirection: "row",
    alignItems: "center",
    gap: 14,
  },
  logoBadge: {
    width: 44,
    height: 44,
    borderRadius: 12,
    backgroundColor: "#161f33",
    borderWidth: 1,
    borderColor: "rgba(99, 102, 241, 0.4)",
    alignItems: "center",
    justifyContent: "center",
  },
  logoIcon: {
    fontSize: 20,
  },
  headerTitles: {
    flex: 1,
    gap: 2,
  },
  titleRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  title: {
    color: "#f8fafc",
    fontSize: 24,
    fontWeight: "800",
    letterSpacing: -0.5,
  },
  versionBadge: {
    backgroundColor: "#1e293b",
    paddingHorizontal: 6,
    paddingVertical: 2,
    borderRadius: 4,
    borderWidth: 1,
    borderColor: "#334155",
  },
  versionText: {
    color: "#94a3b8",
    fontSize: 10,
    fontWeight: "600",
    fontFamily: "monospace",
  },
  sub: {
    color: "#94a3b8",
    fontSize: 12,
    fontWeight: "400",
  },
  badge: {
    paddingHorizontal: 10,
    paddingVertical: 5,
    borderRadius: 20,
    alignSelf: "flex-start",
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  badgeConnected: {
    backgroundColor: "rgba(16, 185, 129, 0.12)",
    borderWidth: 1,
    borderColor: "rgba(16, 185, 129, 0.3)",
  },
  badgeDisconnected: {
    backgroundColor: "rgba(245, 158, 11, 0.12)",
    borderWidth: 1,
    borderColor: "rgba(245, 158, 11, 0.3)",
  },
  badgeDot: {
    width: 6,
    height: 6,
    borderRadius: 3,
  },
  dotConnected: {
    backgroundColor: "#10b981",
  },
  dotDisconnected: {
    backgroundColor: "#f59e0b",
  },
  badgeText: {
    fontSize: 12,
    fontWeight: "600",
  },
  badgeTextConnected: {
    color: "#34d399",
  },
  badgeTextDisconnected: {
    color: "#fbbf24",
  },
  actionBanner: {
    backgroundColor: "#0f172a",
    borderWidth: 1,
    borderColor: "rgba(99, 102, 241, 0.4)",
    borderRadius: 14,
    padding: 16,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  actionBannerLeft: {
    flex: 1,
    gap: 4,
  },
  actionBannerTitle: {
    color: "#f8fafc",
    fontSize: 15,
    fontWeight: "700",
  },
  actionBannerSub: {
    color: "#94a3b8",
    fontSize: 12,
  },
  actionBannerArrow: {
    width: 32,
    height: 32,
    borderRadius: 8,
    backgroundColor: "#6366f1",
    alignItems: "center",
    justifyContent: "center",
    marginLeft: 12,
  },
  actionBannerArrowText: {
    color: "#ffffff",
    fontSize: 16,
    fontWeight: "700",
  },
  connectedBanner: {
    backgroundColor: "#0f172a",
    borderWidth: 1,
    borderColor: "rgba(16, 185, 129, 0.35)",
    borderRadius: 14,
    padding: 16,
    gap: 12,
  },
  bannerInfo: {
    gap: 2,
  },
  bannerTitle: {
    color: "#f8fafc",
    fontSize: 15,
    fontWeight: "700",
  },
  bannerSub: {
    color: "#94a3b8",
    fontSize: 12,
  },
  bannerActions: {
    flexDirection: "row",
    gap: 10,
  },
  primaryActionBtn: {
    flex: 1,
    backgroundColor: "#6366f1",
    borderRadius: 8,
    paddingVertical: 10,
    alignItems: "center",
    justifyContent: "center",
  },
  primaryActionText: {
    color: "#ffffff",
    fontSize: 13,
    fontWeight: "700",
  },
  disconnectBtn: {
    backgroundColor: "#1e293b",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.08)",
    borderRadius: 8,
    paddingHorizontal: 14,
    paddingVertical: 10,
    alignItems: "center",
    justifyContent: "center",
  },
  disconnectBtnText: {
    color: "#94a3b8",
    fontSize: 12,
    fontWeight: "600",
  },
  card: {
    backgroundColor: "#0f172a",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.08)",
    borderRadius: 14,
    padding: 16,
    gap: 14,
  },
  cardTitle: {
    color: "#f8fafc",
    fontSize: 15,
    fontWeight: "700",
  },
  stepList: {
    gap: 12,
  },
  stepItem: {
    flexDirection: "row",
    alignItems: "flex-start",
    gap: 12,
  },
  stepNumBadge: {
    width: 24,
    height: 24,
    borderRadius: 12,
    backgroundColor: "#161f33",
    borderWidth: 1,
    borderColor: "rgba(99, 102, 241, 0.4)",
    alignItems: "center",
    justifyContent: "center",
    marginTop: 2,
  },
  stepNum: {
    color: "#818cf8",
    fontSize: 12,
    fontWeight: "700",
    fontFamily: "monospace",
  },
  stepContent: {
    flex: 1,
    gap: 2,
  },
  stepTitle: {
    color: "#f8fafc",
    fontSize: 13,
    fontWeight: "600",
  },
  stepDesc: {
    color: "#94a3b8",
    fontSize: 11,
    lineHeight: 16,
  },
  metricRow: {
    flexDirection: "row",
    gap: 8,
  },
  metricCard: {
    flex: 1,
    backgroundColor: "#0f172a",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.08)",
    borderRadius: 10,
    padding: 12,
    gap: 4,
  },
  metricLabel: {
    color: "#64748b",
    fontSize: 10,
    textTransform: "uppercase",
    fontWeight: "600",
    letterSpacing: 0.5,
  },
  metricValue: {
    fontSize: 13,
    fontWeight: "700",
    fontFamily: "monospace",
  },
  infoCard: {
    backgroundColor: "rgba(15, 23, 42, 0.5)",
    borderWidth: 1,
    borderColor: "rgba(255, 255, 255, 0.06)",
    borderRadius: 12,
    padding: 14,
    gap: 6,
  },
  infoTitle: {
    color: "#f8fafc",
    fontSize: 12,
    fontWeight: "600",
  },
  footnote: {
    color: "#64748b",
    fontSize: 11,
    lineHeight: 16,
  },
});
