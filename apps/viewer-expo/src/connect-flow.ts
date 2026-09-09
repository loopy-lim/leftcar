import { Alert } from "react-native";
import { router } from "expo-router";
import { markPairingStale } from "./auto-reconnect";
import { currentTranslation } from "./language-store";
import { clearToken } from "./pairing";
import { disconnectHost } from "./session";

/**
 * 401(토큰 만료) 공통 반응: 토큰 폐기 → 연결 해제 → 선택적 부가 처리 →
 * 안내 창 → 페어링 화면. 조용히 끝낼지(자동 재연결), 어떤 주소를 페어링에
 * 넘길지는 호출부가 고른다.
 */
export async function handleUnauthorized(options: {
  /** 연결 해제 직후, 안내·이동 전에 끝낼 화면 상태 갱신. */
  beforeNavigate?: () => void;
  /** 재연결 게이트를 페어링 만료로 막는다 — 조용한 자동 재연결 경로용. */
  markStale?: boolean;
  navigate?: { endpoint?: string; replace?: boolean };
} = {}): Promise<void> {
  await clearToken();
  disconnectHost();
  if (options.markStale) markPairingStale();
  options.beforeNavigate?.();
  if (options.navigate) {
    Alert.alert(
      currentTranslation().viewer.pairingRequiredTitle,
      currentTranslation().viewer.pairingRequiredDesc,
    );
    const { endpoint, replace } = options.navigate;
    if (replace) {
      router.replace({ pathname: "/pairing", params: { endpoint } });
    } else {
      router.push({ pathname: "/pairing", params: { endpoint } });
    }
  }
}
