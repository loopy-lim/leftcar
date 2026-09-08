/**
 * 홈 화면 백그라운드 자동 재연결 판정. 순수 함수 부분(shouldAutoReconnect)은
 * 게이트 상태를 인자로 받아 JVM 테스트로 검증하고, 모듈 게이트는 앱 런타임
 * 상태(사용자가 직접 연결 해제했는지, 조용한 401 재시도 루프에 빠졌는지)를
 * 세션 동안만 기억한다.
 */

/** 같은 호스트에 대한 자동 재시도 최소 간격. 포커스마다 반복 시도를 막는다. */
export const AUTO_RECONNECT_MIN_INTERVAL_MS = 10_000;

export interface AutoReconnectDecision {
  hasClient: boolean;
  hasRecentHost: boolean;
  /** 사용자가 이번 앱 실행 중 "연결 해제"를 눌렀다면 자동 재연결하지 않는다. */
  userDisconnected: boolean;
  /** 자동 연결 중 401(승인 만료)을 만났다면 사용자가 직접 시도할 때까지 멈춘다. */
  pairingStale: boolean;
  lastAttemptAt: number | null;
  now: number;
  minIntervalMs?: number;
}

export function shouldAutoReconnect(decision: AutoReconnectDecision): boolean {
  if (decision.hasClient || !decision.hasRecentHost) return false;
  if (decision.userDisconnected || decision.pairingStale) return false;
  const minInterval = decision.minIntervalMs ?? AUTO_RECONNECT_MIN_INTERVAL_MS;
  if (
    decision.lastAttemptAt !== null &&
    decision.now - decision.lastAttemptAt < minInterval
  ) {
    return false;
  }
  return true;
}

interface AutoReconnectGate {
  userDisconnected: boolean;
  pairingStale: boolean;
  lastAttemptAt: number | null;
}

const gate: AutoReconnectGate = {
  userDisconnected: false,
  pairingStale: false,
  lastAttemptAt: null,
};

export function markUserDisconnected(): void {
  gate.userDisconnected = true;
}

export function markPairingStale(): void {
  gate.pairingStale = true;
}

/** 어떤 경로로든 연결에 성공하면 게이트를 초기화해 자동 재연결 조건을 되살린다. */
export function markConnected(): void {
  gate.userDisconnected = false;
  gate.pairingStale = false;
}

export function noteAutoReconnectAttempt(now: number): void {
  gate.lastAttemptAt = now;
}

/** 뷰어 화면용 게이트 스냅숏을 판정 함수에 넘긴다(테스트 주입 지점). */
export function shouldAutoReconnectFromGate(
  hasClient: boolean,
  hasRecentHost: boolean,
  now: number,
): boolean {
  return shouldAutoReconnect({
    hasClient,
    hasRecentHost,
    userDisconnected: gate.userDisconnected,
    pairingStale: gate.pairingStale,
    lastAttemptAt: gate.lastAttemptAt,
    now,
  });
}
