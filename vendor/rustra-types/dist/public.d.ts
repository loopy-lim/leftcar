import type { RustraError } from './errors.js';
import type { LiveSchemaEntry } from './live-schema.js';
/**
 * 어댑터가 실측으로 제공하는 기술적 지원 표면(A02) — 호환성 매트릭스
 * (docs/compatibility-matrix.md)의 각 셀을 기계 판독 가능하게 옮긴 것.
 * 새 주장이 아니라 기존 문서 계약의 타입화다. 앱은 부작용 이전에
 * `engine.supports?.cancellation === 'cooperative'` 로 분기할 수 있다.
 *
 * - `cancellation`: in-flight `options.signal` 의 실제 관측 수준.
 *   `'shallow'` = JS 프라미스만 거부(Rust 실행은 계속), `'pre-abort'` =
 *   사전 abort 만 보장, `'cooperative'` = Rust 체크포인트까지 전파(조건부).
 *   사전 abort 는 모든 어댑터가 보장하므로 레벨 값에 포함하지 않는다.
 * - `batch`: `invokeBatch` 의 실행 형태. `'single-crossing'` = 정적 명령을
 *   단일 네이티브 횡단으로 일괄(signal 항목은 항목별로 자동 라우팅),
 *   `'per-entry'` = 항목별 invoke, `'none'` = 배치 표면 없음.
 * - `events`: 이벤트 전달 경로. `'push'` 는 폴링 폴백을 포함한다(매트릭스 셀의
 *   "푸시 + 폴백 폴링" 표기) — 실제 경로 판별은 어댑터별 구독 표면을 쓴다.
 * - `timeoutPreemption`: `options.timeoutMs` 레이스가 존재하는지. `false` 면
 *   동기 네이티브 호출을 중단할 수 없다(RN JSON 어댑터).
 *
 * 기술적 지원만 담는다 — 권한(capability)은 별도 표면이다. 초기값은
 * 어댑터의 엔진 생성 함수가 자신의 매트릭스 셀에서 채운다.
 */
export type EngineSupports = {
    cancellation: 'pre-abort' | 'shallow' | 'cooperative';
    batch: 'single-crossing' | 'per-entry' | 'none';
    events: 'push' | 'polling' | 'none';
    channels: boolean;
    /** RN JSON = false — 동기 native 호출은 실행 중 선점할 수 없다. */
    timeoutPreemption: boolean;
};
export type EngineClient = {
    /**
     * 이 엔진의 기술적 지원 표면(A02) — 값은 호환성 매트릭스의 셀과 1:1.
     * 옵셔널: 타사 엔진 구현체에 요구하지 않는다(구조적 호환 유지).
     */
    supports?: EngineSupports;
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
 * `invokeBatchSettled` 의 항목별 결과(A04). 실패한 항목 **이후** 실행되지 않은
 * 항목은 `rejected` 가 아니라 `unexecuted` 다 — "실행됐지만 실패"와 "실행 안 됨"
 * 의 구분이 이 표면의 핵심 가치다. 실행 정책(항상 per-entry 순차, 와이어 배치
 * 미사용)은 `invokeBatchSettled` 문서 참고.
 */
export type BatchSettledEntry<T> = {
    status: 'fulfilled';
    value: T;
} | {
    status: 'rejected';
    reason: unknown;
} | {
    status: 'unexecuted';
};
/** All first-party adapter factories guarantee the batch surface. */
export type EngineClientWithBatch = EngineClient & {
    invokeBatch<T>(entries: BatchEntry[]): Promise<T[]>;
};
/**
 * invoke 추가 옵션 (T1).
 *
 * `signal` 이 abort 되면 프라미스를 즉시 reject 한다. 네이티브가
 * `invokeAsync`/`invokeCancel` 을 노출하면 취소를 전파(전파는 JS 코덱
 * 경로만; typed/tier3 경로는 얕은 취소)하고, 그렇지 않으면 JS 프라미스만
 * 거부하는 얕은 취소로 폴백한다 — Rust 핸들러는 끝까지 실행된다.
 *
 * **얕은 취소 경고**: 취소/타임아웃의 관측 수준은 transport 에 따라 얕을 수
 * 있다. reject 되어도 Rust 명령은 계속 실행됐거나 이미 완료했을 수 있으므로
 * `retryable: true` ≠ "재실행 안전" — 비멱등 명령을 무조건 재시도하지 말고
 * 필요하면 상태를 재조회한다. 자세한 의미는 (저장소)
 * `docs/rust-api-guide.md` 의 "Timeout, Cancellation, and Retry Semantics"
 * 절(한국어: "타임아웃·취소·재시도 의미") 참고.
 */
export type InvokeOptions = {
    /** (T1) AbortSignal — abort 시 Promise 를 즉시 reject 하고, 네이티브가
     *  invokeAsync/invokeCancel 을 노출하면 취소를 전파한다. 전파가 불가한
     *  경로는 얕은 취소(JS 프라미스만 거부, Rust 실행은 계속)에 해당한다. */
    signal?: AbortSignal;
    /**
     * (프로덕션 준비) 호출별 타임아웃(ms). 만료 시 `transport.timeout`
     * (retryable)으로 reject 한다. 네이티브가 응답하지 않는 hang(워커 패닉,
     * FFI 데드락 등)의 유일한 JS 측 탈출구다. 지각 응답은 무시된다.
     * 만료는 Rust 실행을 중단시키지 않는다 — 위 얕은 취소 경고 참고.
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
    /**
     * `buf` 는 소유 ArrayBuffer 또는 재사용 버퍼의 뷰다 — 디코드 동안에만
     * 유효하다(호스트가 재사용하는 caller-buffer 뷰일 수 있음).
     */
    decode(buf: ArrayBuffer | ArrayBufferView): {
        ok: boolean;
        result?: O;
        error?: RustraError;
    };
};
export { createComplexCodec } from './complex-codec.js';
export type { ComplexCodecOptions, ComplexSchema } from './complex-codec.js';
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
//# sourceMappingURL=public.d.ts.map