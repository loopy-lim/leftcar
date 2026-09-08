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
    constructor(code: string, message: string, retryable?: boolean, cause?: unknown);
}
/**
 * `transport.timeout` 전용 서브클래스 — `err instanceof TimeoutError` 로
 * 타임아웃 분기(재시도/백오프)를 문자열 비교 없이 할 수 있다.
 * 코드 매핑은 기존 와이어 값(`transport.timeout`)을 그대로 유지한다.
 */
export declare class TimeoutError extends RustraCommandError {
    constructor(message?: string, cause?: unknown);
}
/**
 * `cancelled` 전용 서브클래스 — 협력적 취소(AbortSignal 등)와 구분되는
 * catch 분기를 제공한다. 코드 매핑은 기존 와이어 값(`cancelled`) 유지.
 */
export declare class CancelledError extends RustraCommandError {
    constructor(message?: string, cause?: unknown);
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
 * Adapter/transport 경계에서 들어온 reject 값을 하나의 RustraCommandError로
 * 정규화한다. Promise rejection은 동기 throw와 달리 바깥 try/catch를 우회하므로
 * 모든 호스트가 이 helper를 `.catch()` 경로에도 사용해야 retryable 플래그가
 * JSON 와이어에서 유실되지 않는다.
 */
export declare function normalizeRustraError(error: unknown): RustraCommandError;
/**
 * 코드 기반 retryable 추론 — Rust `RustraError` 팩토리 관례와 정합.
 * `transport.error`/`transport.timeout`은 Rust 생성 시점에 `retryable: true`로
 * 설정되는 코드군이며 (구조화 와이어에는 retryable 플래그가 없으므로 코드에서
 * 도출한다), `cancelled`도 Rust `RustraError::cancelled` 의 retryable:true 를
 * 미러링한다 (T1 — JSON fallback 경로의 취소 에러 정합).
 */
export declare function isRetryableCode(code: string): boolean;
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
    /**
     * 이벤트 전달 불가 — transport 가 drainEvents(폴링)와 onPushEvent(푸시)를
     * 모두 노출하지 않을 때(node-events.ts loud-fail), 폴링 간격 환경변수가
     * 음수/비숫자일 때(node 전용, node-events.ts), 또는 RN 네이티브 모듈이
     * onEvent를 노출하지 않을 때(react-native-events.ts). JS 어댑터 측 전용
     * 코드 — Rust error.rs 에 대응 값이 없다.
     */
    readonly EventUnavailable: "event.unavailable";
    /** 계약 해시 불일치(JS>native stale). */
    readonly ContractMismatch: "contract.mismatch";
    /** 계약 해시 검증 불가(네이티브 미지원). */
    readonly ContractUnenforceable: "contract.unenforceable";
    /**
     * 인스펙터 스냅샷 JSON이 잘렸거나 파싱 불가 (experimental).
     * 파서 측 전용 코드 — Rust error.rs 에 대응 값이 없다(`rustra_ffi_capture_snapshot`
     * 은 잘못된 스냅샷을 만들지 않는다. 이 코드는 TS 디코더의 loud-fail 계약).
     */
    readonly InspectorInvalidSnapshot: "inspector.invalid_snapshot";
    /**
     * 인스펙터 스냅샷 JSON은 유효하지만 기대한 모양이 아님 (experimental).
     * 파서 측 전용 코드 — Rust error.rs 에 대응 값이 없다(위와 동일 사유).
     */
    readonly InspectorUnexpectedShape: "inspector.unexpected_shape";
    /** 분류 불가 오류. */
    readonly Unknown: "unknown";
};
export type RustraErrorCodeValue = (typeof RustraErrorCode)[keyof typeof RustraErrorCode];
/** 값이 알려진 rustra 에러 코드인지 검사 (타입 가드). */
export declare function isRustraErrorCode(code: string): code is RustraErrorCodeValue;
//# sourceMappingURL=errors.d.ts.map