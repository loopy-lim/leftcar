/**
 * @rustra/types — rustra 브릿지의 핵심 타입 및 글로벌 invoke
 *
 * 모든 플랫폼 어댑터(Node, Bun, Tauri, React Native)가 공유하는
 * EngineClient 인터페이스, 에러 타입, rkyv V2 코덱,
 * 그리고 Tauri-like 글로벌 invoke를 제공합니다.
 *
 * @example
 * ```ts
 * // 설정 (플랫폼별, 한 번만)
 * import { configure } from '@rustra/types';
 * import { createRkyvV2Engine } from '@rustra/react-native';
 * configure(createRkyvV2Engine(native, registry));
 *
 * // 사용 (어디서든, 타입 안전)
 * import { addNumbers } from './generated/commands.js';
 * const result = await addNumbers({ a: 42, b: 58 });
 * ```
 */
export type EngineClient = {
    invoke<T>(command: string, args?: unknown): Promise<T>;
    /**
     * 여러 명령을 한 번에 호출한다 (P0-2). 정적 명령만 있으면 단일 JSI/FFI 횡단
     * (invokeTypedBatch)로 처리하고, 동적 명령이 섞이면 항목별 invoke 로 폴백한다.
     */
    invokeBatch?<T>(entries: BatchEntry[]): Promise<T[]>;
};
/** invokeBatch 의 입력 항목. */
export type BatchEntry = {
    command: string;
    args?: unknown;
};
/**
 * createRkyvV2Engine 이 반환하는 구체 엔진. EngineClient 에 더해 invokeBatch(P0-2) 를
 * 항상 지원한다 — 정적 전용이면 단일 횡단, 동적 혼합이면 항목별 라우팅.
 */
export type RkyvV2Engine = EngineClient & {
    invokeBatch<T>(entries: BatchEntry[]): Promise<T[]>;
};
export type RustraError = {
    readonly code: string;
    readonly message: string;
    /** Rust `RustraError::retryable` — `transport.error`/`transport.timeout` 등에서 true */
    readonly retryable?: boolean;
};
export declare class RustraCommandError extends Error {
    readonly code: string;
    /** 재시도 가능한 에러인지 — Rust `RustraError::is_retryable` 와이어 값을 그대로 노출 */
    readonly retryable: boolean;
    constructor(code: string, message: string, retryable?: boolean);
}
/**
 * Rust `RustraError::Display` 포맷(`"{code}: {message}"`)의 평탄화된 문자열을
 * [`RustraCommandError`]로 파싱한다. JSON fallback 경로(RN/Lynx)에서 사용 —
 * rkyv V2 경로(Node/Tauri)는 구조화된 `{code, message}` 객체를 받으므로 불필요.
 *
 * `": "` 앞이 dot-notation 코드 토큰(`command.not_found`, `internal`,
 * `math.divide_by_zero` 등 — 소문자/숫자/`.`/`_` 만)이면 code/message 를 분리하고,
 * 그렇지 않으면(FFI 수준 에러: `"json decode failed: ..."`, `"payload exceeds size limit"`
 * 등) `invoke.failed` 코드에 전체 문자열을 message 로 쓴다.
 */
export declare function parseRustraErrorString(error: string | undefined | null): RustraCommandError;
/**
 * rkyv V2 코덱 — 각 명령의 바이너리 인코딩/디코딩을 담당합니다.
 * 코드젠이 명령별로 자동 생성합니다.
 */
export type RkyvV2Codec<I, O> = {
    commandId: number;
    encode(args: I): ArrayBuffer;
    decode(buf: ArrayBuffer): {
        ok: boolean;
        result?: O;
        error?: RustraError;
    };
};
/**
 * rkyv V2 네이티브 인터페이스 — 플랫폼별 FFI 브릿지가 구현합니다.
 */
export type RkyvV2Native = {
    invokeRkyvV2(payload: ArrayBuffer): ArrayBuffer;
};
/**
 * 통합 네이티브 인터페이스 — JSI/FFI 브릿지가 노출하는 모든 메서드.
 * 각 어댑터는 필요한 메서드만 사용합니다.
 */
export type RustraNative = {
    invoke(payload: ArrayBuffer): ArrayBuffer;
    invokeMsgpack(payload: ArrayBuffer): ArrayBuffer;
    invokeBincode(payload: ArrayBuffer): ArrayBuffer;
    invokePostcard(payload: ArrayBuffer): ArrayBuffer;
    invokeRkyv(payload: ArrayBuffer): ArrayBuffer;
    invokeHybrid(payload: ArrayBuffer): ArrayBuffer;
    invokeRkyvV2(payload: ArrayBuffer): ArrayBuffer;
    invokeRaw(payload: ArrayBuffer): ArrayBuffer;
    noop(payload: ArrayBuffer): ArrayBuffer;
    /** Live schema query (정적 + 동적 명령). JSI/FFI 가 노출하면 사용. */
    getSchema?(): ArrayBuffer;
    /** B1 (RN JSI): 정적 명령 C++ postcard fast path. JSI 가 노출하면 사용. */
    hasStaticCodec?(name: string): boolean;
    invokeTyped?(name: string, args: unknown): unknown;
    /** P0-2: 정적 명령 N 개를 단일 횡단으로 일괄 처리 (RN JSI). */
    invokeTypedBatch?(names: string[], args: unknown[]): unknown[];
    /**
     * Rust → JS 이벤트 푸시 리스너 등록(RN JSI). `payloadJson` 은 **JSON 문자열**로
     * 전달된다 — TS 래퍼(`@rustra/react-native` `subscribeEvent`)가
     * `JSON.parse` 1회로 객체로 복원한다. 등록 시점에 C++ 이 FFI 싱크를
     * 설치하고, 이후 Rust `emit` 은 CallInvoker 로 JS 스레드에 마샬링되어
     * 콜백을 호출한다.
     */
    onEvent?(name: string, callback: (payloadJson: string) => void): void;
    /** 등록된 이벤트 리스너 제거(RN JSI). 마지막 리스너 제거 시 폴링 경로 복귀. */
    offEvent?(name: string): void;
    /**
     * CallInvoker 없는 호스트의 JS 폴링 drain(RN JSI). 처리된 이벤트 수 반환.
     * CallInvoker 경로가 켜져 있으면 대개 호출 즉시 0(자동 drain 됨).
     */
    drainEvents?(): number;
};
/**
 * 글로벌 엔진을 설정합니다. 앱 시작 시 한 번만 호출합니다.
 *
 * @param engine - 플랫폼별로 생성한 EngineClient
 *
 * @example
 * ```ts
 * // React Native
 * import { configure } from '@rustra/types';
 * import { createRkyvV2Engine } from '@rustra/react-native';
 * configure(createRkyvV2Engine(native, rkyvV2Registry));
 *
 * // Node
 * import { configure } from '@rustra/types';
 * import { createRkyvV2Engine } from '@rustra/node';
 * configure(createRkyvV2Engine(nativeAddon, rkyvV2Registry));
 *
 * // Bun
 * import { configure } from '@rustra/types';
 * import { createRkyvV2Engine } from '@rustra/bun';
 * configure(createRkyvV2Engine(ffi, rkyvV2Registry));
 * ```
 */
