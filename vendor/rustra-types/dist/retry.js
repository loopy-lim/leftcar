import { CancelledError, RustraCommandError, isRetryableCode } from './errors.js';
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
export async function withRetry(fn, options) {
    const retries = options?.retries ?? 2;
    if (!Number.isFinite(retries) || retries < 0) {
        throw new TypeError(`withRetry: retries must be a non-negative finite number, got ${retries}`);
    }
    const baseDelayMs = options?.baseDelayMs ?? 100;
    const signal = options?.signal;
    const retryIf = options?.retryIf ?? ((error) => isRetryableCode(error.code));
    let lastError;
    for (let attempt = 0;; attempt++) {
        // 시도 사이(및 첫 호출 전) abort 검사 — fn 호출 없이 즉시 승격.
        if (signal?.aborted)
            throw new CancelledError('withRetry aborted', lastError);
        try {
            return await fn(attempt);
        }
        catch (error) {
            lastError = error;
            if (attempt >= retries ||
                !(error instanceof RustraCommandError) ||
                !retryIf(error, attempt)) {
                throw error;
            }
        }
        // 백오프 sleep — abort 되면 남은 대기를 버리고 즉시 CancelledError.
        await abortableSleep(baseDelayMs * 2 ** attempt, signal, lastError);
    }
}
/** abort 가능한 sleep — signal 이 abort 하면 남은 대기와 무관하게 즉시 거부. */
function abortableSleep(ms, signal, cause) {
    if (signal?.aborted)
        return Promise.reject(new CancelledError('withRetry aborted', cause));
    if (!signal)
        return new Promise((resolve) => setTimeout(resolve, ms));
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
            signal.removeEventListener('abort', onAbort);
            resolve();
        }, ms);
        const onAbort = () => {
            clearTimeout(timer);
            reject(new CancelledError('withRetry aborted', cause));
        };
        signal.addEventListener('abort', onAbort, { once: true });
    });
}
//# sourceMappingURL=retry.js.map