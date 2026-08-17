/**
 * RN용 rustra 엔진 어댑터
 *
 * 글로벌 invoke + RN JSI 전용 엔진을 제공합니다.
 * 설정은 `@rustra/types`의 configure()를 사용합니다.
 */
import type { EngineClient as EngineClientType, RustraNative } from '@rustra/types';
export type { EngineClient, RustraError, RkyvV2Codec, RkyvV2Native } from '@rustra/types';
export { RustraCommandError, configure, invoke, createRkyvV2Engine, parseRustraErrorString, } from '@rustra/types';
export type RustraJSINative = {
    invoke(payload: ArrayBuffer): ArrayBuffer;
    invokeRkyvV2(payload: ArrayBuffer): ArrayBuffer;
    /** B1 fast path: JSI 가 노출하는 정적 명령 C++ postcard 코덱. */
    getSchema?(): ArrayBuffer;
    hasStaticCodec?(name: string): boolean;
    invokeTyped?(name: string, args: unknown): unknown;
    invokeTypedBatch?(names: string[], args: unknown[]): unknown[];
    /**
     * Rust → JS 이벤트 푸시(RN JSI EventDispatcher). 콜백 인자는 JSON 문자열 —
     * `subscribeEvent` 래퍼가 파싱한다.
     */
    onEvent?(name: string, callback: (payloadJson: string) => void): void;
    offEvent?(name: string): void;
    /** CallInvoker 없는 호스트의 수동 drain 폴백. */
    drainEvents?(): number;
};
export declare function createReactNativeEngine(native: {
    invoke(payload: ArrayBuffer): ArrayBuffer;
}): {
    invoke<T>(command: string, args?: unknown): Promise<T>;
};
/**
 * 동기 엔진 — JSI sync call로 Promise 오버헤드 없이 즉시 결과를 반환합니다.
 */
export type SyncEngineClient = {
    invoke<T>(command: string, args?: unknown): T;
};
/**
 * 고속 엔진 생성 옵션.
 *
 * rkyv V2 바이너리 경로를 필수로 사용합니다 (최고 성능).
 */
export type FastEngineOptions = {
    rkyvV2Codecs: Map<string, import('@rustra/types').RkyvV2Codec<any, any>>;
    /**
     * (F5, opt-in) 빌드 시점 계약 해시. 설정하면 엔진 생성 시 네이티브의
     * 실시간 해시(getContractHash)와 비교해 불일치 시 즉시 throw 한다.
     */
    contractHash?: string;
};
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
export declare function getRustraNative(): RustraNative;
export declare function createFastEngine(native: RustraJSINative, options: FastEngineOptions): EngineClientType;
/**
 * P0-3 async offload용 네이티브 인터페이스 확장 (선택 구현).
 *
 * 네이티브가 `invokeTypedAsync(name, args, callback)`을 노출하면 전용 worker
 * 큐(또는 dispatch_async)에서 Rust 를 호출한 뒤 JS 콜백 큐로 직렬화한다 —
 * 긴 Rust 연산이 JS 스레드를 블록하지 않는다 (jank 방지).
 *
 * 네이티브 구현이 없으면 `invokeAsync` 는 동기 `invokeTyped` 로 폴백한다
 * (기능은 동일, 스레드 오프로드 없음).
 */
export type RustraJSIAsyncNative = RustraJSINative & {
    /** 성공/에러 후 JS 콜백 큐에서 호출될 콜백 등록형 비동기 호출. */
    invokeTypedAsync?(name: string, args: unknown, onSuccess: (result: unknown) => void, onError: (message: string) => void): void;
};
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
export declare function createAsyncEngine(native: RustraJSIAsyncNative, options: FastEngineOptions): EngineClientType;
/**
 * 이벤트 푸시에 필요한 최소 네이티브 표면 — 구조적 타이핑으로 어떤 호스트
 * 객체도(`RustraNative`, `RustraJSINative`, 테스트 mock) 전달 가능하다.
 */
export type RustraEventNative = {
    onEvent?(name: string, callback: (payloadJson: string) => void): void;
    offEvent?(name: string): void;
};
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
export declare function subscribeEvent(native: RustraEventNative, name: string, cb: (payload: unknown) => void): () => void;
//# sourceMappingURL=index.d.ts.map