export declare function configure(engine: EngineClient): void;
/**
 * 글로벌 엔진으로 명령을 호출합니다.
 *
 * 일반적으로 직접 호출하지 않고, 코드젠이 생성한 명령 함수를 사용합니다.
 *
 * @example
 * ```ts
 * const result = await invoke<AddNumbersOutput>('addNumbers', { a: 42, b: 58 });
 * // 또는:
 * const result = await addNumbers({ a: 42, b: 58 });
 * ```
 */
export declare function invoke<T>(command: string, args?: unknown): Promise<T>;
/**
 * 글로벌 엔진으로 여러 명령을 한 번에 호출합니다 (P0-2 invokeBatch).
 *
 * 정적 명령만 있으면 단일 네이티브 횡단으로 일괄 처리되어 잦은 호출의 jank 를 줄이고,
 * 동적 명령이 섞이면 항목별로 자동 라우팅됩니다.
 *
 * @example
 * ```ts
 * const [a, b] = await invokeBatch([
 *   { command: 'addNumbers', args: { a: 1, b: 2 } },
 *   { command: 'multiply', args: { a: 3, b: 4 } },
 * ]);
 * ```
 */
export declare function invokeBatch<T>(entries: BatchEntry[]): Promise<T[]>;
export type LiveSchemaEntry = {
    commandId: number;
    inputSchema?: unknown;
    outputSchema?: unknown;
};
/** createRkyvV2Engine 이 요구하는 네이티브 인터페이스 (invokeRkyvV2 + live schema). */
export type RkyvV2SchemaNative = {
    invokeRkyvV2(payload: ArrayBuffer): ArrayBuffer;
    getSchema?(): ArrayBuffer;
    /**
     * 네이티브 빌드의 계약 해시(SHA-256 hex)를 반환한다 (F5 opt-in 검증용).
     * `rustra_ffi_contract_hash` 와 대응. `contractHash` 엔진 옵션이 설정된
     * 경우에만 호출된다.
     */
    getContractHash?(): ArrayBuffer;
    /** B1 (RN JSI): 정적 명령 C++ postcard fast path. 둘 다 있으면 JS 코덱 대신 사용. */
    hasStaticCodec?(name: string): boolean;
    invokeTyped?(name: string, args: unknown): unknown;
    /** P0-2: 정적 명령 N 개를 단일 횡단으로 일괄 처리 (RN JSI). */
    invokeTypedBatch?(names: string[], args: unknown[]): unknown[];
};
/**
 * 네이티브 getSchema() 로부터 현재 명령 스키마를 조회한다 (정적 + 동적 명령 포함).
 * 동적 명령의 commandId/타입을 알아내 rkyvV2 Tier 3 fallback 에 사용된다.
 */
export declare function getLiveSchema(native: {
    getSchema?(): ArrayBuffer;
}): Map<string, LiveSchemaEntry>;
/**
 * rkyv V2 네이티브 모듈로 EngineClient을 생성한다.
 *
 * 정적 명령은 codegen codec registry 로 fast-path(postcard). registry 에 없는
 * 동적(런타임 등록) 명령은 live schema 에서 commandId 를 조회해 Tier 3(JSON) 로
 * fallback 한다. 단일 엔진이 정적 + 동적 모두 처리.
 */
/**
 * `createRkyvV2Engine` 옵션. 모두 opt-in 이며 생략 시 하위 호환 동작을 유지한다.
 */
export type RkyvV2EngineOptions = {
    /**
     * (F5) 빌드 시점 코드젠이 생성한 계약 해시(`GENERATED_CONTRACT_HASH`).
     * 설정하면 엔진 생성 시 네이티브의 실시간 해시(`getContractHash`)와 비교해
     * 불일치면 즉시 throw 한다 — 생성된 클라이언트와 네이티브 바이너리의 스키마
     * 드리프트를 시작 시점에 잡는다. 미설정 시 검증하지 않는다(기본값).
     */
    contractHash?: string;
};
export declare function createRkyvV2Engine(native: RkyvV2SchemaNative, registry: Map<string, RkyvV2Codec<any, any>>, options?: RkyvV2EngineOptions): RkyvV2Engine;
//# sourceMappingURL=index.d.ts.map