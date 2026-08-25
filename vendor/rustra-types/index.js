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
export class RustraCommandError extends Error {
    code;
    /** 재시도 가능한 에러인지 — Rust `RustraError::is_retryable` 와이어 값을 그대로 노출 */
    retryable;
    constructor(code, message, retryable = false) {
        super(message);
        this.name = 'RustraCommandError';
        this.code = code;
        this.retryable = retryable;
    }
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
export function parseRustraErrorString(error) {
    const raw = error ?? 'Rustra invoke failed';
    if (raw.startsWith('{') && raw.endsWith('}')) {
        try {
            const parsed = JSON.parse(raw);
            if (typeof parsed.code === 'string' && typeof parsed.message === 'string') {
                const retryable = typeof parsed.retryable === 'boolean' ? parsed.retryable : isRetryableCode(parsed.code);
                return new RustraCommandError(parsed.code, parsed.message, retryable);
            }
        }
        catch {
            // Fall through to plain text splitting
        }
    }
    const idx = raw.indexOf(': ');
    if (idx > 0) {
        const code = raw.slice(0, idx);
        if (/^[a-z][a-z0-9_.]*$/.test(code)) {
            return new RustraCommandError(code, raw.slice(idx + 2), isRetryableCode(code));
        }
    }
    return new RustraCommandError('invoke.failed', raw);
}
/**
 * 코드 기반 retryable 추론 — Rust `RustraError` 팩토리 관례와 정합.
 * `transport.error`/`transport.timeout`은 Rust 생성 시점에 `retryable: true`로
 * 설정되는 코드군이며 (구조화 와이어에는 retryable 플래그가 없으므로 코드에서
 * 도출한다), `cancelled`도 Rust `RustraError::cancelled` 의 retryable:true 를
 * 미러링한다 (T1 — JSON fallback 경로의 취소 에러 정합).
 */
function isRetryableCode(code) {
    return code === 'transport.error' || code === 'transport.timeout' || code === 'cancelled';
}
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
export const RustraErrorCode = {
    /** 명령을 레지스트리에서 찾을 수 없음. */
    CommandNotFound: 'command.not_found',
    /** 인자 역직렬화/검증 실패. */
    CommandInvalidArgs: 'command.invalid_args',
    /** capability 미부여로 거부됨 (deny-by-default). */
    CapabilityDenied: 'capability.denied',
    /** 페이로드가 크기 한도(기본 1MiB)를 초과. */
    PayloadTooLarge: 'payload.too_large',
    /** transport 계열 일시 오류 — retryable. */
    TransportError: 'transport.error',
    /** 자동 host 탐색에서 실행 가능한 native transport를 찾지 못함. */
    TransportUnavailable: 'transport.unavailable',
    /** 타임아웃 레이스 만료 — retryable. */
    TransportTimeout: 'transport.timeout',
    /** 사전/협력적 취소 — retryable. */
    Cancelled: 'cancelled',
    /** Rust 내부 오류(패닉 정규화 포함). */
    Internal: 'internal',
    /** 동결 레지스트리의 구조 mutation 거부. */
    RegistryFrozen: 'registry.frozen',
    /** command_id 공간 고갈. */
    RegistryIdExhausted: 'registry.id_exhausted',
    /** FFI 전역 패키지 미등록. */
    FfiNotRegistered: 'ffi.not_registered',
    /** invoke 일반 실패(JS 폴백 기본 코드). */
    InvokeFailed: 'invoke.failed',
    /** 와이어 프레임 파싱 실패. */
    InvokeMalformed: 'invoke.malformed',
    /** 페이로드가 헤더보다 짧음. */
    InvokeTooShort: 'invoke.too_short',
    /** 스키마 조회 실패. */
    SchemaUnavailable: 'schema.unavailable',
    /** 계약 해시 불일치(JS>native stale). */
    ContractMismatch: 'contract.mismatch',
    /** 계약 해시 검증 불가(네이티브 미지원). */
    ContractUnenforceable: 'contract.unenforceable',
    /** 분류 불가 오류. */
    Unknown: 'unknown',
};
/** 값이 알려진 rustra 에러 코드인지 검사 (타입 가드). */
export function isRustraErrorCode(code) {
    return Object.values(RustraErrorCode).includes(code);
}
// ── Global invoke (Tauri-like) ──────────────────────────────
// A Metro bundle can contain more than one physical copy of @rustra/types
// (for example when generated code lives outside the app workspace). Use a
// versioned global protocol so those copies share configuration and private
// fast-path symbols without colliding with incompatible package versions.
const invokeByIdSync = Symbol.for('dev.rustra.types.v0.4.0.invokeByIdSync');
const invokeGeneratedFieldsSync = Symbol.for('dev.rustra.types.v0.4.0.invokeGeneratedFieldsSync');
const resolveGeneratedFieldsSync = Symbol.for('dev.rustra.types.v0.4.0.resolveGeneratedFieldsSync');
const invokeGeneratedBytesSync = Symbol.for('dev.rustra.types.v0.4.0.invokeGeneratedBytesSync');
const resolveGeneratedBytesSync = Symbol.for('dev.rustra.types.v0.4.0.resolveGeneratedBytesSync');
const CODEC_TYPED = 1 << 0;
const CODEC_POSITIONAL = 1 << 1;
const CODEC_RAW = 1 << 2;
const CODEC_BUFFER = 1 << 3;
function isNativeByteBuffer(value) {
    if (typeof ArrayBuffer === 'undefined' || typeof value !== 'object' || value === null) {
        return false;
    }
    if (value instanceof ArrayBuffer)
        return true;
    // `ArrayBuffer.isView` works across realms. Restrict views to one-byte
    // elements so Int16Array/DataView cannot silently change the byte contract.
    return (ArrayBuffer.isView(value) && value.BYTES_PER_ELEMENT === 1);
}
const RUSTRA_RUNTIME_STATE = Symbol.for('dev.rustra.types.v0.4.0.runtimeState');
const runtimeGlobal = globalThis;
const existingRuntime = runtimeGlobal[RUSTRA_RUNTIME_STATE];
const runtime = existingRuntime ?? {
    engine: null,
    engineGeneration: 0,
    generatedFieldsRoutes: [],
    generatedBytesRoutes: [],
};
if (!existingRuntime)
    runtimeGlobal[RUSTRA_RUNTIME_STATE] = runtime;
function resetConfiguredRoutes() {
    runtime.engineGeneration += 1;
    runtime.generatedFieldsRoutes = [];
    runtime.generatedBytesRoutes = [];
}
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
export function configure(engine) {
    runtime.engine = engine;
    runtime.engineInitializer = undefined;
    runtime.engineInitialization = undefined;
    resetConfiguredRoutes();
}
/**
 * Registers a single lazy engine bootstrap. Generated commands can then be the
 * first Rustra API a user calls: concurrent first calls share one initializer,
 * while initialized hot paths retain the same direct engine branch.
 */
export function configureLazy(initializer) {
    runtime.engine = null;
    runtime.engineInitializer = initializer;
    runtime.engineInitialization = undefined;
    resetConfiguredRoutes();
}
/** Resolves the configured engine, running a registered lazy bootstrap once. */
export function ensureConfigured() {
    if (runtime.engine)
        return Promise.resolve(runtime.engine);
    if (!runtime.engineInitializer) {
        return Promise.reject(new Error('Rustra not configured. Call configure(engine), or import the generated React Native entry that registers lazy setup.'));
    }
    if (!runtime.engineInitialization) {
        const initializer = runtime.engineInitializer;
        const generation = runtime.engineGeneration;
        const initialization = Promise.resolve()
            .then(initializer)
            .then((engine) => {
            // Explicit configure/configureLazy during an in-flight bootstrap wins;
            // a late native installer must not replace the user's newer engine.
            if (runtime.engineGeneration !== generation || runtime.engineInitializer !== initializer) {
                return runtime.engine ?? ensureConfigured();
            }
            configure(engine);
            return engine;
        })
            .catch((error) => {
            if (runtime.engineInitialization === initialization) {
                runtime.engineInitialization = undefined;
            }
            throw error;
        });
        runtime.engineInitialization = initialization;
    }
    return runtime.engineInitialization;
}
function hasLazyInitializer() {
    return runtime.engineInitializer !== undefined;
}
/**
 * 코드젠이 생성한 명령 함수에서 실제 명령 이름을 추출한다.
 *
 * 코드젠 산출물은 함수에 `commandId` 문자열 프로퍼티를 심는다
 * (`addNumbers.commandId === 'addNumbers'`). minifier 가 함수 이름을 바꿔도
 * (esbuild/terser mangling) 이 프로퍼티는 문자열 리터럴이라 그대로 살아있어
 * `commandFn.name` 의존(`Function.prototype.name` — 프로덕션 번들에서 `a1` 로
 * 뭉개질 수 있음)보다 안전하다. 수동으로 만든 함수에는 `.name` 이 폴백으로 쓰인다.
 */
export function resolveCommandId(commandFn) {
    const withId = commandFn;
    if (typeof withId.commandId === 'string' && withId.commandId.length > 0) {
        return withId.commandId;
    }
    if (typeof commandFn.name === 'string' && commandFn.name.length > 0) {
        return commandFn.name;
    }
    throw new Error('Command function must have a commandId or name property (use generated commands or pass a named function)');
}
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
export function invoke(command, args, options) {
    const engine = runtime.engine;
    if (!engine) {
        if (hasLazyInitializer()) {
            return ensureConfigured().then(() => invoke(command, args, options));
        }
        throw new Error('Rustra not configured. Call configure(engine) first.');
    }
    // 옵션을 엔진에 그대로 전달한다 (T1). 옵션을 이해하지 못하는 구형/서드파티
    // 엔진은 JS 호출 규약상 추가 인자를 무시한다 — 호출부 파괴 없이 확장된다.
    // timeoutMs 는 글로벌 레이스(invokeWithTimeout)로 여기서 소비한다.
    return invokeWithTimeout(engine, command, args, options);
}
/**
 * 생성된 명령 클라이언트 전용 빠른 경로.
 *
 * 숫자 id를 지원하는 엔진은 문자열 Map 조회를 생략한다. 서드파티/구 엔진은
 * 기존 `invoke`로 폴백하므로 생성 코드의 이식성은 유지된다. 엔진은 id와 이름이
 * 현재 registry에서 일치할 때만 숫자 경로를 사용해야 한다.
 */
export function invokeGenerated(commandId, command, args, options) {
    const engine = runtime.engine;
    if (!engine) {
        if (hasLazyInitializer()) {
            return ensureConfigured().then(() => invokeGenerated(commandId, command, args, options));
        }
        throw new Error('Rustra not configured. Call configure(engine) first.');
    }
    // createRkyvV2Engine의 동기 transport는 여기서 바로 한 번만 Promise로
    // 승격한다. 공개 EngineClient 계약과 옵션 의미는 그대로 두면서
    // invokeByIdWithTimeout → engine.invokeById → dispatchPromiseById가
    // 같은 Promise를 반복 정규화하던 hot-path 래퍼를 건너뛴다.
    const syncInvoke = engine[invokeByIdSync];
    if (options === undefined && syncInvoke) {
        try {
            return Promise.resolve(syncInvoke(commandId, command, args));
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    if (!engine.invokeById) {
        return invokeWithTimeout(engine, command, args, options);
    }
    return invokeByIdWithTimeout(engine, commandId, command, args, options);
}
function resolveCachedGeneratedFieldsRoute(engine, commandId, command, fieldCount) {
    const invoke = engine[resolveGeneratedFieldsSync]?.(commandId, command, fieldCount);
    const cached = invoke ? { command, fieldCount, invoke } : null;
    runtime.generatedFieldsRoutes[commandId] = cached;
    return cached;
}
function resolveCachedGeneratedBytesRoute(engine, commandId, command) {
    const invoke = engine[resolveGeneratedBytesSync]?.(commandId, command);
    const cached = invoke ? { command, invoke } : null;
    runtime.generatedBytesRoutes[commandId] = cached;
    return cached;
}
/**
 * Generated-client helper for an input with exactly one schema-proven
 * `Vec<u8>` field. `number[]` remains supported through the regular generated
 * field route; only ArrayBuffer and one-byte typed views use the native buffer
 * entry point.
 */
export function invokeGeneratedBytes(commandId, command, args, value, options) {
    const engine = runtime.engine;
    if (!engine) {
        if (hasLazyInitializer()) {
            return ensureConfigured().then(() => invokeGeneratedBytes(commandId, command, args, value, options));
        }
        throw new Error('Rustra not configured. Call configure(engine) first.');
    }
    if (options === undefined) {
        let route = runtime.generatedBytesRoutes[commandId];
        if (route === undefined) {
            route = resolveCachedGeneratedBytesRoute(engine, commandId, command);
        }
        if (route && route.command === command) {
            try {
                return Promise.resolve(route.invoke(args, value));
            }
            catch (error) {
                return Promise.reject(error);
            }
        }
        const syncInvoke = engine[invokeGeneratedBytesSync];
        if (syncInvoke) {
            try {
                return Promise.resolve(syncInvoke(commandId, command, args, value));
            }
            catch (error) {
                return Promise.reject(error);
            }
        }
    }
    return invokeGeneratedFields1(commandId, command, args, value, options);
}
/** Generated-client helper for a schema-proven one-field input. */
export function invokeGeneratedFields1(commandId, command, args, field0, options) {
    const engine = runtime.engine;
    if (!engine) {
        if (hasLazyInitializer()) {
            return ensureConfigured().then(() => invokeGeneratedFields1(commandId, command, args, field0, options));
        }
        throw new Error('Rustra not configured. Call configure(engine) first.');
    }
    if (options === undefined) {
        let route = runtime.generatedFieldsRoutes[commandId];
        if (route === undefined) {
            route = resolveCachedGeneratedFieldsRoute(engine, commandId, command, 1);
        }
        if (route && route.command === command && route.fieldCount === 1) {
            try {
                return Promise.resolve(route.invoke(args, field0));
            }
            catch (error) {
                return Promise.reject(error);
            }
        }
    }
    const syncInvoke = engine[invokeGeneratedFieldsSync];
    if (options === undefined && syncInvoke) {
        try {
            return Promise.resolve(syncInvoke(commandId, command, args, 1, field0, undefined, undefined));
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    return invokeGenerated(commandId, command, args, options);
}
/** Generated-client helper for a schema-proven two-field input. */
export function invokeGeneratedFields2(commandId, command, args, field0, field1, options) {
    const engine = runtime.engine;
    if (!engine) {
        if (hasLazyInitializer()) {
            return ensureConfigured().then(() => invokeGeneratedFields2(commandId, command, args, field0, field1, options));
        }
        throw new Error('Rustra not configured. Call configure(engine) first.');
    }
    if (options === undefined) {
        let route = runtime.generatedFieldsRoutes[commandId];
        if (route === undefined) {
            route = resolveCachedGeneratedFieldsRoute(engine, commandId, command, 2);
        }
        if (route && route.command === command && route.fieldCount === 2) {
            try {
                return Promise.resolve(route.invoke(args, field0, field1));
            }
            catch (error) {
                return Promise.reject(error);
            }
        }
    }
    const syncInvoke = engine[invokeGeneratedFieldsSync];
    if (options === undefined && syncInvoke) {
        try {
            return Promise.resolve(syncInvoke(commandId, command, args, 2, field0, field1, undefined));
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    return invokeGenerated(commandId, command, args, options);
}
/**
 * Creates a generated two-field command whose no-options hot path resolves the
 * native route once per configured engine. Timeout/cancellation options and
 * engines without a raw/positional route retain the established helper path.
 */
export function createGeneratedFields2(commandId, command, field0Key, field1Key, functionName = command) {
    let routeGeneration = -1;
    let route = null;
    const generated = ((input, options) => {
        const engine = runtime.engine;
        const field0 = input[field0Key];
        const field1 = input[field1Key];
        if (!engine) {
            return invokeGeneratedFields2(commandId, command, input, field0, field1, options);
        }
        if (options !== undefined) {
            return invokeGeneratedFields2(commandId, command, input, field0, field1, options);
        }
        if (routeGeneration !== runtime.engineGeneration) {
            route = engine[resolveGeneratedFieldsSync]?.(commandId, command, 2) ?? null;
            routeGeneration = runtime.engineGeneration;
        }
        if (route) {
            try {
                return Promise.resolve(route(input, field0, field1));
            }
            catch (error) {
                return Promise.reject(error);
            }
        }
        return invokeGeneratedFields2(commandId, command, input, field0, field1);
    });
    Object.defineProperty(generated, 'name', { configurable: true, value: functionName });
    generated.commandId = command;
    return generated;
}
/** Generated-client helper for a schema-proven three-field input. */
export function invokeGeneratedFields3(commandId, command, args, field0, field1, field2, options) {
    const engine = runtime.engine;
    if (!engine) {
        if (hasLazyInitializer()) {
            return ensureConfigured().then(() => invokeGeneratedFields3(commandId, command, args, field0, field1, field2, options));
        }
        throw new Error('Rustra not configured. Call configure(engine) first.');
    }
    if (options === undefined) {
        let route = runtime.generatedFieldsRoutes[commandId];
        if (route === undefined) {
            route = resolveCachedGeneratedFieldsRoute(engine, commandId, command, 3);
        }
        if (route && route.command === command && route.fieldCount === 3) {
            try {
                return Promise.resolve(route.invoke(args, field0, field1, field2));
            }
            catch (error) {
                return Promise.reject(error);
            }
        }
    }
    const syncInvoke = engine[invokeGeneratedFieldsSync];
    if (options === undefined && syncInvoke) {
        try {
            return Promise.resolve(syncInvoke(commandId, command, args, 3, field0, field1, field2));
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    return invokeGenerated(commandId, command, args, options);
}
/**
 * 엔진 호출에 타임아웃 레이스를 건다. `options.timeoutMs` 가 없으면 엔진
 * 호출을 그대로 반환한다(오버헤드 0). 타임아웃은 settle 경쟁이며 지각 응답은
 * 무시된다 — 엔진이 나중에 reject 해도 unhandled rejection 이 되지 않도록
 * 뒤늦은 프라미스를 no-op catch 로 흡수한다.
 */
export function invokeWithTimeout(engine, command, args, options) {
    let p;
    try {
        // Promise.resolve(existingPromise)는 동일 객체를 반환한다. timeout이 없는
        // hot path에서 async 함수가 만들던 추가 Promise/microtask 층을 없애면서,
        // 잘못 구현된 서드파티 엔진의 동기 값도 기존처럼 Promise로 정규화한다.
        p = Promise.resolve(engine.invoke(command, args, options));
    }
    catch (error) {
        // 기존 async 함수 계약: 엔진의 동기 throw도 호출부에는 rejected Promise로 보인다.
        return Promise.reject(error);
    }
    const ms = options?.timeoutMs;
    if (ms === undefined)
        return p;
    // 원본 프라미스의 지각 reject 흡수 — race 에서 진 뒤에도 reject 되면
    // unhandled rejection 이 되므로 no-op catch 로 처리 표시만 남긴다.
    void p.catch(() => { });
    let timer;
    return Promise.race([
        p,
        new Promise((_, reject) => {
            timer = setTimeout(() => {
                reject(new RustraCommandError('transport.timeout', `invoke("${command}") timed out after ${ms}ms`, true));
            }, ms);
        }),
    ]).finally(() => {
        if (timer !== undefined)
            clearTimeout(timer);
    });
}
/** invokeGenerated의 id 경로에 invoke와 동일한 timeout/throw 계약을 적용한다. */
function invokeByIdWithTimeout(engine, commandId, command, args, options) {
    let p;
    try {
        p = Promise.resolve(engine.invokeById(commandId, command, args, options));
    }
    catch (error) {
        return Promise.reject(error);
    }
    const ms = options?.timeoutMs;
    if (ms === undefined)
        return p;
    void p.catch(() => { });
    let timer;
    return Promise.race([
        p,
        new Promise((_, reject) => {
            timer = setTimeout(() => {
                reject(new RustraCommandError('transport.timeout', `invoke("${command}") timed out after ${ms}ms`, true));
            }, ms);
        }),
    ]).finally(() => {
        if (timer !== undefined)
            clearTimeout(timer);
    });
}
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
export function invokeBatch(entries) {
    const engine = runtime.engine;
    if (!engine) {
        if (hasLazyInitializer()) {
            return ensureConfigured().then(() => invokeBatch(entries));
        }
        throw new Error('Rustra not configured. Call configure(engine) first.');
    }
    if (!engine.invokeBatch) {
        throw new Error('Configured engine does not support invokeBatch.');
    }
    // 배치 타임아웃 — 항목 timeoutMs 의 최솟값으로 배치 전체에 레이스를 건다.
    // 배치는 단일 프라미스로 settle 되므로 항목별 레이스보다 최솟값이 정확하다.
    const batchTimeout = entries.reduce((min, entry) => {
        const ms = entry.options?.timeoutMs;
        if (ms === undefined)
            return min;
        return min === undefined || ms < min ? ms : min;
    }, undefined);
    if (batchTimeout === undefined) {
        return engine.invokeBatch(entries);
    }
    const stripped = entries.map((entry) => entry.options?.timeoutMs === undefined
        ? entry
        : { ...entry, options: { ...entry.options, timeoutMs: undefined } });
    const p = Promise.resolve(engine.invokeBatch(stripped));
    // 지각 reject 흡수 — invokeWithTimeout 과 동일 계약.
    void p.catch(() => { });
    let timer;
    return Promise.race([
        p,
        new Promise((_, reject) => {
            timer = setTimeout(() => {
                reject(new RustraCommandError('transport.timeout', `invokeBatch(${entries.length} entries) timed out after ${batchTimeout}ms`, true));
            }, batchTimeout);
        }),
    ]).finally(() => {
        if (timer !== undefined)
            clearTimeout(timer);
    });
}
// ── Runtime-safe UTF-8 helpers ─────────────────────────────
// 임베디드 JS 런타임(예: Hermes)에는 TextEncoder/TextDecoder 글로벌이 없을 수
// 있으므로 엔진은 이에 의존하지 않는다. Pure-JS UTF-8 코덱 (surrogate-pair 정확).
//
// 폴백 계층: TextEncoder 가 있으면 네이티브 구현을 쓰고(대형 문자열에서 수 배
// 빠름), 없을 때만 사전 크기 추정 Writer 로 pure-JS 폴백을 돌린다. 과거 폴백은
// number[] 에 push 후 마지막에 Uint8Array 로 재복사해 할당이 2배였다.
const _hasTextEncoder = typeof TextEncoder !== 'undefined';
const _textEncoder = _hasTextEncoder ? new TextEncoder() : undefined;
function _utf8Encode(s) {
    if (_textEncoder) {
        return _textEncoder.encode(s);
    }
    // 폴백: ASCII 가 많은 실제 페이로드에서 s.length 상한으로 1회 할당하고
    // 커서를 옮겨 쓴다. 멀티바이트 확장이 커서를 넘기면 1회 재할당한다(희박).
    // 빈 문자열도 성장 시 0 * 2가 계속 0이 되지 않도록 최소 1바이트에서 시작한다.
    // `out` 자체를 교체해야 한다. 이전 구현은 최초 버퍼를 닫아 둔 `ensure`가
    // 성장 후에도 계속 원본 길이/내용을 참조해 두 번째 성장부터 앞부분을 0으로
    // 덮어썼다(Hermes에서 긴 한글/이모지 payload 손상).
    let out = new Uint8Array(Math.max(1, s.length));
    let cursor = 0;
    const ensure = (needed) => {
        if (cursor + needed <= out.length)
            return;
        const grown = new Uint8Array(Math.max(out.length * 2, cursor + needed));
        grown.set(out.subarray(0, cursor));
        out = grown;
    };
    for (let i = 0; i < s.length; i++) {
        const c = s.charCodeAt(i);
        if (c < 0x80) {
            ensure(1);
            out[cursor++] = c;
        }
        else if (c < 0x800) {
            ensure(2);
            out[cursor++] = 0xc0 | (c >> 6);
            out[cursor++] = 0x80 | (c & 0x3f);
        }
        else if (c >= 0xd800 && c <= 0xdbff) {
            const low = i + 1 < s.length ? s.charCodeAt(i + 1) : -1;
            if (low >= 0xdc00 && low <= 0xdfff) {
                i += 1;
                const cp = 0x10000 + ((c - 0xd800) << 10) + (low - 0xdc00);
                ensure(4);
                out[cursor++] = 0xf0 | (cp >> 18);
                out[cursor++] = 0x80 | ((cp >> 12) & 0x3f);
                out[cursor++] = 0x80 | ((cp >> 6) & 0x3f);
                out[cursor++] = 0x80 | (cp & 0x3f);
            }
            else {
                // WHATWG TextEncoder와 동일하게 고립 surrogate는 U+FFFD로 치환한다.
                ensure(3);
                out[cursor++] = 0xef;
                out[cursor++] = 0xbf;
                out[cursor++] = 0xbd;
            }
        }
        else if (c >= 0xdc00 && c <= 0xdfff) {
            ensure(3);
            out[cursor++] = 0xef;
            out[cursor++] = 0xbf;
            out[cursor++] = 0xbd;
        }
        else {
            ensure(3);
            out[cursor++] = 0xe0 | (c >> 12);
            out[cursor++] = 0x80 | ((c >> 6) & 0x3f);
            out[cursor++] = 0x80 | (c & 0x3f);
        }
    }
    return out.subarray(0, cursor);
}
function _utf8Decode(bytes, start, end) {
    let s = '';
    let i = start;
    while (i < end) {
        const b = bytes[i];
        if (b < 0x80) {
            s += String.fromCharCode(b);
            i += 1;
        }
        else if ((b & 0xe0) === 0xc0) {
            s += String.fromCharCode(((b & 0x1f) << 6) | (bytes[i + 1] & 0x3f));
            i += 2;
        }
        else if ((b & 0xf0) === 0xe0) {
            s += String.fromCharCode(((b & 0x0f) << 12) | ((bytes[i + 1] & 0x3f) << 6) | (bytes[i + 2] & 0x3f));
            i += 3;
        }
        else if ((b & 0xf8) === 0xf0) {
            const cp = ((b & 0x07) << 18) |
                ((bytes[i + 1] & 0x3f) << 12) |
                ((bytes[i + 2] & 0x3f) << 6) |
                (bytes[i + 3] & 0x3f);
            const adj = cp - 0x10000;
            s += String.fromCharCode(0xd800 + (adj >> 10), 0xdc00 + (adj & 0x3ff));
            i += 4;
        }
        else {
            i += 1;
        }
    }
    return s;
}
/** getLiveSchema 의 파싱 내부 — 엔진 생성 시 schemaVersion 까지 읽는다 (T2). */
function parseLiveSchemaDocument(native) {
    if (!native.getSchema) {
        // (의미론 마감) 네이티브가 getSchema 를 노출하지 않으면 live schema 자체를
        // 얻을 수 없다 — 빈 Map 을 돌려주면 Tier 3 동적 명령이 command.not_found 로
        // 오해받는다. 스키마 조회가 실제로 필요한 호출자가 즉시 실패하도록 명시적
        // 에러를 던진다 (엔진 생성 시 schemaVersion 협상은 선택적이라 try/catch 로
        // 이미 흡수된다).
        throw new RustraCommandError('schema.unavailable', 'native module does not expose getSchema(); live schema is unavailable');
    }
    const bytes = native.getSchema();
    const u = new Uint8Array(bytes);
    const json = _utf8Decode(u, 0, u.length);
    const parsed = JSON.parse(json);
    const map = new Map();
    for (const c of parsed.commands ?? []) {
        map.set(c.name, {
            commandId: c.commandId,
            inputSchema: c.inputSchema,
            outputSchema: c.outputSchema,
        });
    }
    const doc = { commands: map };
    if (typeof parsed.schemaVersion === 'number' && Number.isFinite(parsed.schemaVersion)) {
        doc.schemaVersion = parsed.schemaVersion;
    }
    return doc;
}
/**
 * 네이티브 getSchema() 로부터 현재 명령 스키마를 조회한다 (정적 + 동적 명령 포함).
 * 동적 명령의 commandId/타입을 알아내 rkyvV2 Tier 3 fallback 에 사용된다.
 * getSchema 미노출 네이티브에서는 schema.unavailable 에러를 던진다.
 */
export function getLiveSchema(native) {
    return parseLiveSchemaDocument(native).commands;
}
// ── Tier 3 (JSON-in-binary) wire helpers ────────────────────
// request:  [command_id: u16 LE @0][json @2]
// success:  [ok:1 @0][pad 3B][json_len: u32 LE @4][json @8]
// error:    [ok:0 @0][pad to @8][err_len: u16 LE @8][postcard({code,message}) @10]
function encodeTier3Request(commandId, args) {
    const json = _utf8Encode(JSON.stringify(args ?? {}, _jsonSetReplacer));
    const buf = new Uint8Array(2 + json.length);
    new DataView(buf.buffer).setUint16(0, commandId, true);
    buf.set(json, 2);
    return buf.buffer;
}
/**
 * JSON 경로에서 `Set`을 배열로 직렬화한다 — Rust `BTreeSet`/`HashSet`은
 * serde JSON 에서 배열로 직렬화되므로 와이어 호환을 맞춘다
 * (`Map`은 rustra 계약에 없으므로 다루지 않는다).
 */
function _jsonSetReplacer(_key, value) {
    if (value instanceof Set)
        return [...value];
    return value;
}
// postcard varint + length-prefixed string decode, local to the Tier 3 path so
// this file has no dependency on the generated codec helpers.
function _tier3DecodeString(u, offset) {
    let shift = 0;
    let bytesRead = 0;
    let len = 0;
    while (true) {
        const b = u[offset + bytesRead];
        len |= (b & 0x7f) << shift;
        bytesRead++;
        if ((b & 0x80) === 0)
            break;
        shift += 7;
        if (bytesRead > 5)
            throw new Error('varint too long');
    }
    len = len >>> 0;
    const start = offset + bytesRead;
    return {
        value: _utf8Decode(u, start, start + len),
        bytesRead: bytesRead + len,
    };
}
function decodeTier3Response(bytes) {
    if (bytes.byteLength < 8) {
        return { ok: false, error: { code: 'invoke.too_short', message: 'response too short' } };
    }
    const u = new Uint8Array(bytes);
    if (u[0] === 1) {
        const len = new DataView(bytes).getUint32(4, true);
        if (bytes.byteLength < 8 + len) {
            return {
                ok: false,
                error: { code: 'invoke.too_short', message: 'response payload truncated' },
            };
        }
        const json = _utf8Decode(u, 8, 8 + len);
        try {
            return { ok: true, result: JSON.parse(json) };
        }
        catch (e) {
            return { ok: false, error: { code: 'invoke.malformed', message: `invalid json: ${e}` } };
        }
    }
    if (bytes.byteLength < 10) {
        return { ok: false, error: { code: 'invoke.too_short', message: 'error frame too short' } };
    }
    const errLen = new DataView(bytes).getUint16(8, true);
    let error = { code: 'invoke.failed', message: 'invoke failed' };
    if (errLen > 0) {
        // postcard({ code: String, message: String })
        try {
            const { value: code, bytesRead: b1 } = _tier3DecodeString(u, 10);
            const { value: message } = _tier3DecodeString(u, 10 + b1);
            error = { code, message };
        }
        catch {
            // fallback if postcard decoding fails
        }
    }
    return { ok: false, error };
}
/**
 * tier 2(JS 코덱) 응답 프레임을 결과/에러로 환산한다 — `dispatch` 와 전파
 * 경로 콜백이 공유하는 유일 경로 (T1 리뷰). `codec.decode` 가 잘못된 프레임으로
 * throw 하면 그 예외를 reject 값으로 돌린다(비-Error 는 `invoke.failed` 로
 * 래핑): 전파 경로의 콜백은 네이티브 트램펄린 안에서 실행되므로 예외가
 * 새어나가면 프라미스가 영원히 정착하지 않는다. 이 함수 자체는 throw 하지 않는다.
 */
function tier2Outcome(codec, frame) {
    let response;
    try {
        response = codec.decode(frame);
    }
    catch (err) {
        return {
            ok: false,
            error: err instanceof Error
                ? err
                : new RustraCommandError('invoke.failed', `codec decode failed: ${String(err)}`),
        };
    }
    if (!response.ok) {
        const e = response.error ?? { code: 'invoke.failed', message: 'RkyvV2 invoke failed' };
        return {
            ok: false,
            error: new RustraCommandError(e.code, e.message, e.retryable ?? isRetryableCode(e.code)),
        };
    }
    return { ok: true, value: response.result };
}
/**
 * 얕은 취소 (T1) — 네이티브 전파가 불가능할 때 JS 프라미스만 거부한다.
 * Rust 핸들러는 끝까지 실행되며, 그 결과는 이 프라미스 체인에서 버려진다.
 */
function raceAbort(promise, signal, command) {
    return new Promise((resolve, reject) => {
        const onAbort = () => reject(new RustraCommandError('cancelled', `invoke("${command}") aborted`, true));
        signal.addEventListener('abort', onAbort, { once: true });
        promise.then((v) => {
            signal.removeEventListener('abort', onAbort);
            resolve(v);
        }, (e) => {
            signal.removeEventListener('abort', onAbort);
            reject(e);
        });
    });
}
/**
 * (T3) 인코딩된 페이로드의 크기 사전 검사 — JS 코덱(tier 2)/tier 3 경로가
 * 네이티브를 호출하기 직전에 공유한다. `limit` 이 undefined 면 검사하지 않는다
 * (네이티브의 동적 한도가 최종 게이트). 초과 시 `payload.too_large`
 * (non-retryable — 결정론적 클라이언트 조건) 를 반환하고 호출자는 네이티브
 * 왕복 없이 즉시 reject 한다.
 */
function payloadTooLargeError(encodedBytes, limit) {
    if (limit === undefined || encodedBytes <= limit)
        return undefined;
    return new RustraCommandError('payload.too_large', `encoded payload ${encodedBytes}B exceeds maxPayloadBytes ${limit}B`, false);
}
export function createRkyvV2Engine(native, registry, options) {
    // 동적 명령 lookup의 getSchema → UTF-8 decode → JSON.parse를 호출마다
    // 반복하지 않는다. 엔진별 lazy cache를 쓰고 debug runtime mutation은
    // refreshLiveSchema() 또는 cache miss 시 자동 refresh로 반영한다.
    let liveSchemaCache;
    const refreshLiveSchema = () => {
        const document = parseLiveSchemaDocument(native);
        liveSchemaCache = document.commands;
        return liveSchemaCache;
    };
    const lookupCachedLiveSchemaEntry = (command) => {
        const cached = liveSchemaCache?.get(command);
        if (cached)
            return cached;
        try {
            return refreshLiveSchema().get(command);
        }
        catch {
            return undefined;
        }
    };
    // F5 (opt-in): 계약 해시 검증. 빌드 시점 hash 와 네이티브 실시간 hash 가 다르면
    // 기본적으로 엔진을 만들지 않고 즉시 실패(fail-fast)한다. T2 onContractMismatch
    // 콜백을 설정하면 불일치 시 throw 대신 콜백 호출 후 degraded 모드로 계속 생성한다.
    if (options?.contractHash !== undefined) {
        if (typeof native.getContractHash !== 'function') {
            // unenforceable 은 콜백과 무관하게 항상 throw — native hash 가 없으면
            // degraded 모드가 무의미하다 (검증 가능한 것이 아무것도 없다).
            throw new RustraCommandError('contract.unenforceable', 'contractHash option was set but the native module does not expose ' +
                'getContractHash(); cannot verify schema drift. Check that the current generated codecs ' +
                'and Rust native archive were both compiled into the installed app.');
        }
        const hashBytes = new Uint8Array(native.getContractHash());
        const nativeHash = _utf8Decode(hashBytes, 0, hashBytes.length).trim();
        if (nativeHash !== options.contractHash) {
            if (!options.onContractMismatch) {
                throw new RustraCommandError('contract.mismatch', `contract hash mismatch: native="${nativeHash.slice(0, 16)}…" vs ` +
                    `expected="${options.contractHash.slice(0, 16)}…" — generated client ` +
                    `and native binary are out of sync; regenerate the TypeScript and native codecs, ` +
                    `rebuild the Rust archive, then rebuild the native app`);
            }
            options.onContractMismatch({ nativeHash, expectedHash: options.contractHash });
        }
    }
    // T2 (opt-in): schemaVersion staleness 검사. JS > native 면 경고만 한다
    // (fatal 아님 — OTA 롤백/지연 배포 상황에서도 앱은 동작해야 한다).
    // getSchema 미노출 구 네이티브는 조용히 건너뛴다 (비교할 것이 없다).
    if (options?.schemaVersion !== undefined && typeof native.getSchema === 'function') {
        // 구 네이티브(pre-Task-8)의 schema JSON 에는 schemaVersion 이 없다 —
        // CLI old-schema 관례대로 1 로 취급한다. 이 기능의 대상이 되는 정확히 그
        // 구 바이너리를 향한 스퓨리어스 경고를 막는 디폴트다.
        //
        // 스키마 파싱(getSchema 호출 자체의 실패 포함)은 절대 치명적이지 않다 —
        // 파싱이 throw 하면 staleness 검사를 조용히 건너뛴다 (getSchema 미노출
        // 경우와 동일한 취급). 경고 기능이 엔진 생성을 깨뜨리면 "fatal 아님"
        // 계약 자체가 위반된다. invoke 시점의 tier-3 스키마 파싱(getLiveSchema)은
        // 별개의 기존 동작 — malformed 스키마에서 동적 명령 호출 시 JSON.parse 가
        // dispatch 밖으로 동기 throw 할 수 있다.
        let nativeVersion;
        try {
            const document = parseLiveSchemaDocument(native);
            liveSchemaCache = document.commands;
            nativeVersion = document.schemaVersion ?? 1;
        }
        catch {
            nativeVersion = undefined;
        }
        if (nativeVersion !== undefined && options.schemaVersion > nativeVersion) {
            const info = { nativeVersion, jsVersion: options.schemaVersion };
            if (options.onSchemaStale) {
                options.onSchemaStale(info);
            }
            else {
                console.warn(`[rustra] schema stale: JS bundle schemaVersion=${info.jsVersion} > ` +
                    `native schemaVersion=${info.nativeVersion} — native binary is older ` +
                    `than the JS bundle (OTA rollback / delayed rollout); newer commands ` +
                    `may fail until native catches up`);
            }
        }
    }
    // B1 fast path: 네이티브가 C++ typed 코덱(invokeTyped + hasStaticCodec)을 노출하면
    // 정적 명령을 C++에서 postcard 인코딩/디코딩한다 (JS codec 왕복 ~3.4µs 제거).
    const hasLegacyTypedPath = !!(native.invokeTyped && native.hasStaticCodec);
    const hasCapabilityPath = typeof native.getCodecCapabilities === 'function' &&
        typeof native.invokeTypedById === 'function';
    const hasTypedPath = hasLegacyTypedPath || hasCapabilityPath;
    // P0-3: byId 진입(invokeTypedById)이 가능한지 — 가능하면 dispatch 1순위가
    // JSI 1회 횡단 + u16 디스패치로 바뀐다. 미노출이면 이름 기반 invokeTyped 유지.
    const hasByIdPath = hasTypedPath && typeof native.invokeTypedById === 'function';
    const hasRawPath = hasCapabilityPath && typeof native.invokeTypedRaw === 'function';
    const hasPositionalPath = hasCapabilityPath && typeof native.invokeTypedPos === 'function';
    const hasBufferPath = hasCapabilityPath && typeof native.invokeTypedBuffer === 'function';
    // P0-2: 단일 횡단 배치가 가능하려면 invokeTypedBatch 도 필요.
    const hasBatchPath = hasTypedPath && !!native.invokeTypedBatch;
    // P0-2 byId: 배치 진입의 cmd_id 배열 변형 — 문자열 마샬링 N 회 제거.
    const hasBatchByIdPath = hasBatchPath && typeof native.invokeTypedBatchById === 'function';
    // (T3) JS 사전 크기 검사 — undefined 면 검사하지 않는다 (네이티브 동적 한도가
    // 최종 게이트). typed(tier 1) 경로는 JS 측 인코딩이 없어 검사 대상이 아니다.
    const payloadLimit = options?.maxPayloadBytes;
    // 정적 명령 집합 JS 캐시 (P0-3) — hasStaticCodec JSI 호출을 호출당 1회에서
    // 엔진 생애 1회 스윕으로 축소한다. 불변식: 코드젠 시점 정적 명령은 항상
    // registry 에 있다 (registry 도 코드젠 산출물). registry 에 없는 이름은
    // 동적 명령 → Tier 3 경로. 스윕은 registry 를 기준으로 하므로 C++ 코덱만
    // 있고 registry 에 빠진 정적 명령(불변식 위반)도 자연스럽게 Tier 3 로 간다.
    // 스윕 도중 예외 시 부분 맵 재사용 — 미스윕 항목은 registry 안 이름이므로
    // Tier 2(JS codec)로 라우팅된다. Tier 3는 registry 밖 동적 명령 전용 경로다.
    let staticCommandIds = null;
    let staticCommandNamesById = null;
    let staticCommandCapabilitiesById = null;
    const ensureStaticIds = () => {
        if (staticCommandIds !== null || !hasTypedPath)
            return staticCommandIds;
        staticCommandIds = new Map();
        staticCommandNamesById = [];
        staticCommandCapabilitiesById = [];
        for (const [name, codec] of registry) {
            const capabilities = hasCapabilityPath
                ? native.getCodecCapabilities(codec.commandId)
                : native.hasStaticCodec(name)
                    ? CODEC_TYPED
                    : 0;
            if ((capabilities & CODEC_TYPED) !== 0) {
                staticCommandIds.set(name, codec.commandId);
                staticCommandNamesById[codec.commandId] = name;
                staticCommandCapabilitiesById[codec.commandId] = capabilities;
            }
        }
        return staticCommandIds;
    };
    const isVerifiedStaticId = (commandId, command) => {
        return staticCommandNamesById?.[commandId] === command;
    };
    // 신호 없는 기본 3-티어 dispatch (T1 리팩터링 — 로직은 기존 그대로).
    // encodeInto 재사용 버퍼 풀 — 커맨드 이름별 최근 버퍼 1개(단일 진입 dispatch
    // 의 직렬 인코딩 전제). 미사용 시 맵은 비어 있어 오버헤드 0이다.
    const encodeIntoBuffers = new Map();
    const dispatch = (command, args) => {
        // 1순위: C++ fast path (RN JSI). 정적 명령만. JS 측 인코딩이 없어
        // maxPayloadBytes 검사를 건너뛴다 — 네이티브 한도가 그대로 적용된다.
        // byId 진입이 가능하면 JSI 1회 + u16 디스패치 (P0-3).
        if (hasTypedPath) {
            const cmdId = ensureStaticIds()?.get(command);
            if (cmdId !== undefined) {
                if (hasByIdPath) {
                    return native.invokeTypedById(cmdId, args);
                }
                return native.invokeTyped(command, args);
            }
        }
        // 2순위: JS codec (Node/Bun/Tauri 또는 typed 누락 시). 정적 명령.
        const codec = registry.get(command);
        if (codec) {
            // encodeInto(재사용 버퍼)가 있으면 호출당 신규 할당을 피한다. 버퍼는
            // 커맨드별로 1개(단일 진입 dispatch 는 동시에 한 요청만 인코딩한다)다.
            // invokeRkyvV2 는 왕복 전에 버퍼를 소비하므로 재진입 안전하다.
            let encoded;
            if (codec.encodeInto) {
                const bucket = encodeIntoBuffers;
                const reuse = bucket.get(command);
                const written = codec.encodeInto(args, reuse);
                if (written.buffer !== reuse?.buffer)
                    bucket.set(command, written);
                encoded = written.buffer;
                if (written.byteOffset !== 0 || written.byteLength !== written.buffer.byteLength) {
                    // 재사용 버퍼가 subarray 라면 정확한 슬라이스 ArrayBuffer 로 사본을
                    // 만든다(첫 호출 grow 후엔 byteOffset 0/full-length 로 수렴한다).
                    encoded = written.slice().buffer;
                }
            }
            else {
                encoded = codec.encode(args);
            }
            // (T3) 네이티브 왕복 전에 크기 검사 — 초과면 invokeRkyvV2 를 부르지 않는다.
            const tooLarge = payloadTooLargeError(encoded.byteLength, payloadLimit);
            if (tooLarge)
                throw tooLarge;
            const resultBytes = native.invokeRkyvV2(encoded);
            // Reject (do not throw) so the declared Promise<T> contract holds and
            // callers can use .catch() / await-try-consistently for command errors.
            const outcome = tier2Outcome(codec, resultBytes);
            if (!outcome.ok)
                throw outcome.error;
            return outcome.value;
        }
        // 3순위: 동적 명령 → Tier 3 fallback (live schema 의 commandId 사용).
        // getSchema 미노출 네이티브에서 cached lookup은 undefined 를
        // 돌려주므로 기존 command.not_found 계약이 그대로 유지된다.
        const entry = lookupCachedLiveSchemaEntry(command);
        if (!entry) {
            throw new RustraCommandError('command.not_found', `RkyvV2: no codec and not in live schema for "${command}"`);
        }
        const tier3Request = encodeTier3Request(entry.commandId, args);
        // (T3) tier 2 와 동일한 사전 검사 — 네이티브 호출 전에 조기 실패.
        const tooLarge = payloadTooLargeError(tier3Request.byteLength, payloadLimit);
        if (tooLarge)
            throw tooLarge;
        const resp = decodeTier3Response(native.invokeRkyvV2(tier3Request));
        if (!resp.ok) {
            const e = resp.error ?? { code: 'invoke.failed', message: 'RkyvV2 (tier3) invoke failed' };
            throw new RustraCommandError(e.code, e.message, e.retryable ?? isRetryableCode(e.code));
        }
        return resp.result;
    };
    // 공개 EngineClient는 항상 Promise를 반환하지만 RN JSI/Node/Bun의 기본
    // dispatch는 동기다. 단 한 번만 Promise로 승격해 async dispatch가 만들던
    // 불필요한 Promise/microtask를 제거하고, 동기 throw는 rejected Promise로
    // 바꿔 기존 호출 계약을 유지한다.
    const dispatchPromise = (command, args) => {
        try {
            return Promise.resolve(dispatch(command, args));
        }
        catch (error) {
            return Promise.reject(error);
        }
    };
    const dispatchById = (commandId, command, args) => {
        if (hasByIdPath && isVerifiedStaticId(commandId, command)) {
            return native.invokeTypedById(commandId, args);
        }
        return dispatch(command, args);
    };
    const dispatchGeneratedFields = (commandId, command, args, fieldCount, field0, field1, field2) => {
        if (isVerifiedStaticId(commandId, command)) {
            const capabilities = staticCommandCapabilitiesById?.[commandId] ?? 0;
            if (hasRawPath && (capabilities & CODEC_RAW) !== 0) {
                const result = fieldCount === 1
                    ? native.invokeTypedRaw(commandId, field0)
                    : fieldCount === 2
                        ? native.invokeTypedRaw(commandId, field0, field1)
                        : native.invokeTypedRaw(commandId, field0, field1, field2);
                // New hosts return the declared output shape. NaN is the defensive
                // fallback marker used when the Rust registry cannot service raw even
                // though generated metadata advertised it.
                if (!(typeof result === 'number' && Number.isNaN(result)))
                    return result;
            }
            if (hasPositionalPath && (capabilities & CODEC_POSITIONAL) !== 0) {
                return (fieldCount === 1
                    ? native.invokeTypedPos(commandId, field0)
                    : fieldCount === 2
                        ? native.invokeTypedPos(commandId, field0, field1)
                        : native.invokeTypedPos(commandId, field0, field1, field2));
            }
        }
        return dispatchById(commandId, command, args);
    };
    const dispatchPromiseById = (commandId, command, args) => {
        try {
            return Promise.resolve(dispatchById(commandId, command, args));
        }
        catch (error) {
            return Promise.reject(error);
        }
    };
    const resolveGeneratedFieldsRoute = (commandId, command, fieldCount) => {
        if (staticCommandNamesById?.[commandId] !== command)
            return undefined;
        const capabilities = staticCommandCapabilitiesById?.[commandId] ?? 0;
        let fallback;
        if (hasPositionalPath && (capabilities & CODEC_POSITIONAL) !== 0) {
            fallback =
                fieldCount === 1
                    ? (_args, field0) => native.invokeTypedPos(commandId, field0)
                    : fieldCount === 2
                        ? (_args, field0, field1) => native.invokeTypedPos(commandId, field0, field1)
                        : (_args, field0, field1, field2) => native.invokeTypedPos(commandId, field0, field1, field2);
        }
        else if (hasByIdPath) {
            fallback = (args) => native.invokeTypedById(commandId, args);
        }
        if (hasRawPath && (capabilities & CODEC_RAW) !== 0) {
            if (!fallback) {
                return fieldCount === 1
                    ? (_args, field0) => native.invokeTypedRaw(commandId, field0)
                    : fieldCount === 2
                        ? (_args, field0, field1) => native.invokeTypedRaw(commandId, field0, field1)
                        : (_args, field0, field1, field2) => native.invokeTypedRaw(commandId, field0, field1, field2);
            }
            const rawFallback = fallback;
            return fieldCount === 1
                ? (args, field0) => {
                    const result = native.invokeTypedRaw(commandId, field0);
                    return typeof result === 'number' && Number.isNaN(result)
                        ? rawFallback(args, field0)
                        : result;
                }
                : fieldCount === 2
                    ? (args, field0, field1) => {
                        const result = native.invokeTypedRaw(commandId, field0, field1);
                        return typeof result === 'number' && Number.isNaN(result)
                            ? rawFallback(args, field0, field1)
                            : result;
                    }
                    : (args, field0, field1, field2) => {
                        const result = native.invokeTypedRaw(commandId, field0, field1, field2);
                        return typeof result === 'number' && Number.isNaN(result)
                            ? rawFallback(args, field0, field1, field2)
                            : result;
                    };
        }
        return fallback;
    };
    const resolveGeneratedBytesRoute = (commandId, command) => {
        if (staticCommandNamesById?.[commandId] !== command)
            return undefined;
        const capabilities = staticCommandCapabilitiesById?.[commandId] ?? 0;
        const fallback = resolveGeneratedFieldsRoute(commandId, command, 1);
        if (!hasBufferPath || (capabilities & CODEC_BUFFER) === 0)
            return fallback;
        return (args, value) => isNativeByteBuffer(value)
            ? native.invokeTypedBuffer(commandId, value)
            : fallback
                ? fallback(args, value)
                : dispatchById(commandId, command, args);
    };
    // Capability routing is immutable for one engine/native runtime. Resolve it
    // once at construction so generated calls do not pay an ensure function and
    // two Map lookups on every invocation.
    ensureStaticIds();
    return {
        refreshLiveSchema,
        [invokeByIdSync](commandId, command, args) {
            return dispatchById(commandId, command, args);
        },
        [invokeGeneratedFieldsSync](commandId, command, args, fieldCount, field0, field1, field2) {
            return dispatchGeneratedFields(commandId, command, args, fieldCount, field0, field1, field2);
        },
        [resolveGeneratedFieldsSync](commandId, command, fieldCount) {
            return resolveGeneratedFieldsRoute(commandId, command, fieldCount);
        },
        [invokeGeneratedBytesSync](commandId, command, args, value) {
            const route = resolveGeneratedBytesRoute(commandId, command);
            return route
                ? route(args, value)
                : dispatchGeneratedFields(commandId, command, args, 1, value);
        },
        [resolveGeneratedBytesSync](commandId, command) {
            return resolveGeneratedBytesRoute(commandId, command);
        },
        invoke(command, args, options) {
            const signal = options?.signal;
            if (signal?.aborted) {
                return Promise.reject(new RustraCommandError('cancelled', `invoke("${command}") aborted before dispatch`, true));
            }
            if (!signal)
                return dispatchPromise(command, args);
            // 네이티브 전파 경로 (T1): JS 코덱(tier 2) 명령이고 invokeAsync +
            // invokeCancel 이 모두 노출되면 Rust 측 체크포인트까지 취소가 닿는다.
            // typed(tier 1)/tier 3 동적 경로는 invokeAsync 가 있어도 얕은 취소로
            // 폴백한다 (설계 노트: 전파는 JS 코덱 경로만).
            const codec = registry.get(command);
            // P0-3: hasStaticCodec JSI 호출 대신 엔진 생애 1회 스윕 캐시 조회.
            const onTypedPath = hasTypedPath && ensureStaticIds()?.has(command) === true;
            if (!onTypedPath && codec && native.invokeAsync && native.invokeCancel) {
                return new Promise((resolve, reject) => {
                    let settled = false;
                    let invocationId = -1;
                    const onAbort = () => {
                        if (settled)
                            return;
                        settled = true;
                        native.invokeCancel(invocationId);
                        reject(new RustraCommandError('cancelled', `invoke("${command}") aborted`, true));
                    };
                    // encode/invokeAsync 가 동기 throw 해도 abort 리스너가 signal 에
                    // 새어남기지 않도록 try/catch 로 정리한다. catch 에서 reject 할 때
                    // 이미 콜백이 정착했다면 reject 는 no-op 이므로 안전하다.
                    try {
                        // invokeAsync 가 콜백을 동기적으로 부를 수 있으므로 리스너를 먼저 단다.
                        signal.addEventListener('abort', onAbort, { once: true });
                        const encoded = codec.encode(args);
                        // (T3) 전파 경로도 동일한 사전 검사 — 초과면 invokeAsync 를 부르지
                        // 않고 throw 한다. Error 이므로 아래 catch 가 리스너를 정리한 뒤
                        // 그대로 reject 한다 (기존 동기 throw 정리 경로 재사용).
                        const tooLarge = payloadTooLargeError(encoded.byteLength, payloadLimit);
                        if (tooLarge)
                            throw tooLarge;
                        invocationId = native.invokeAsync(encoded, (resp) => {
                            if (settled)
                                return;
                            // settled 를 올리기 전에 환산한다 — tier2Outcome 은 decode 가
                            // throw 해도 (잘못된 프레임) 에러로 환산할 뿐 절대 throw 하지
                            // 않으므로, 이 지점 이후 프라미스는 반드시 정착한다. 예외가
                            // 네이티브 트램펄린으로 새어나가 영원히 대기하는 일이 없다.
                            const outcome = tier2Outcome(codec, resp);
                            settled = true;
                            signal.removeEventListener('abort', onAbort);
                            if (outcome.ok)
                                resolve(outcome.value);
                            else
                                reject(outcome.error);
                        });
                    }
                    catch (err) {
                        settled = true;
                        signal.removeEventListener('abort', onAbort);
                        reject(err instanceof Error
                            ? err
                            : new RustraCommandError('invoke.failed', `invoke("${command}") dispatch failed: ${String(err)}`));
                    }
                });
            }
            // (의미론 마감) typed(tier 1)/tier 3 경로 전파 확장 — 코덱이 없어도
            // invokeAsync + invokeCancel 이 노출되면 Rust 취소 체크포인트까지 전파한다.
            // 인코딩: typed 캐시에 commandId 가 있으면 Tier 3(JSON-in-binary) 프레임으로
            // invokeRkyvV2 와 동일한 와이어를 invokeAsync 로 보낸다. commandId 를 모르면
            // (live schema 미노출) 얕은 취소로 폴백한다.
            if (!codec && native.invokeAsync && native.invokeCancel) {
                const cmdId = hasTypedPath ? ensureStaticIds()?.get(command) : undefined;
                const entry = cmdId !== undefined ? { commandId: cmdId } : lookupCachedLiveSchemaEntry(command);
                if (entry) {
                    return new Promise((resolve, reject) => {
                        let settled = false;
                        let invocationId = -1;
                        const onAbort = () => {
                            if (settled)
                                return;
                            settled = true;
                            native.invokeCancel(invocationId);
                            reject(new RustraCommandError('cancelled', `invoke("${command}") aborted`, true));
                        };
                        try {
                            signal.addEventListener('abort', onAbort, { once: true });
                            const encoded = encodeTier3Request(entry.commandId, args);
                            const tooLarge = payloadTooLargeError(encoded.byteLength, payloadLimit);
                            if (tooLarge)
                                throw tooLarge;
                            invocationId = native.invokeAsync(encoded, (resp) => {
                                if (settled)
                                    return;
                                settled = true;
                                signal.removeEventListener('abort', onAbort);
                                const outcome = decodeTier3Response(resp);
                                if (outcome.ok)
                                    resolve(outcome.result);
                                else {
                                    const e = outcome.error ??
                                        { code: 'invoke.failed', message: 'RkyvV2 (tier3) invoke failed' };
                                    reject(new RustraCommandError(e.code, e.message, e.retryable ?? false));
                                }
                            });
                        }
                        catch (err) {
                            settled = true;
                            signal.removeEventListener('abort', onAbort);
                            reject(err instanceof Error
                                ? err
                                : new RustraCommandError('invoke.failed', `invoke("${command}") dispatch failed: ${String(err)}`));
                        }
                    });
                }
            }
            // 전파 불가 — 얕은 취소 (JS 프라미스만 거부, Rust 는 끝까지 실행):
            return raceAbort(dispatchPromise(command, args), signal, command);
        },
        invokeById(commandId, command, args, options) {
            const signal = options?.signal;
            if (signal?.aborted) {
                return Promise.reject(new RustraCommandError('cancelled', `invoke("${command}") aborted before dispatch`, true));
            }
            if (!signal)
                return dispatchPromiseById(commandId, command, args);
            // 검증된 typed-by-id 명령은 기존 invoke의 typed 경로와 동일하게 얕은
            // 취소를 적용한다. 검증 실패/구 네이티브는 기존 이름 경로가 취소 전파
            // 가능 여부를 판단하도록 위임한다.
            if (hasByIdPath && isVerifiedStaticId(commandId, command)) {
                return raceAbort(dispatchPromiseById(commandId, command, args), signal, command);
            }
            return this.invoke(command, args, options);
        },
        invokeBatch(entries) {
            // 계약: 단일 JSI 횡단 배치(invokeTypedBatch[ById])는 취소를 지원하지
            // 않는다 — signal 이 붙은 항목이 하나라도 있으면 자동으로 항목별
            // invoke 경로(각자의 전파/얕은 취소 정책)로 라우팅된다. 배치 자체의
            // 항목별 취소 지원은 명시적 미지원 계약 (followup-3 유예 유지).
            //
            // 모든 항목이 정적 코덱이고 signal 이 없어야 단일 JSI 횡단으로 일괄 처리.
            // 단일 횡단 진입은 2단계: byId 배치(invokeTypedBatchById) 가 우선, 미노출이면
            // 이름 기반 invokeTypedBatch(아래 분기 참조). 정적 여부/id 조사는 캐시
            // 조회로 한다 (P0-3: hasStaticCodec JSI 호출 N 회 → 엔진 생애 1회 스윕).
            const staticIds = hasBatchPath && entries.length > 0 ? ensureStaticIds() : null;
            if (staticIds &&
                entries.every((e) => staticIds.has(e.command)) &&
                entries.every((e) => !e.options?.signal)) {
                const args = entries.map((e) => e.args);
                // byId 진입(P0-2 후속): 네이티브가 cmd_id 배열 배치를 노출하면 문자열
                // 배열 마샬링 없이 id 로 단일 횡단. 모든 항목의 id 가 캐시에 있는 위의
                // every 검사가 이미 조립 가능성을 보장한다.
                if (hasBatchByIdPath) {
                    const ids = entries.map((e) => staticIds.get(e.command));
                    const results = native.invokeTypedBatchById(ids, args);
                    return Promise.resolve(results);
                }
                const names = entries.map((e) => e.command);
                const results = native.invokeTypedBatch(names, args);
                return Promise.resolve(results);
            }
            // 동적 명령/시그널 항목이 섞였거나 배치 미지원 → 항목별 라우팅.
            // 항목의 options(signal) 를 그대로 실어 보내 항목 단위 취소가 각자의
            // 취소 정책(전파/얕은)을 따르게 한다 (T1 후속).
            return Promise.all(entries.map((e) => this.invoke(e.command, e.args, e.options)));
        },
    };
}
//# sourceMappingURL=index.js.map