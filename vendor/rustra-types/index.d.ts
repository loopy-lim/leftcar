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
    invoke<T>(command: string, args?: unknown, options?: InvokeOptions): Promise<T>;
    /**
     * 코드젠이 이미 알고 있는 숫자 command id를 전달하는 빠른 경로.
     * `command`도 함께 받아 엔진이 id/name 정합성을 검증하고, 미지원 또는
     * 불일치 시 안전하게 이름 기반 invoke로 폴백할 수 있게 한다.
     */
    invokeById?<T>(commandId: number, command: string, args?: unknown, options?: InvokeOptions): Promise<T>;
    /**
     * 여러 명령을 한 번에 호출한다 (P0-2). 정적 명령만 있으면 단일 JSI/FFI 횡단
     * (invokeTypedBatch)로 처리하고, 동적 명령이 섞이면 항목별 invoke 로 폴백한다.
     */
    invokeBatch?<T>(entries: BatchEntry[]): Promise<T[]>;
};
/** invokeBatch 의 입력 항목. `options.signal` 은 항목 단위 취소로 전달된다. */
export type BatchEntry = {
    command: string;
    args?: unknown;
    options?: InvokeOptions;
};
/**
 * invoke 추가 옵션 (T1).
 *
 * `signal` 이 abort 되면 프라미스를 즉시 reject 한다. 네이티브가
 * `invokeAsync`/`invokeCancel` 을 노출하면 취소를 전파(전파는 JS 코덱
 * 경로만; typed/tier3 경로는 얕은 취소)하고, 그렇지 않으면 JS 프라미스만
 * 거부하는 얕은 취소로 폴백한다 — Rust 핸들러는 끝까지 실행된다.
 */
export type InvokeOptions = {
    /** (T1) AbortSignal — abort 시 Promise 를 즉시 reject 하고, 네이티브가
     *  invokeAsync/invokeCancel 을 노출하면 취소를 전파한다. */
    signal?: AbortSignal;
    /**
     * (프로덕션 준비) 호출별 타임아웃(ms). 만료 시 `transport.timeout`
     * (retryable)으로 reject 한다. 네이티브가 응답하지 않는 hang(워커 패닉,
     * FFI 데드락 등)의 유일한 JS 측 탈출구다. 지각 응답은 무시된다.
     */
    timeoutMs?: number;
};
/**
 * createRkyvV2Engine 이 반환하는 구체 엔진. EngineClient 에 더해 invokeBatch(P0-2) 를
 * 항상 지원한다 — 정적 전용이면 단일 횡단, 동적 혼합이면 항목별 라우팅.
 */
