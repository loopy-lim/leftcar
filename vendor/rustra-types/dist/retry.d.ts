import { RustraCommandError } from './errors.js';
/** [`withRetry`] 옵션. 모든 필드는 선택 — 기본값은 각 필드 설명 참고. */
export type RetryOptions = {
    /**
     * 마지막 시도 실패 뒤 추가 재시도 횟수 — 기본 2 (총 시도 3회).
     * 음수/비유한 값(NaN, ±Infinity)은 `TypeError` 로 즉시 거부한다 —
     * `attempt >= NaN` 은 항상 거짓이라 조용히 무한 루프에 빠지기 때문.
     */
    retries?: number;
    /** 백오프 기준 지연(ms) — 실패한 attempt N 뒤 `baseDelayMs * 2^N` 대기. 기본 100. */
    baseDelayMs?: number;
    /** 중단 신호 — abort 시 sleep 중이라도 남은 대기를 버리고 즉시 CancelledError 로 거부. */
    signal?: AbortSignal;
    /**
     * 재시도 판정 — 기본은 `isRetryableCode(error.code)`. 지정하면 기본 판정을
     * 완전히 대체한다(합성 아님). `attempt` 는 방금 실패한 시도(0-based).
     * 판정 함수 자체가 던진 에러는 재시도 없이 그대로 거부된다(마지막 에러 계약과 동일).
     */
    retryIf?: (error: RustraCommandError, attempt: number) => boolean;
};
/**
 * 재시도 가능 소비 유틸 — `fn` 을 실행하고 retryable 실패에 한해 지수 백오프로 재시도한다.
 *
 * 경계 계약:
 * - 마지막 에러는 **원본 그대로**(객체 동일성 보존) 재던진다 — 래핑/정규화 없음.
 *   정규화는 호출자 책임(`normalizeRustraError` 기존 관례).
 * - `RustraCommandError` 가 아닌 reject 값은 retry 판정 없이 즉시 그대로 재던진다
 *   (retryIf 의 파라미터 계약이 RustraCommandError 이므로 판정을 위임할 수 없음).
 * - signal abort 는 즉시 `CancelledError` 승격: 첫 호출 전이면 `fn` 을 호출하지 않고,
 *   백오프 sleep 중이면 남은 대기를 버리고 즉시, 시도 사이에 이미 abort 된 상태로
 *   진입해도 즉시. abort 시점에 실패한 에러가 있었다면 `cause` 로 보존한다.
 * - `retries: 0` → 단일 시도, 지연 없음. retryable 이어도 재시도하지 않는다.
 * - 기본 판정에서 non-retryable 코드(`command.not_found` 등)는 지연 없이 즉시 재던진다.
 * - `fn` 은 0-based attempt 번호를 받는다.
 *
 * ```ts
 * const user = await withRetry((attempt) => invoke('get_user', { id }), {
 *   retries: 2,
 *   baseDelayMs: 100,
 *   signal: controller.signal,
 * });
 * ```
 */
export declare function withRetry<T>(fn: (attempt: number) => Promise<T>, options?: RetryOptions): Promise<T>;
//# sourceMappingURL=retry.d.ts.map