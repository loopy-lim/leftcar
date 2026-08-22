import { useCallback, useState } from "react";
import {
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import { router, useFocusEffect } from "expo-router";
import { controlClient, controlHost } from "../src/session";

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

  useFocusEffect(
    useCallback(() => {
      checkConnection();
    }, [checkConnection])
  );

  return (
    <SafeAreaView style={styles.safeArea} edges={["top", "left", "right"]}>
      <ScrollView style={styles.root} contentContainerStyle={styles.content}>
        {/* Header Branding */}
        <View style={styles.header}>
          <View style={styles.brandRow}>
            <View style={styles.logoBadge}>
              <Text style={styles.logoIcon}>🖥️</Text>
            </View>
            <View style={styles.brandTextGroup}>
              <View style={styles.titleRow}>
                <Text style={styles.appTitle}>Leftcar XR</Text>
                <View style={styles.versionBadge}>
                  <Text style={styles.versionText}>v0.1</Text>
                </View>
              </View>
              <Text style={styles.appSubtitle}>초저지연 데스크톱 다중 화면 뷰어</Text>
            </View>
          </View>
        </View>

        {/* Main Status & Action Card */}
        {isConnected ? (
          <View style={styles.connectedCard}>
            <View style={styles.cardHeaderRow}>
              <View style={styles.statusBadgeGreen}>
                <View style={styles.dotGreen} />
                <Text style={styles.statusBadgeTextGreen}>연결됨</Text>
              </View>
              <Text style={styles.hostEndpointText} numberOfLines={1}>
                {hostAddr}
              </Text>
            </View>

            <View style={styles.cardMainText}>
              <Text style={styles.cardMainTitle}>컴퓨터 화면을 선택하세요</Text>
              <Text style={styles.cardMainSub}>
                원하는 모니터를 가상 공간에 독립된 창으로 띄울 수 있습니다.
              </Text>
            </View>

            <View style={styles.btnRow}>
              <Pressable onPress={openCatalog} style={styles.btnPrimary}>
                <Text style={styles.btnPrimaryText}>화면 선택하기 →</Text>
              </Pressable>
              <Pressable onPress={openHostPicker} style={styles.btnSecondary}>
                <Text style={styles.btnSecondaryText}>다른 호스트</Text>
              </Pressable>
            </View>
          </View>
        ) : (
          <View style={styles.disconnectedCard}>
            <View style={styles.cardHeaderRow}>
              <View style={styles.statusBadgeGray}>
                <View style={styles.dotGray} />
                <Text style={styles.statusBadgeTextGray}>연결 대기</Text>
              </View>
            </View>

            <View style={styles.cardMainText}>
              <Text style={styles.cardMainTitle}>컴퓨터에 연결하세요</Text>
              <Text style={styles.cardMainSub}>
                동일한 Wi-Fi에 연결된 Leftcar 호스트를 자동으로 찾거나 QR 코드로 페어링하세요.
              </Text>
            </View>

            <View style={styles.btnRow}>
              <Pressable onPress={openHostPicker} style={styles.btnPrimary}>
                <Text style={styles.btnPrimaryText}>호스트 연결하기 →</Text>
              </Pressable>
              <Pressable onPress={openPairing} style={styles.btnSecondary}>
                <Text style={styles.btnSecondaryText}>QR 페어링</Text>
              </Pressable>
            </View>
          </View>
        )}

        {/* 3-Step Setup Guide */}
        <View style={styles.guideCard}>
          <Text style={styles.guideCardTitle}>간편 사용 가이드</Text>

          <View style={styles.stepItem}>
            <View style={styles.stepNumberBadge}>
              <Text style={styles.stepNumberText}>1</Text>
            </View>
            <View style={styles.stepTextGroup}>
              <Text style={styles.stepTitle}>호스트 앱 실행</Text>
              <Text style={styles.stepDesc}>Mac/Windows에서 Leftcar Host 앱을 실행합니다.</Text>
            </View>
          </View>

          <View style={styles.stepDivider} />

          <View style={styles.stepItem}>
            <View style={styles.stepNumberBadge}>
              <Text style={styles.stepNumberText}>2</Text>
            </View>
            <View style={styles.stepTextGroup}>
              <Text style={styles.stepTitle}>호스트 선택 및 페어링</Text>
              <Text style={styles.stepDesc}>[호스트 연결하기]를 눌러 검색된 컴퓨터를 선택합니다.</Text>
            </View>
          </View>

          <View style={styles.stepDivider} />

          <View style={styles.stepItem}>
            <View style={styles.stepNumberBadge}>
              <Text style={styles.stepNumberText}>3</Text>
            </View>
            <View style={styles.stepTextGroup}>
              <Text style={styles.stepTitle}>XR 공간에 화면 배치</Text>
              <Text style={styles.stepDesc}>모니터를 열고 가상 공간 원하는 위치에 배치하세요.</Text>
            </View>
          </View>
        </View>

        {/* Feature Specs */}
        <View style={styles.specsRow}>
          <View style={styles.specCard}>
            <Text style={styles.specLabel}>지연 시간</Text>
            <Text style={styles.specHighlight}>&lt;30ms</Text>
            <Text style={styles.specSub}>초저지연 반응</Text>
          </View>
          <View style={styles.specCard}>
            <Text style={styles.specLabel}>재생률</Text>
            <Text style={styles.specValue}>60 FPS</Text>
            <Text style={styles.specSub}>부드러운 화면</Text>
          </View>
          <View style={styles.specCard}>
            <Text style={styles.specLabel}>창 모드</Text>
            <Text style={styles.specValue}>독립 창</Text>
            <Text style={styles.specSub}>다중 화면 배치</Text>
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
    padding: 16,
    gap: 14,
    paddingBottom: 36,
  },
  header: {
    marginTop: 4,
    marginBottom: 2,
  },
  brandRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
  },
  logoBadge: {
    width: 42,
    height: 42,
    borderRadius: 10,
    backgroundColor: "#EFF6FF",
    borderWidth: 1,
    borderColor: "#DBEAFE",
    alignItems: "center",
    justifyContent: "center",
  },
  logoIcon: {
    fontSize: 20,
  },
  brandTextGroup: {
    flex: 1,
    gap: 2,
  },
  titleRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  appTitle: {
    color: "#0F172A",
    fontSize: 20,
    fontWeight: "700",
    letterSpacing: -0.3,
  },
  versionBadge: {
    backgroundColor: "#F1F5F9",
    paddingHorizontal: 6,
    paddingVertical: 2,
    borderRadius: 4,
    borderWidth: 1,
    borderColor: "#E2E8F0",
  },
  versionText: {
    color: "#64748B",
    fontSize: 10,
    fontWeight: "600",
    fontFamily: "monospace",
  },
  appSubtitle: {
    color: "#64748B",
    fontSize: 12,
  },

  /* Main Cards */
  connectedCard: {
    backgroundColor: "#FFFFFF",
    borderRadius: 12,
    borderWidth: 1,
    borderColor: "#A7F3D0",
    padding: 16,
    gap: 12,
    shadowColor: "#000000",
    shadowOffset: { width: 0, height: 1 },
    shadowOpacity: 0.05,
    shadowRadius: 2,
    elevation: 1,
  },
  disconnectedCard: {
    backgroundColor: "#FFFFFF",
    borderRadius: 12,
    borderWidth: 1,
    borderColor: "#E2E8F0",
    padding: 16,
    gap: 12,
    shadowColor: "#000000",
    shadowOffset: { width: 0, height: 1 },
    shadowOpacity: 0.05,
    shadowRadius: 2,
    elevation: 1,
  },
  cardHeaderRow: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: 8,
  },
  statusBadgeGreen: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    backgroundColor: "#ECFDF5",
    borderWidth: 1,
    borderColor: "#A7F3D0",
    paddingHorizontal: 8,
    paddingVertical: 3,
    borderRadius: 12,
  },
  dotGreen: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#059669",
  },
  statusBadgeTextGreen: {
    color: "#059669",
    fontSize: 11,
    fontWeight: "600",
  },
  statusBadgeGray: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    backgroundColor: "#F1F5F9",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    paddingHorizontal: 8,
    paddingVertical: 3,
    borderRadius: 12,
  },
  dotGray: {
    width: 6,
    height: 6,
    borderRadius: 3,
    backgroundColor: "#94A3B8",
  },
  statusBadgeTextGray: {
    color: "#64748B",
    fontSize: 11,
    fontWeight: "500",
  },
  hostEndpointText: {
    color: "#64748B",
    fontSize: 12,
    fontFamily: "monospace",
    flex: 1,
    textAlign: "right",
  },
  cardMainText: {
    gap: 4,
  },
  cardMainTitle: {
    fontSize: 15,
    fontWeight: "600",
    color: "#0F172A",
  },
  cardMainSub: {
    fontSize: 12,
    color: "#64748B",
    lineHeight: 18,
  },
  btnRow: {
    flexDirection: "row",
    gap: 8,
    marginTop: 2,
  },
  btnPrimary: {
    flex: 1,
    backgroundColor: "#2563EB",
    borderRadius: 8,
    paddingVertical: 10,
    alignItems: "center",
    justifyContent: "center",
  },
  btnPrimaryText: {
    color: "#FFFFFF",
    fontSize: 13,
    fontWeight: "600",
  },
  btnSecondary: {
    backgroundColor: "#F1F5F9",
    borderWidth: 1,
    borderColor: "#E2E8F0",
    borderRadius: 8,
    paddingHorizontal: 12,
    paddingVertical: 10,
    alignItems: "center",
    justifyContent: "center",
  },
  btnSecondaryText: {
    color: "#334155",
    fontSize: 13,
    fontWeight: "600",
  },

  /* Guide Card */
  guideCard: {
    backgroundColor: "#FFFFFF",
    borderRadius: 12,
    borderWidth: 1,
    borderColor: "#E2E8F0",
    padding: 16,
    gap: 12,
    shadowColor: "#000000",
    shadowOffset: { width: 0, height: 1 },
    shadowOpacity: 0.04,
    shadowRadius: 2,
    elevation: 1,
  },
  guideCardTitle: {
    fontSize: 13,
    fontWeight: "600",
    color: "#0F172A",
  },
  stepItem: {
    flexDirection: "row",
    alignItems: "flex-start",
    gap: 10,
  },
  stepDivider: {
    height: 1,
    backgroundColor: "#F1F5F9",
    marginLeft: 30,
  },
  stepNumberBadge: {
    width: 20,
    height: 20,
    borderRadius: 10,
    backgroundColor: "#EFF6FF",
    borderWidth: 1,
    borderColor: "#BFDBFE",
    alignItems: "center",
    justifyContent: "center",
    marginTop: 1,
  },
  stepNumberText: {
    color: "#2563EB",
    fontSize: 10,
    fontWeight: "700",
  },
  stepTextGroup: {
    flex: 1,
    gap: 1,
  },
  stepTitle: {
    fontSize: 12,
    fontWeight: "600",
    color: "#0F172A",
  },
  stepDesc: {
    fontSize: 11,
    color: "#64748B",
    lineHeight: 16,
  },

  /* Specs */
  specsRow: {
    flexDirection: "row",
    gap: 8,
  },
  specCard: {
    flex: 1,
    backgroundColor: "#FFFFFF",
    borderRadius: 8,
    borderWidth: 1,
    borderColor: "#E2E8F0",
    padding: 10,
    gap: 2,
  },
  specLabel: {
    color: "#94A3B8",
    fontSize: 10,
    fontWeight: "500",
  },
  specHighlight: {
    color: "#059669",
    fontSize: 14,
    fontWeight: "700",
    fontFamily: "monospace",
  },
  specValue: {
    color: "#0F172A",
    fontSize: 14,
    fontWeight: "700",
  },
  specSub: {
    color: "#64748B",
    fontSize: 10,
  },
});