export type RkyvV2Engine = EngineClient & {
    invokeById<T>(commandId: number, command: string, args?: unknown, options?: InvokeOptions): Promise<T>;
    invokeBatch<T>(entries: BatchEntry[]): Promise<T[]>;
    /** 동적 registry 변경 뒤 엔진의 live-schema cache를 명시적으로 갱신한다. */
    refreshLiveSchema(): ReadonlyMap<string, LiveSchemaEntry>;
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
 * [`RustraCommandError`]로 파싱한다. JSON fallback 경로(네이티브 모듈)에서 사용 —
 * rkyv V2 경로(Node/Tauri)는 구조화된 `{code, message}` 객체를 받으므로 불필요.
 *
 * `": "` 앞이 dot-notation 코드 토큰(`command.not_found`, `internal`,
 * `math.divide_by_zero` 등 — 소문자/숫자/`.`/`_` 만)이면 code/message 를 분리하고,
 * 그렇지 않으면(FFI 수준 에러: `"json decode failed: ..."`, `"payload exceeds size limit"`
 * 등) `invoke.failed` 코드에 전체 문자열을 message 로 쓴다.
 */
export declare function parseRustraErrorString(error: string | undefined | null): RustraCommandError;
/**
 * rustra 에러 코드의 중앙 레지스트리 — Rust `RustraError`(crates/rustra/src/error.rs)
 * 와 JS 어댑터가 발행하는 전체 코드 집합. 과거엔 각 소스에 문자열 리터럴로
 * 흩어져 있어 `err.code === 'transport.timeout'` 오타가 컴파일 타임에 안 잡혔다.
 * 상수를 쓰면 자동완성+타입 체크가 둘 다 동작한다:
 *
 * ```ts
 * import { RustraErrorCode } from '@rustra/types';
 * if (err.code === RustraErrorCode.TransportTimeout) { retry(); }
 * ```
 *
 * 새 코드 추가 시 여기와 Rust error.rs 를 함께 갱신한다(단일 소스 관례).
 */
export declare const RustraErrorCode: {
    /** 명령을 레지스트리에서 찾을 수 없음. */
    readonly CommandNotFound: "command.not_found";
    /** 인자 역직렬화/검증 실패. */
    readonly CommandInvalidArgs: "command.invalid_args";
    /** capability 미부여로 거부됨 (deny-by-default). */
    readonly CapabilityDenied: "capability.denied";
    /** 페이로드가 크기 한도(기본 1MiB)를 초과. */
    readonly PayloadTooLarge: "payload.too_large";
    /** transport 계열 일시 오류 — retryable. */
    readonly TransportError: "transport.error";
    /** 자동 host 탐색에서 실행 가능한 native transport를 찾지 못함. */
    readonly TransportUnavailable: "transport.unavailable";
    /** 타임아웃 레이스 만료 — retryable. */
    readonly TransportTimeout: "transport.timeout";
    /** 사전/협력적 취소 — retryable. */
    readonly Cancelled: "cancelled";
    /** Rust 내부 오류(패닉 정규화 포함). */
    readonly Internal: "internal";
    /** 동결 레지스트리의 구조 mutation 거부. */
    readonly RegistryFrozen: "registry.frozen";
    /** command_id 공간 고갈. */
    readonly RegistryIdExhausted: "registry.id_exhausted";
    /** FFI 전역 패키지 미등록. */
    readonly FfiNotRegistered: "ffi.not_registered";
    /** invoke 일반 실패(JS 폴백 기본 코드). */
    readonly InvokeFailed: "invoke.failed";
    /** 와이어 프레임 파싱 실패. */
    readonly InvokeMalformed: "invoke.malformed";
    /** 페이로드가 헤더보다 짧음. */
    readonly InvokeTooShort: "invoke.too_short";
    /** 스키마 조회 실패. */
    readonly SchemaUnavailable: "schema.unavailable";
    /** 계약 해시 불일치(JS>native stale). */
    readonly ContractMismatch: "contract.mismatch";
    /** 계약 해시 검증 불가(네이티브 미지원). */
    readonly ContractUnenforceable: "contract.unenforceable";
    /** 분류 불가 오류. */
    readonly Unknown: "unknown";
};
export type RustraErrorCodeValue = (typeof RustraErrorCode)[keyof typeof RustraErrorCode];
/** 값이 알려진 rustra 에러 코드인지 검사 (타입 가드). */
export declare function isRustraErrorCode(code: string): code is RustraErrorCodeValue;
/**
 * rkyv V2 코덱 — 각 명령의 바이너리 인코딩/디코딩을 담당합니다.
 * 코드젠이 명령별로 자동 생성합니다.
 */
export type RkyvV2Codec<I, O> = {
    commandId: number;
    encode(args: I): ArrayBuffer;
    /**
     * (선택) 재사용 버퍼에 직접 인코딩한다. 대형 페이로드(≥64KiB)에서 매 호출
     * 신규 할당이 지배적이었다(실측: 1MiB 할당 ~42µs vs 재사용 memcpy 20µs).
     * 버퍼가 부족하면 내부적으로 정확한 크기로 재할당하고 그 버퍼를 반환한다 —
     * 호출자는 반환 subarray를 다음 호출에 그대로 재전달하면 된다. 미구현
     * 코덱(레거시)에서는 encode 와 동일한 새 ArrayBuffer 를 돌려준다.
     */
    encodeInto?(args: I, reuse?: Uint8Array): Uint8Array;
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
    /**
     * (P0-3) cmd_id 진입 typed fast path — `invokeTyped` 의 u16 디스패치 변형.
     * 문자열 마샬링과 C++ 이름 비교체인을 제거한다 (JSI 횡단 2→1, 문자열 2→0).
     * 미노출 구 네이티브는 이름 기반 `invokeTyped` 로 폴백한다.
     */
    invokeTypedById?(cmdId: number, args: unknown): unknown;
    /**
     * Generated command capability mask keyed by numeric command id.
     * bit 0 = typed, bit 1 = positional, bit 2 = raw scalar,
     * bit 3 = a single schema-proven byte buffer.
     */
    getCodecCapabilities?(cmdId: number): number;
    /** Tier 0: scalar fields and scalar/unit output without postcard conversion. */
    invokeTypedRaw?(cmdId: number, ...fields: unknown[]): unknown;
    /** Tier 1: one to three generated scalar/string fields without object reads. */
    invokeTypedPos?(cmdId: number, ...fields: unknown[]): unknown;
    /**
     * Tier 0.5: one schema-proven `Vec<u8>` field. Native code only borrows the
     * input for this synchronous call and returns a JS-owned result.
     */
    invokeTypedBuffer?(cmdId: number, value: Uint8Array | ArrayBuffer): unknown;
    /** P0-2: 정적 명령 N 개를 단일 횡단으로 일괄 처리 (RN JSI). */
    invokeTypedBatch?(names: string[], args: unknown[]): unknown[];
    /**
     * P0-2 byId 변형 — `invokeTypedBatch` 의 cmd_id 배열 진입. 배치 경로에서도
     * 문자열 마샬링 N 회를 제거한다. 미노출 구 네이티브는 이름 기반
     * `invokeTypedBatch` 로 폴백한다.
     */
    invokeTypedBatchById?(cmdIds: number[], args: unknown[]): unknown[];
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
    /** (T1) 진행 중 async 호출 취소 — invokeAsync 가 반환한 invocation id 를 넘긴다. */
    invokeCancel?(invocationId: number): boolean;
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
 * Registers a single lazy engine bootstrap. Generated commands can then be the
 * first Rustra API a user calls: concurrent first calls share one initializer,
 * while initialized hot paths retain the same direct engine branch.
 */
export declare function configureLazy(initializer: () => EngineClient | Promise<EngineClient>): void;
/** Resolves the configured engine, running a registered lazy bootstrap once. */
export declare function ensureConfigured(): Promise<EngineClient>;
/**
 * 코드젠이 생성한 명령 함수에서 실제 명령 이름을 추출한다.
 *
 * 코드젠 산출물은 함수에 `commandId` 문자열 프로퍼티를 심는다
 * (`addNumbers.commandId === 'addNumbers'`). minifier 가 함수 이름을 바꿔도
 * (esbuild/terser mangling) 이 프로퍼티는 문자열 리터럴이라 그대로 살아있어
 * `commandFn.name` 의존(`Function.prototype.name` — 프로덕션 번들에서 `a1` 로
 * 뭉개질 수 있음)보다 안전하다. 수동으로 만든 함수에는 `.name` 이 폴백으로 쓰인다.
 */
export declare function resolveCommandId(commandFn: (...args: never[]) => unknown): string;
/**
 * 글로벌 엔진으로 명령을 호출합니다.
 *
 * 일반적으로 직접 호출하지 않고, 코드젠이 생성한 명령 함수를 사용합니다.
 *
 * `options.signal` (T1) 이 abort 되면 엔진의 취소 정책(전파 가능하면
 * 네이티브 전파, 아니면 얕은 취소)에 따라 프라미스가 즉시 reject 됩니다.
 *
 * @example
 * ```ts
 * const result = await invoke<AddNumbersOutput>('addNumbers', { a: 42, b: 58 });
 * // 또는:
 * const result = await addNumbers({ a: 42, b: 58 });
 * // 취소 가능한 호출 (T1):
 * const ac = new AbortController();
 * const r = await invoke('addNumbers', { a: 42, b: 58 }, { signal: ac.signal });
 * ```
 */
export declare function invoke<T>(command: string, args?: unknown, options?: InvokeOptions): Promise<T>;
/**
 * 생성된 명령 클라이언트 전용 빠른 경로.
 *
 * 숫자 id를 지원하는 엔진은 문자열 Map 조회를 생략한다. 서드파티/구 엔진은
 * 기존 `invoke`로 폴백하므로 생성 코드의 이식성은 유지된다. 엔진은 id와 이름이
 * 현재 registry에서 일치할 때만 숫자 경로를 사용해야 한다.
 */
export declare function invokeGenerated<T>(commandId: number, command: string, args?: unknown, options?: InvokeOptions): Promise<T>;
/**
 * Generated-client helper for an input with exactly one schema-proven
 * `Vec<u8>` field. `number[]` remains supported through the regular generated
 * field route; only ArrayBuffer and one-byte typed views use the native buffer
 * entry point.
 */
export declare function invokeGeneratedBytes<T>(commandId: number, command: string, args: unknown, value: Uint8Array | ArrayBuffer | number[], options?: InvokeOptions): Promise<T>;
/** Generated-client helper for a schema-proven one-field input. */
export declare function invokeGeneratedFields1<T>(commandId: number, command: string, args: unknown, field0: unknown, options?: InvokeOptions): Promise<T>;
/** Generated-client helper for a schema-proven two-field input. */
export declare function invokeGeneratedFields2<T>(commandId: number, command: string, args: unknown, field0: unknown, field1: unknown, options?: InvokeOptions): Promise<T>;
/** A generated command with a stable, minifier-safe command identifier. */
export type GeneratedCommand<TInput, TOutput> = ((input: TInput, options?: InvokeOptions) => Promise<TOutput>) & {
    commandId: string;
};
/**
 * Creates a generated two-field command whose no-options hot path resolves the
 * native route once per configured engine. Timeout/cancellation options and
 * engines without a raw/positional route retain the established helper path.
 */
export declare function createGeneratedFields2<TInput extends object, TOutput>(commandId: number, command: string, field0Key: keyof TInput, field1Key: keyof TInput, functionName?: string): GeneratedCommand<TInput, TOutput>;
/** Generated-client helper for a schema-proven three-field input. */
export declare function invokeGeneratedFields3<T>(commandId: number, command: string, args: unknown, field0: unknown, field1: unknown, field2: unknown, options?: InvokeOptions): Promise<T>;
/**
 * 엔진 호출에 타임아웃 레이스를 건다. `options.timeoutMs` 가 없으면 엔진
 * 호출을 그대로 반환한다(오버헤드 0). 타임아웃은 settle 경쟁이며 지각 응답은
 * 무시된다 — 엔진이 나중에 reject 해도 unhandled rejection 이 되지 않도록
 * 뒤늦은 프라미스를 no-op catch 로 흡수한다.
 */
export declare function invokeWithTimeout<T>(engine: EngineClient, command: string, args?: unknown, options?: InvokeOptions): Promise<T>;
/**
 * 글로벌 엔진으로 여러 명령을 한 번에 호출합니다 (P0-2 invokeBatch).
 *
 * 정적 명령만 있으면 단일 네이티브 횡단으로 일괄 처리되어 잦은 호출의 jank 를 줄이고,
 * 동적 명령이 섞이면 항목별로 자동 라우팅됩니다.
 *
 * 항목별 취소 (T1 후속): 각 항목의 `options.signal` 이 항목 단위 invoke 로
 * 전달된다 — 해당 항목은 각자 전파(JS 코덱+invokeAsync+invokeCancel 충족 시)
 * 또는 얕은 취소로 동작한다. signal 있는 항목이 하나라도 섞이면 전체가
 * Promise.all 폴백으로 라우팅된다(단일 횡단 경로는 취소를 지원하지 않는다).
 * 항목별 `timeoutMs` 중 최솟값이 배치 전체의 타임아웃 레이스로 적용된다 —
 * 만료 시 `transport.timeout`(retryable) 으로 reject 한다(단일 invoke 의
 * `invokeWithTimeout` 과 동일 의미론, 지각 응답 흡수 포함).
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
    /**
     * (P0-3) cmd_id 진입 typed fast path — `invokeTyped` 의 id 인덱싱 변형.
     * 문자열 마샬링과 C++ 이름 비교체인을 u16 디스패치로 대체한다
     * (JSI 횡단 2→1, 문자열 2→0). 미노출 구 네이티브는 이름 기반
     * `invokeTyped` 로 폴백한다.
     */
    invokeTypedById?(cmdId: number, args: unknown): unknown;
    /** bit 0 = typed, bit 1 = positional, bit 2 = raw scalar, bit 3 = byte buffer. */
    getCodecCapabilities?(cmdId: number): number;
    /** Tier 0 scalar entry. Successful results retain the generated public shape. */
    invokeTypedRaw?(cmdId: number, ...fields: unknown[]): unknown;
    /** Tier 1 positional entry for one to three flat generated fields. */
    invokeTypedPos?(cmdId: number, ...fields: unknown[]): unknown;
    /** Synchronous single-`Vec<u8>` entry; input is borrowed only for the call. */
    invokeTypedBuffer?(cmdId: number, value: Uint8Array | ArrayBuffer): unknown;
    /** P0-2: 정적 명령 N 개를 단일 횡단으로 일괄 처리 (RN JSI). */
    invokeTypedBatch?(names: string[], args: unknown[]): unknown[];
    /**
     * P0-2 byId 변형 — `invokeTypedBatch` 의 cmd_id 배열 진입. 배치 경로에서도
     * 문자열 마샬링 N 회를 제거한다. 미노출 구 네이티브는 이름 기반
     * `invokeTypedBatch` 로 폴백한다.
     */
    invokeTypedBatchById?(cmdIds: number[], args: unknown[]): unknown[];
    /**
     * (T1) 취소 전파 가능한 비동기 invoke — invocation id 를 반환하고, 결과는 콜백으로.
     *
     * **호스트 구현 계약**: payload 는 `invokeRkyvV2` 와 동일한 rkyv V2 요청
     * 프레임(정적 postcard 또는 Tier 3 JSON-in-binary)이고, 응답도 동일한 응답
     * 프레임을 `onDone` 으로 전달한다. 반환된 invocation id 로 `invokeCancel` 을
     * 호출하면 Rust 측 취소 체크포인트까지 전파된다. 이 모듈(native adapter)을
     * 구현하는 호스트는 워커/비동기 스레드에서 invoke 를 실행하고, 취소 시
     * `cancelled` 에러 프레임을 콜백해야 한다.
     */
    invokeAsync?(payload: ArrayBuffer, onDone: (response: ArrayBuffer) => void): number;
    /** (T1) 진행 중 async 호출 취소 — `invokeAsync` 가 반환한 invocation id 를 넘긴다. */
    invokeCancel?(invocationId: number): boolean;
};
/**
 * 네이티브 getSchema() 로부터 현재 명령 스키마를 조회한다 (정적 + 동적 명령 포함).
 * 동적 명령의 commandId/타입을 알아내 rkyvV2 Tier 3 fallback 에 사용된다.
 * getSchema 미노출 네이티브에서는 schema.unavailable 에러를 던진다.
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
    /**
     * (T2, OTA) 계약 해시 불일치 시의 정책. 미설정 시 기존대로 throw
     * (fail-fast). 콜백을 설정하면 throw 대신 호출 후 **degraded 모드**로
     * 엔진을 계속 생성한다 — 구 JS + 신 네이티브(또는 그 반대) OTA 조합에서
     * 앱 전체 마비 대신 부분 동작을 택하는 배포 정책에 사용한다.
     * degraded 모드는 위험하다: 호환되지 않는 명령은 codec/tier3 디코딩에서
     * 실패할 수 있다. 콜백에서 live schema 를 조회해 공통 명령만 쓰도록
     * 안내하는 것은 호출자의 책임이다.
     *
     * `getContractHash` 미노출 네이티브는 검증 자체가 불가능하므로 이 콜백과
     * 무관하게 항상 `contract.unenforceable` 로 throw 한다 (native hash 가
     * 없으면 degraded 모드가 무의미하다).
     */
    onContractMismatch?: (info: {
        nativeHash: string;
        expectedHash: string;
    }) => void;
    /**
     * (T2, OTA) 빌드 시점 스키마 버전 — 코드젠이 생성한 SCHEMA_VERSION.
     * 설정하면 엔진 생성 시 live schema(getSchema)의 schemaVersion 과 비교해
     * JS > native 면 onSchemaStale 콜백(또는 console.warn)으로 경고한다.
     * 구 JS + 신 네이티브가 정상인 조합(신 기능은 못 쓰지만 기존 동작)과
     * 달리, JS > native 는 "네이티브가 구버전" — OTA 롤백/지연 배포 상황.
     * fatal 아님: 경고만 한다. 미설정 시 검증하지 않는다.
     *
     * 구 네이티브(pre-Task-8)는 schemaVersion 필드 없는 schema JSON 을,
     * 미등록 패키지는 `{}` 를 반환한다 — live schemaVersion 이 없으면 CLI 의
     * old-schema 관례대로 **1 로 취급**한다 (이 기능의 대상인 구 바이너리이며
     * 비교 불가(undefined→NaN) 로 스퓨리어스 경고하지 않게 막는다).
     */
    schemaVersion?: number;
    /** (T2) schemaVersion 검증 결과 JS > native 인 경우의 콜백. 미설정 시 console.warn. */
    onSchemaStale?: (info: {
        nativeVersion: number;
        jsVersion: number;
    }) => void;
    /**
     * (T3) 요청 페이로드 바이트 한도. 인코딩 직후 검사해 네이티브 왕복 전에
     * 조기 실패시킨다 — 네이티브 호출을 아끼고 에러에 컨텍스트(인코딩된 크기)
     * 를 싣는다. typed(C++ fast path) 경로는 JS 측 인코딩이 없어 검사를
     * 건너뛴다 — 네이티브 한도가 적용된다. 미설정 시 검사하지 않는다
     * (네이티브의 동적 한도가 최종 게이트). 값은 양의 정수여야 한다 —
     * 0/음수는 모든 페이로드를 거부한다 (전문가 노브, 클램핑 없음).
     */
    maxPayloadBytes?: number;
};
export declare function createRkyvV2Engine(native: RkyvV2SchemaNative, registry: Map<string, RkyvV2Codec<unknown, unknown>>, options?: RkyvV2EngineOptions): RkyvV2Engine;
//# sourceMappingURL=index.d.ts.map