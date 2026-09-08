/** Opt-in runtime diagnostics for transport and wire debugging. */
export type RustraDebugEvent = {
    direction: 'request' | 'response' | 'error';
    transport: 'json' | 'rkyv' | 'typed';
    command: string;
    bytes?: string;
    byteLength?: number;
    value?: unknown;
    error?: string;
    /**
     * 계약 밖 진단 어휘의 식별자 — `response.shape`(json-engine 응답 셰이프 경고),
     * `ndjson.unparsed`(@rustra/node) 등 이벤트 종류를 식별한다. debugRustra 는
     * 이벤트를 싱크에 그대로 spread 하므로 선택 필드 추가는 기존 이벤트에 영향을
     * 주지 않는다(non-breaking, additive).
     */
    kind?: string;
    /**
     * `kind`가 붙은 진단 이벤트의 세부 규칙 식별자 — `response.shape` 이벤트에서
     * `double_envelope` / `failed_without_error` / `envelope_missing_payload` /
     * `resolved_error_envelope` 를 구분한다(선택 필드, 위와 동일한 추가).
     */
    reason?: string;
};
export type RustraDebugSink = (event: RustraDebugEvent) => void;
/** Install a bounded diagnostic sink; passing undefined disables it. */
export declare function configureDebug(sink?: RustraDebugSink): void;
/** Returns true for `RUSTRA_DEBUG=1|true|verbose` or the RN global switch. */
export declare function isRustraDebugEnabled(): boolean;
export declare function shouldDumpWire(): boolean;
/**
 * 방향 + 바이트 hex를 stderr로 덤프한다. `RUSTRA_DEBUG` 가 없으면 완전 무음이므로
 * 파이프로 연결된 프로세스에서 폐기되어도 안전하다. 요청/응답 바이트 정합을
 * 눈으로 확인할 때 쓰는 저수준 진단이다(구조화 값은 `configureDebug` 싱크 사용).
 */
export declare function dumpWire(direction: 'request' | 'response' | 'error', bytes: ArrayBuffer | ArrayBufferView): void;
/** @internal — test-only: clears RUSTRA_DEBUG from env and invalidates the dump-gate memo. Not public API. */
export declare function resetDebugEnvForTests(): void;
/** Emit diagnostics only when explicitly enabled; secrets are never logged by default. */
export declare function debugRustra(event: RustraDebugEvent): void;
/** Add a bounded hex preview to an event without retaining the full wire buffer. */
export declare function debugWire(direction: RustraDebugEvent['direction'], transport: RustraDebugEvent['transport'], command: string, bytes: ArrayBuffer | ArrayBufferView, error?: string): void;
/**
 * 와이어 왕복 1점의 진단 총괄 — 구조화 이벤트(debugWire)와 바이트 덤프
 * (dumpWire)를 한 번에 거친다. dispatch 의 3경로(tier2/dynamic/tier3)가
 * 요청·응답 각 점에서 이 헬퍼 하나로 커버를 완결한다(호출 수 감소,
 * dumpWire 누락 경로 제거).
 */
export declare function traceWire(direction: RustraDebugEvent['direction'], command: string, bytes: ArrayBuffer | ArrayBufferView): void;
//# sourceMappingURL=debug.d.ts.map