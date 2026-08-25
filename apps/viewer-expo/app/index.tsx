import { useCallback, useState } from "react";
import {
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
              <Ionicons name="desktop" size={20} color="#FFFFFF" />
            </View>
            <View style={styles.titleColumn}>
              <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                <Text style={styles.appTitle}>Leftcar Viewer</Text>
                <View style={styles.versionBadge}>
                  <Text style={styles.versionBadgeText}>Hub</Text>
                </View>
              </View>
              <Text style={styles.appSubtitle}>내 컴퓨터 화면을 초저지연으로 이어서 보기</Text>
            </View>
          </View>
        </View>

        {/* Hero Connection Card */}
        {isConnected ? (
          <View style={styles.heroCardConnected}>
            <View style={styles.cardTopRow}>
              <View style={styles.badgeSuccess}>
                <View style={styles.dotSuccess} />
                <Text style={styles.badgeSuccessText}>컴퓨터 연결됨</Text>
              </View>
              <Text style={styles.endpointLabel} numberOfLines={1}>
                {hostAddr}
              </Text>
            </View>

            <View style={styles.heroBody}>
              <Text style={styles.heroTitle}>열어 볼 화면을 선택하세요</Text>
              <Text style={styles.heroDesc}>
                컴퓨터의 각 디스플레이를 독립된 고화질 창으로 열고 공간에 자유롭게 배치할 수 있습니다.
              </Text>
            </View>

            <View style={styles.heroActionRow}>
              <Pressable onPress={openCatalog} style={styles.primaryActionBtn}>
                <Text style={styles.primaryActionText}>화면 목록 보기 →</Text>
              </Pressable>
              <Pressable onPress={openHostPicker} style={styles.secondaryActionBtn}>
                <Text style={styles.secondaryActionText}>컴퓨터 변경</Text>
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
              <Text style={styles.networkHintText}>동일 Wi-Fi 권장</Text>
            </View>

            <View style={styles.heroBody}>
              <Text style={styles.heroTitle}>내 컴퓨터 연결하기</Text>
              <Text style={styles.heroDesc}>
                동일한 로컬 Wi-Fi 또는 Tailscale 네트워크의 컴퓨터를 자동으로 찾거나 QR 코드로 즉시 페어링하세요.
              </Text>
            </View>

            <View style={styles.heroActionRow}>
              <Pressable onPress={openHostPicker} style={styles.primaryActionBtn}>
                <Text style={styles.primaryActionText}>컴퓨터 찾기 →</Text>
              </Pressable>
              <Pressable onPress={openPairing} style={styles.secondaryActionBtn}>
                <Ionicons name="qr-code-outline" size={15} color="#09090B" style={{ marginRight: 4 }} />
                <Text style={styles.secondaryActionText}>QR 연결</Text>
              </Pressable>
            </View>
          </View>
        )}

        {/* 3-Step Setup Guide */}
        <View style={styles.sectionCard}>
          <Text style={styles.sectionTitle}>간편 3단계 시작 가이드</Text>

          <View style={styles.stepRow}>
            <View style={styles.stepBadge}>
              <Text style={styles.stepNum}>1</Text>
            </View>
            <View style={styles.stepInfo}>
              <Text style={styles.stepName}>컴퓨터 앱 실행</Text>
              <Text style={styles.stepText}>Mac 또는 PC에서 Leftcar Host Studio를 엽니다.</Text>
            </View>
          </View>

          <View style={styles.divider} />

          <View style={styles.stepRow}>
            <View style={styles.stepBadge}>
              <Text style={styles.stepNum}>2</Text>
            </View>
            <View style={styles.stepInfo}>
              <Text style={styles.stepName}>동일 네트워크 확인</Text>
              <Text style={styles.stepText}>호스트와 뷰어가 동일한 Wi-Fi(5GHz 권장)에 연결되어 있는지 확인합니다.</Text>
            </View>
          </View>

          <View style={styles.divider} />

          <View style={styles.stepRow}>
            <View style={styles.stepBadge}>
              <Text style={styles.stepNum}>3</Text>
            </View>
            <View style={styles.stepInfo}>
              <Text style={styles.stepName}>화면 연결 & 공간 배치</Text>
              <Text style={styles.stepText}>[컴퓨터 찾기]를 누르거나 QR 코드를 스캔하여 화면을 엽니다.</Text>
            </View>
          </View>
        </View>

        {/* Quick Specs Grid (2 Column Clean Layout) */}
        <View style={styles.featureGrid}>
          <View style={styles.featureBox}>
            <View style={styles.featureIconBox}>
              <Ionicons name="flash" size={16} color="#09090B" />
            </View>
            <Text style={styles.featureValue}>초저지연 60 FPS</Text>
            <Text style={styles.featureLabel}>화면을 보면서 즉시 마우스·키보드 조작</Text>
          </View>
          <View style={styles.featureBox}>
            <View style={styles.featureIconBox}>
              <Ionicons name="tv" size={16} color="#09090B" />
            </View>
            <Text style={styles.featureValue}>멀티 디스플레이</Text>
            <Text style={styles.featureLabel}>모니터마다 개별 공간 창으로 분리 배치</Text>
          </View>
        </View>
      </ScrollView>
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
    paddingHorizontal: 18,
    paddingTop: 14,
    paddingBottom: 32,
    gap: 14,
  },
  brandHeader: {
    paddingVertical: 6,
  },
  logoRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
  },
  logoBadge: {
    width: 38,
    height: 38,
    borderRadius: 8,
    backgroundColor: "#09090B",
    alignItems: "center",
    justifyContent: "center",
  },
  titleColumn: {
    flex: 1,
    gap: 2,
  },
  appTitle: {
    color: "#09090B",
    fontSize: 18,
    fontWeight: "700",
    letterSpacing: -0.3,
  },
  versionBadge: {
    backgroundColor: "#F4F4F5",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    paddingHorizontal: 6,
    paddingVertical: 1,
    borderRadius: 4,
  },
  versionBadgeText: {
    color: "#52525B",
    fontSize: 10,
    fontWeight: "700",
    fontFamily: "monospace",
  },
  appSubtitle: {
    color: "#71717A",
    fontSize: 12,
  },

  /* Hero Cards */
  heroCardConnected: {
    backgroundColor: "#FFFFFF",
    borderRadius: 12,
    borderWidth: 1,
    borderColor: "#D4D4D8",
    padding: 16,
    gap: 12,
  },
  heroCardStandby: {
    backgroundColor: "#FFFFFF",
    borderRadius: 12,
    borderWidth: 1,
    borderColor: "#E4E4E7",
    padding: 16,
    gap: 12,
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
    backgroundColor: "#F4F4F5",
    borderWidth: 1,
    borderColor: "#D4D4D8",
    paddingHorizontal: 8,
    paddingVertical: 3,
    borderRadius: 12,
  },
  dotSuccess: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#09090B",
  },
  badgeSuccessText: {
    color: "#09090B",
    fontSize: 11,
    fontWeight: "700",
  },
  badgeStandby: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    backgroundColor: "#F4F4F5",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    paddingHorizontal: 8,
    paddingVertical: 3,
    borderRadius: 12,
  },
  dotStandby: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#A1A1AA",
  },
  badgeStandbyText: {
    color: "#71717A",
    fontSize: 11,
    fontWeight: "600",
  },
  networkHintText: {
    color: "#A1A1AA",
    fontSize: 11,
  },
  endpointLabel: {
    color: "#71717A",
    fontSize: 12,
    fontFamily: "monospace",
    fontVariant: ["tabular-nums"],
    flex: 1,
    textAlign: "right",
  },
  heroBody: {
    gap: 3,
  },
  heroTitle: {
    fontSize: 15,
    fontWeight: "700",
    color: "#09090B",
    letterSpacing: -0.2,
  },
  heroDesc: {
    fontSize: 12,
    color: "#52525B",
    lineHeight: 17,
  },
  heroActionRow: {
    flexDirection: "row",
    gap: 8,
    marginTop: 4,
  },
  primaryActionBtn: {
    flex: 1,
    backgroundColor: "#09090B",
    borderRadius: 8,
    paddingVertical: 10,
    alignItems: "center",
    justifyContent: "center",
  },
  primaryActionText: {
    color: "#FFFFFF",
    fontSize: 12,
    fontWeight: "600",
  },
  secondaryActionBtn: {
    backgroundColor: "#F4F4F5",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    borderRadius: 8,
    paddingHorizontal: 12,
    paddingVertical: 10,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
  },
  secondaryActionText: {
    color: "#09090B",
    fontSize: 12,
    fontWeight: "600",
  },
  disconnectActionBtn: {
    backgroundColor: "#FFFFFF",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    borderRadius: 8,
    paddingHorizontal: 12,
    paddingVertical: 10,
    alignItems: "center",
    justifyContent: "center",
  },
  disconnectActionText: {
    color: "#71717A",
    fontSize: 12,
    fontWeight: "600",
  },

  /* Setup Guide */
  sectionCard: {
    backgroundColor: "#FFFFFF",
    borderRadius: 12,
    borderWidth: 1,
    borderColor: "#E4E4E7",
    padding: 16,
    gap: 12,
  },
  sectionTitle: {
    fontSize: 11,
    fontWeight: "700",
    color: "#71717A",
    textTransform: "uppercase",
    letterSpacing: 0.04,
  },
  stepRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
  },
  stepBadge: {
    width: 22,
    height: 22,
    borderRadius: 11,
    backgroundColor: "#09090B",
    alignItems: "center",
    justifyContent: "center",
  },
  stepNum: {
    color: "#FFFFFF",
    fontSize: 10,
    fontWeight: "700",
    fontFamily: "monospace",
  },
  stepInfo: {
    flex: 1,
    gap: 1,
  },
  stepName: {
    fontSize: 13,
    fontWeight: "600",
    color: "#09090B",
  },
  stepText: {
    fontSize: 11,
    color: "#71717A",
    lineHeight: 15,
  },
  divider: {
    height: 1,
    backgroundColor: "#F4F4F5",
    marginLeft: 34,
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
    borderColor: "#E4E4E7",
    padding: 14,
    gap: 4,
  },
  featureIconBox: {
    width: 28,
    height: 28,
    borderRadius: 6,
    backgroundColor: "#F4F4F5",
    borderWidth: 1,
    borderColor: "#E4E4E7",
    alignItems: "center",
    justifyContent: "center",
    marginBottom: 2,
  },
  featureValue: {
    fontSize: 12,
    fontWeight: "700",
    color: "#09090B",
    fontVariant: ["tabular-nums"],
  },
  featureLabel: {
    fontSize: 11,
    color: "#71717A",
    lineHeight: 15,
  },
});
