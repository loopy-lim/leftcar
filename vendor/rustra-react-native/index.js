/**
 * RN용 rustra 엔진 어댑터
 *
 * 글로벌 invoke + RN JSI 전용 엔진을 제공합니다.
 * 설정은 `@rustra/types`의 configure()를 사용합니다.
 */
import { createRkyvV2Engine, parseRustraErrorString, } from '@rustra/types';
export { RustraCommandError, configure, invoke, createRkyvV2Engine, parseRustraErrorString, } from '@rustra/types';
export function createReactNativeEngine(native) {
    const encoder = new TextEncoder();
    const decoder = new TextDecoder();
    return {
        async invoke(command, args) {
            const json = JSON.stringify({ command, args });
            const payload = encoder.encode(json);
            const resultBytes = native.invoke(payload.buffer);
            const resultJson = decoder.decode(resultBytes);
            const response = JSON.parse(resultJson);
            if (!response.ok) {
                throw parseRustraErrorString(response.error);
            }
            return response.result;
        },
    };
}
/**
 * 고속 엔진 — JSI 동기 호출로 Promise 오버헤드 없이 결과를 반환합니다.
 *
 * rkyv V2 바이너리 코덱을 통해 최고 성능의 동기 호출을 제공합니다.
 *
 * @example
 * ```ts
 * import { createFastEngine } from '@rustra/react-native';
 * import { registry } from './generated/rkyv-registry.js';
 *
 * const native = global.__rustraNative;
 * const engine = createFastEngine(native, { rkyvV2Codecs: registry });
 * configure(engine);
 * ```
 */
/**
 * 글로벌 JSI 네이티브 모듈에 접근합니다.
 *
 * JSI가 설치된 후 `global.__rustraNative`에서 네이티브 모듈을 가져옵니다.
 * 설치 전에 호출하면 에러를 던집니다.
 *
 * @example
 * ```ts
 * import { getRustraNative } from '@rustra/react-native';
 * const native = getRustraNative();
 * const engine = createFastEngine(native, { rkyvV2Codecs: registry });
 * ```
 */
export function getRustraNative() {
    const native = globalThis.__rustraNative;
    if (!native) {
        throw new Error('JSI native module not installed. Call installRustraJSI() from your native module first.');
    }
    return native;
}
export function createFastEngine(native, options) {
    return createRkyvV2Engine(native, options.rkyvV2Codecs, {
        contractHash: options.contractHash,
    });
}
/**
 * 비동기 invoke — 무거운 Rust 연산을 JS 스레드에서 오프로드한다.
 *
 * - 네이티브 `invokeTypedAsync` 가 있으면: 즉시 반환, 결과는 JS 콜백 큐로 전달.
 * - 없으면: 동기 fast path(`createFastEngine`)로 폴백 — 마이크로태스크로 래핑해
 *   API 계약(`Promise<T>`)은 항상 동일하게 유지.
 *
 * @example
 * ```ts
 * import { createAsyncEngine } from '@rustra/react-native';
 * const engine = createAsyncEngine(getRustraNative(), { rkyvV2Codecs: registry });
 * const result = await engine.invoke('heavyCompute', { n: 1_000_000 });
 * ```
 */
export function createAsyncEngine(native, options) {
    const syncEngine = createFastEngine(native, options);
    if (typeof native.invokeTypedAsync !== 'function') {
        // 폴백: 동기 엔진 재사용 (Promise 는 sync 엔진이 이미 반환).
        return syncEngine;
    }
    const invokeTypedAsync = native.invokeTypedAsync.bind(native);
    return {
        invoke(command, args) {
            return new Promise((resolve, reject) => {
                invokeTypedAsync(command, args, (result) => resolve(result), (message) => reject(parseRustraErrorString(message)));
            });
        },
    };
}
/**
 * Rust `emit` → JS 콜백 구독. 반환 함수로 구독 해제한다.
 *
 * 네이티브 경로(C++ JSI `onEvent`/`offEvent`) 위에서:
 * - **페이로드 파싱**: C++ 가 JSON 문자열을 JSI 로 그대로 넘기고(경계 횡단
 *   비용 최소화) 이 래퍼가 `JSON.parse` 1회로 객체를 복원한다. 콜백은 항상
 *   파싱된 객체를 받는다.
 * - **스레딩**: Rust `emit` 은 어느 스레드에서든 호출될 수 있다. C++ 이
 *   이벤트를 큐에 적재하고 JS CallInvoker 로 JS 런타임 스레드에 drain 을
 *   예약하므로 콜백은 항상 JS 스레드에서 실행된다.
 * - **전달 계약**: 첫 구독 시 네이티브가 FFI 이벤트 싱크를 설치한다(폴링
 *   경로 → 푸시 전환). 마지막 구독 해제 시 싱크가 해제되어 폴링 경로로
 *   복귀한다. JS 콜백이 throw 해도 나머지 이벤트는 유실되지 않는다.
 *
 * 네이티브가 `onEvent` 를 노출하지 않으면(구버전 브릿지) 구독이 즉시
 * 해제되는 no-op 로 동작한다.
 *
 * @example
 * ```ts
 * import { subscribeEvent } from '@rustra/react-native';
 *
 * const unsubscribe = subscribeEvent(
 *   getRustraNative(), // onEvent/offEvent 를 노출하는 네이티브 객체
 *   'progress.tick',
 *   (payload) => {
 *     console.log(payload.step, '/', payload.total); // 파싱된 객체
 *   },
 * );
 * // 나중에
 * unsubscribe();
 * ```
 */
export function subscribeEvent(native, name, cb) {
    if (typeof native.onEvent !== 'function') {
        // 구버전 네이티브 — no-op 구독 해제 함수 반환.
        return () => { };
    }
    native.onEvent(name, (payloadJson) => {
        // JSON 문자열 → 객체 1회 파싱. 파싱 실패(빈 문자열/손상 페이로드)는
        // null 로 정규화해 콜백 계약을 지킨다.
        let payload = null;
        if (payloadJson && payloadJson.length > 0) {
            try {
                payload = JSON.parse(payloadJson);
            }
            catch {
                payload = null;
            }
        }
        cb(payload);
    });
    return () => {
        if (typeof native.offEvent === 'function') {
            native.offEvent(name);
        }
    };
}
//# sourceMappingURL=index.js.map