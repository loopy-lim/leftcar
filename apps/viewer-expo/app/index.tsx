import { useCallback, useState } from "react";
import {
  Alert,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { Ionicons } from "@expo/vector-icons";
import { SafeAreaView } from "react-native-safe-area-context";
import { router, useFocusEffect } from "expo-router";
import { controlClient, controlHost, disconnectHost } from "../src/session";
import { clearToken } from "../src/pairing";
import { isUnauthorizedError, type CatalogView } from "../src/control";

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
  const [hostAddr, setHostAddr] = useState<string>("");
  const [isConnected, setIsConnected] = useState<boolean>(false);

  const checkConnection = useCallback(() => {
    const client = controlClient();
    const addr = controlHost();
    setIsConnected(!!client);
    setHostAddr(addr);
  }, []);

  const handleDisconnect = useCallback(() => {
    disconnectHost();
    checkConnection();
  }, [checkConnection]);

  useFocusEffect(
    useCallback(() => {
      checkConnection();
      const client = controlClient();
      if (client) {
        // Verify that the current connection and token are actually authorized
        client.request<CatalogView>("getCatalog").catch((e) => {
          if (isUnauthorizedError(e)) {
            void (async () => {
              await clearToken();
              disconnectHost();
              checkConnection();
            })();
          }
        });
      }
    }, [checkConnection])
  );

  return (
    <SafeAreaView style={styles.safeArea} edges={["top", "left", "right", "bottom"]}>
      <ScrollView
        style={styles.root}
        contentContainerStyle={styles.content}
        showsVerticalScrollIndicator={false}
      >
        {/* Top App Branding */}
        <View style={styles.brandHeader}>
          <View style={styles.logoRow}>
            <View style={styles.logoBadge}>
              <Ionicons name="desktop-outline" size={24} color="#2563EB" />
            </View>
            <View style={styles.titleColumn}>
              <Text style={styles.appTitle}>Leftcar XR</Text>
              <Text style={styles.appSubtitle}>초저지연 다중 데스크톱 화면 스트리밍</Text>
            </View>
          </View>
        </View>

        {/* Hero Connection Card */}
        {isConnected ? (
          <View style={styles.heroCardConnected}>
            <View style={styles.cardTopRow}>
              <View style={styles.badgeSuccess}>
                <View style={styles.dotSuccess} />
                <Text style={styles.badgeSuccessText}>호스트 연결됨</Text>
              </View>
              <Text style={styles.endpointLabel} numberOfLines={1}>
                {hostAddr}
              </Text>
            </View>

            <View style={styles.heroBody}>
              <Text style={styles.heroTitle}>모니터 화면을 선택하세요</Text>
              <Text style={styles.heroDesc}>
                컴퓨터의 디스플레이를 가상 공간에 독립된 창으로 열어 작업할 수 있습니다.
              </Text>
            </View>

            <View style={styles.heroActionRow}>
              <Pressable onPress={openCatalog} style={styles.primaryActionBtn}>
                <Text style={styles.primaryActionText}>화면 목록 보기 →</Text>
              </Pressable>
              <Pressable onPress={openHostPicker} style={styles.secondaryActionBtn}>
                <Text style={styles.secondaryActionText}>호스트 변경</Text>
              </Pressable>
              <Pressable onPress={handleDisconnect} style={styles.disconnectActionBtn}>
                <Text style={styles.disconnectActionText}>연결 해제</Text>
              </Pressable>
            </View>
          </View>
        ) : (
          <View style={styles.heroCardStandby}>
            <View style={styles.cardTopRow}>
              <View style={styles.badgeStandby}>
                <View style={styles.dotStandby} />
                <Text style={styles.badgeStandbyText}>연결 대기 중</Text>
              </View>
            </View>

            <View style={styles.heroBody}>
              <Text style={styles.heroTitle}>컴퓨터와 연결하기</Text>
              <Text style={styles.heroDesc}>
                동일한 Wi-Fi 네트워크의 컴퓨터를 탐색하거나 QR 코드로 즉시 페어링하세요.
              </Text>
            </View>

            <View style={styles.heroActionRow}>
              <Pressable onPress={openHostPicker} style={styles.primaryActionBtn}>
                <Text style={styles.primaryActionText}>호스트 찾기 →</Text>
              </Pressable>
              <Pressable onPress={openPairing} style={styles.secondaryActionBtn}>
                <Text style={styles.secondaryActionText}>QR 페어링</Text>
              </Pressable>
            </View>
          </View>
        )}

        {/* 3-Step Setup Guide */}
        <View style={styles.sectionCard}>
          <Text style={styles.sectionTitle}>빠른 시작 가이드</Text>

          <View style={styles.stepRow}>
            <View style={styles.stepBadge}>
              <Text style={styles.stepNum}>1</Text>
            </View>
            <View style={styles.stepInfo}>
              <Text style={styles.stepName}>호스트 앱 실행</Text>
              <Text style={styles.stepText}>Mac 또는 PC에서 Leftcar Host를 켭니다.</Text>
            </View>
          </View>

          <View style={styles.divider} />

          <View style={styles.stepRow}>
            <View style={styles.stepBadge}>
              <Text style={styles.stepNum}>2</Text>
            </View>
            <View style={styles.stepInfo}>
              <Text style={styles.stepName}>기기 연결 및 페어링</Text>
              <Text style={styles.stepText}>[호스트 찾기]를 눌러 컴퓨터를 선택합니다.</Text>
            </View>
          </View>

          <View style={styles.divider} />

          <View style={styles.stepRow}>
            <View style={styles.stepBadge}>
              <Text style={styles.stepNum}>3</Text>
            </View>
            <View style={styles.stepInfo}>
              <Text style={styles.stepName}>가상 공간에 화면 배치</Text>
              <Text style={styles.stepText}>원하는 모니터를 열어 자유롭게 배치합니다.</Text>
            </View>
          </View>
        </View>

        {/* Quick Specs Grid (2 Column Clean Layout) */}
        <View style={styles.featureGrid}>
          <View style={styles.featureBox}>
            <Ionicons name="flash-outline" size={24} color="#2563EB" style={{ marginBottom: 4 }} />
            <Text style={styles.featureValue}>&lt;30ms 초저지연</Text>
            <Text style={styles.featureLabel}>실시간 마우스 조작 반응</Text>
          </View>
          <View style={styles.featureBox}>
            <Ionicons name="tv-outline" size={24} color="#2563EB" style={{ marginBottom: 4 }} />
            <Text style={styles.featureValue}>독립 다중 창</Text>
            <Text style={styles.featureLabel}>모니터별 개별 XR 배치</Text>
          </View>
        </View>
      </ScrollView>
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
    paddingHorizontal: 20,
    paddingTop: 16,
    paddingBottom: 32,
    gap: 16,
  },
  brandHeader: {
    paddingVertical: 8,
  },
  logoRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
  },
  logoBadge: {
    width: 44,
    height: 44,
    borderRadius: 12,
    backgroundColor: "#EFF6FF",
    borderWidth: 1,
    borderColor: "#DBEAFE",
    alignItems: "center",
    justifyContent: "center",
  },
  logoIcon: {
    fontSize: 22,
  },
  titleColumn: {
    flex: 1,
    gap: 2,
  },
  appTitle: {
    color: "#0F172A",
    fontSize: 22,
    fontWeight: "700",
    letterSpacing: -0.4,
  },
  appSubtitle: {
    color: "#64748B",
    fontSize: 12,
  },

  /* Hero Cards */
  heroCardConnected: {
    backgroundColor: "#FFFFFF",
    borderRadius: 14,
    borderWidth: 1,
    borderColor: "#A7F3D0",
    padding: 18,
    gap: 12,
    boxShadow: "0 2px 4px rgba(0, 0, 0, 0.05)",
  },
  heroCardStandby: {
    backgroundColor: "#FFFFFF",
    borderRadius: 14,
    borderWidth: 1,
    borderColor: "#E2E8F0",
    padding: 18,
    gap: 12,
    boxShadow: "0 2px 4px rgba(0, 0, 0, 0.04)",
  },
  cardTopRow: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: 8,
  },
  badgeSuccess: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    backgroundColor: "#ECFDF5",
    borderWidth: 1,
    borderColor: "#A7F3D0",
    paddingHorizontal: 8,
    paddingVertical: 4,
    borderRadius: 12,
  },
  dotSuccess: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#059669",
  },
  badgeSuccessText: {
    color: "#059669",
    fontSize: 11,
    fontWeight: "600",
  },
  badgeStandby: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    backgroundColor: "#F1F5F9",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    paddingHorizontal: 8,
    paddingVertical: 4,
    borderRadius: 12,
  },
  dotStandby: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#94A3B8",
  },
  badgeStandbyText: {
    color: "#64748B",
    fontSize: 11,
    fontWeight: "500",
  },
  endpointLabel: {
    color: "#64748B",
    fontSize: 12,
    fontFamily: "monospace",
    flex: 1,
    textAlign: "right",
  },
  heroBody: {
    gap: 4,
  },
  heroTitle: {
    fontSize: 16,
    fontWeight: "600",
    color: "#0F172A",
  },
  heroDesc: {
    fontSize: 13,
    color: "#64748B",
    lineHeight: 18,
  },
  heroActionRow: {
    flexDirection: "row",
    gap: 8,
    marginTop: 4,
  },
  primaryActionBtn: {
    flex: 1,
    backgroundColor: "#2563EB",
    borderRadius: 8,
    paddingVertical: 11,
    alignItems: "center",
    justifyContent: "center",
  },
  primaryActionText: {
    color: "#FFFFFF",
    fontSize: 13,
    fontWeight: "600",
  },
  secondaryActionBtn: {
    backgroundColor: "#F1F5F9",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    borderRadius: 8,
    paddingHorizontal: 14,
    paddingVertical: 11,
    alignItems: "center",
    justifyContent: "center",
  },
  secondaryActionText: {
    color: "#334155",
    fontSize: 13,
    fontWeight: "600",
  },
  disconnectActionBtn: {
    backgroundColor: "#FEF2F2",
    borderWidth: 1,
    borderColor: "#FECACA",
    borderRadius: 8,
    paddingHorizontal: 12,
    paddingVertical: 11,
    alignItems: "center",
    justifyContent: "center",
  },
  disconnectActionText: {
    color: "#DC2626",
    fontSize: 13,
    fontWeight: "600",
  },

  /* Setup Guide */
  sectionCard: {
    backgroundColor: "#FFFFFF",
    borderRadius: 14,
    borderWidth: 1,
    borderColor: "#E2E8F0",
    padding: 18,
    gap: 14,
    boxShadow: "0 1px 3px rgba(0, 0, 0, 0.03)",
  },
  sectionTitle: {
    fontSize: 14,
    fontWeight: "600",
    color: "#0F172A",
  },
  stepRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
  },
  stepBadge: {
    width: 24,
    height: 24,
    borderRadius: 12,
    backgroundColor: "#EFF6FF",
    borderWidth: 1,
    borderColor: "#BFDBFE",
    alignItems: "center",
    justifyContent: "center",
  },
  stepNum: {
    color: "#2563EB",
    fontSize: 11,
    fontWeight: "700",
  },
  stepInfo: {
    flex: 1,
    gap: 1,
  },
  stepName: {
    fontSize: 13,
    fontWeight: "600",
    color: "#0F172A",
  },
  stepText: {
    fontSize: 12,
    color: "#64748B",
  },
  divider: {
    height: 1,
    backgroundColor: "#F1F5F9",
    marginLeft: 36,
  },

  /* 2-Column Feature Grid */
  featureGrid: {
    flexDirection: "row",
    gap: 10,
  },
  featureBox: {
    flex: 1,
    backgroundColor: "#FFFFFF",
    borderRadius: 12,
    borderWidth: 1,
    borderColor: "#E2E8F0",
    padding: 14,
    gap: 4,
  },
  featureEmoji: {
    fontSize: 18,
    marginBottom: 2,
  },
  featureValue: {
    fontSize: 13,
    fontWeight: "600",
    color: "#0F172A",
  },
  featureLabel: {
    fontSize: 11,
    color: "#64748B",
    lineHeight: 15,
  },
});
