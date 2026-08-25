/**
 * RN용 rustra 엔진 어댑터
 *
 * 글로벌 invoke + RN JSI 전용 엔진을 제공합니다.
 * 설정은 `@rustra/types`의 configure()를 사용합니다.
 */
import type { EngineClient as EngineClientType, InvokeOptions, RkyvV2EngineOptions, RkyvV2SchemaNative, RustraNative } from '@rustra/types';
export type { EngineClient, InvokeOptions, RustraError, RkyvV2Codec, RkyvV2Native, RkyvV2SchemaNative, } from '@rustra/types';
export { RustraCommandError, configure, invoke, createRkyvV2Engine, parseRustraErrorString, } from '@rustra/types';
/**
 * RN JSI 네이티브 표면 — 코어 `RkyvV2SchemaNative` 를 그대로 상속하고 RN 전용
 * 메서드만 추가 선언한다. 과거엔 동일 메서드를 3곳(types의 RkyvV2SchemaNative,
 * RustraNative, 여기)에서 수동 미러링해 신규 옵션마다 세 곳을 동기화했었다 —
 * 이제 단일 소스(types)를 재사용한다.
 */
export type RustraJSINative = RkyvV2SchemaNative & {
    invoke(payload: ArrayBuffer): ArrayBuffer;
    /**
     * Rust → JS 이벤트 푸시(RN JSI EventDispatcher). 콜백 인자는 JSON 문자열 —
     * `subscribeEvent` 래퍼가 파싱한다.
     */
    onEvent?(name: string, callback: (payloadJson: string) => void): void;
    offEvent?(name: string): void;
    /** CallInvoker 없는 호스트의 수동 drain 폴백. */
    drainEvents?(): number;
    /** Rust가 JS로 값을 보낼 수 있는 일회성 채널 핸들을 만든다. */
    createChannel?(callback: (payloadJson: string) => void): number;
    /** 앞서 만든 채널을 해제한다. 이미 해제된 핸들은 false를 반환한다. */
    dropChannel?(handle: number): boolean;
};
export declare function createReactNativeEngine(native: {
    invoke(payload: ArrayBuffer): ArrayBuffer;
}): {
    invoke<T>(command: string, args?: unknown, options?: InvokeOptions): Promise<T>;
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
 * rkyv V2 바이너리 경로를 필수로 사용합니다 (최고 성능). 나머지 필드는 core
 * `RkyvV2EngineOptions` 를 그대로 전달한다 — contractHash 검증(F5),
 * onContractMismatch/schemaVersion/onSchemaStale(OTA, T2), maxPayloadBytes(T3).
 */
export type FastEngineOptions = {
    rkyvV2Codecs: Map<string, import('@rustra/types').RkyvV2Codec<unknown, unknown>>;
} & RkyvV2EngineOptions;
export type RustraBootstrapOptions = FastEngineOptions & {
    /** Installs the platform JSI module on the live JavaScript Runtime thread. */
    install(): Promise<void>;
    /** Returns the native surface after install completes. */
    getNative(): RustraJSINative;
};
export type RustraBootstrap = {
    /** Optional eager readiness hook; generated commands also initialize lazily. */
    ready(): Promise<EngineClientType>;
};
/**
 * Registers one concurrency-safe lazy React Native bootstrap. A generated
 * platform entry calls this at module import, so the first generated command
 * installs JSI, verifies the Rust contract, and configures the fast engine.
 */
export declare function createRustraBootstrap(options: RustraBootstrapOptions): RustraBootstrap;
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
export declare function getRustraNative(): RustraJSINative & RustraNative;
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
    /**
     * 성공/에러 후 JS 콜백 큐에서 호출될 콜백 등록형 비동기 호출.
     *
     * 반환값: invocation id (취소 핸들, follow-up 3). 네이티브가 id 를
     * 노출하면 `invokeCancel(id)` 로 Rust 취소 체크포인트까지 전파된다.
     * 구형 네이티브가 undefined 를 반환하면 어댑터가 얕은 취소로 폴백한다.
     */
    invokeTypedAsync?(name: string, args: unknown, onSuccess: (result: unknown) => void, onError: (message: string) => void): number | void;
    /** 진행 중 async 호출 취소 — invokeTypedAsync 가 반환한 id. */
    invokeCancel?(invocationId: number): boolean;
};
/**
 * 비동기 invoke — 무거운 Rust 연산을 JS 스레드에서 오프로드한다.
 *
 * - 네이티브 `invokeTypedAsync` 가 있으면: 즉시 반환( invocation id 포함),
 *   결과는 JS 콜백 큐로 전달.
 * - 없으면: 동기 fast path(`createFastEngine`)로 폴백 — 마이크로태스크로 래핑해
 *   API 계약(`Promise<T>`)은 항상 동일하게 유지.
 * - `options.signal` (T1): abort 시 `cancelled` 로 즉시 거부. 네이티브가
 *   `invokeCancel` 을 노출하면 id 로 **취소 전파**(Rust 체크포인트까지),
 *   아니면 얕은 취소(JS 프라미스만 거부, Rust 핸들러는 끝까지 실행)로
 *   폴백한다. 폴백(동기 엔진) 경로는 기존 T1 배선을 따른다.
 *
 * @example
 * ```ts
 * import { createAsyncEngine } from '@rustra/react-native';
 * const engine = createAsyncEngine(getRustraNative(), { rkyvV2Codecs: registry });
 * const result = await engine.invoke('heavyCompute', { n: 1_000_000 });
 * // 취소 (T1):
 * const ac = new AbortController();
 * engine.invoke('heavyCompute', { n: 1 }, { signal: ac.signal });
 * ac.abort();
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
export declare function subscribeEvent(native: RustraEventNative, name: string, cb: (payload: unknown) => void, options?: {
    allowMissingNative?: boolean;
}): () => void;
//# sourceMappingURL=index.d.ts.map