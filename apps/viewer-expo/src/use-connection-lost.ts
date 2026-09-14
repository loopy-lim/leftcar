import { useEffect } from "react";
import {
  beginHostSelection,
  controlClient,
  subscribeConnectionChanged,
} from "./session";
import { getRecentHosts, type RecentHostItem } from "./recent-hosts";

/**
 * 연결이 저절로 끊기면(호스트 재시작·네트워크 전환) 배지를 즉시 갱신하고
 * 저장된 최근 호스트로 조용한 재연결을 시도한다. 재시도 간격은
 * auto-reconnect 게이트의 최소 간격이 흡수한다. `onLost`는 끊김이 확인된
 * 뒤 재연결 대상과 함께 호출된다.
 */
export function useConnectionLost(
  onLost: (target: RecentHostItem | null) => void,
  refreshConnected: () => void,
): void {
  useEffect(() => {
    const unsubscribe = subscribeConnectionChanged(() => {
      refreshConnected();
      if (controlClient()) return;
      void getRecentHosts().then((hosts) => onLost(hosts[0] ?? null));
    });
    return unsubscribe;
  }, [onLost, refreshConnected]);
}
