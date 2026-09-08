/**
 * Inspector (B1) — Rust `rustra_ffi_capture_snapshot` 이 노출하는 표준 덤프
 * 스냅샷의 타입과 파서.
 *
 * # experimental
 *
 * 이 모듈과 `DumpedWire` 형태는 **experimental** 이다
 * (docs/versioning-policy.md 실험 표면 표 참조) — 형태 변경은 깨지는
 * 변경이지만, 필드 **추가**는 하위호환으로 취급한다(기존 필드
 * 삭제/이름 변경/타입 변경은 breaking).
 *
 * 스냅샷 blob 은 JSON(UTF-8)이므로 "디코더"는 엄격한 JSON 검증으로 귀결된다:
 * 잘린 바이트·깨진 UTF-8·모양이 다른 문서를 조용히 받아들이지 않고, 위치와
 * 기대값을 밝히는 에러로 크게 실패한다(loud). 와이어 프레임(postcard)이
 * 스냅샷 자체에 포함되지는 않으므로 복잡 코덱 디코더의 재사용은 불필요하고,
 * 필요하면 B2 wire 뷰어(`rustra inspect`)가 그쪽 조립을 담당한다.
 *
 * 정준 골든 바이트는 실제 Rust 캡처에서 나온다:
 * `crates/rustra/tests/fixtures/inspector-golden.hex.txt` — 이 테스트가
 * 소비하는 단일 아티팩트(갱신 절차는 fixture 헤더와
 * crates/rustra/tests/inspector_golden.rs 참조).
 */
/** `DumpedWire.limits` — 현재는 페이로드 크기 한도 하나다(존재하는 개념만 노출). */
export type DumpedWireLimits = {
    /** `rustra_ffi_get_max_payload` 과 동일 값(바이트). */
    maxPayloadBytes: number;
};
/** `DumpedWire.commands` 항목 — 레지스트리 명령의 덤프 표현. */
export type DumpedWireCommand = {
    /** command_id (u16, wire 디스패치에 쓰이는 것과 같은 값). */
    id: number;
    /** 명령 이름. */
    name: string;
    /** `required_capability` — 요구가 없으면 null. */
    capability: string | null;
};
/** `DumpedWire.stats` — 새 계측기 없이 기존 카운터만 노출한다. */
export type DumpedWireStats = {
    /** 레지스트리에 등록된 명령 수 (`commands.length` 와 항상 동일). */
    registeredCommands: number;
    /** 부여된 capability 이름 목록 (deny-by-default 해제 집합). */
    grantedCapabilities: string[];
    /** 이벤트 버스에 대기 중인 이벤트 수. */
    pendingEvents: number;
    /** 버스 용량 초과로 버려진 이벤트 누적 수. */
    droppedEvents: number;
};
/**
 * `rustra_ffi_capture_snapshot` blob (UTF-8 JSON)의 타입화된 형태.
 *
 * 미등록 패키지의 degenerate 스냅샷(`packageId`/`contractHash`/
 * `schemaGeneration` 이 null)도 이 타입으로 표현된다 — `packageId` 가
 * `null` 인지로 미등록 상태를 구분한다.
 */
export type DumpedWire = {
    /** 등록된 패키지 id — 미등록이면 null. */
    packageId: string | null;
    /** 네이티브 계약 해시(SHA-256 hex) — `rustra_ffi_contract_hash` 와 동일 값. 미등록이면 null. */
    contractHash: string | null;
    /** (T0) 스키마 세대 — `rustra_ffi_schema_generation` 과 동일 값. 미등록이면 null. */
    schemaGeneration: number | null;
    /** 레지스트리 명령 목록(id/name/capability). */
    commands: DumpedWireCommand[];
    /** 런타임 한도. */
    limits: DumpedWireLimits;
    /** 기존 카운터의 스냅샷. */
    stats: DumpedWireStats;
};
/**
 * UTF-8 JSON 바이트 또는 문자열 스냅샷을 [`DumpedWire`] 로 엄격하게 검증·파싱한다.
 *
 * 잘린 JSON, 깨진 UTF-8, 모양이 다른 문서(필수 필드 누락/타입 불일치)는
 * `inspector.invalid_snapshot` / `inspector.unexpected_shape` 코드의
 * [`RustraCommandError`] 로 크게 실패한다 — 덤프 도구가 조용히 빈 스냅샷을
 * 렌더하는 것을 막기 위한 계약이다. 카운터·한도·세대 필드는 안전 정수이면서
 * 음수가 아니어야 하고, `commands[].id` 는 추가로 u16 범위여야 한다.
 */
export declare function parseSnapshot(input: Uint8Array | string): DumpedWire;
/**
 * [`parseSnapshot`] 의 역 — 스냅샷을 UTF-8 JSON 바이트로 직렬화한다. 호스트가
 * 덤프 파일을 쓸 때 쓴다(에러 경로 없음 — 스냅샷은 직렬화 가능한 값만 담는다).
 * golden hex 의 정준 소스는 Rust 캡처 fixture 다(이 함수는 그 바이트 계약을
 * 재현하는 보조 수단).
 */
export declare function serializeSnapshot(snapshot: DumpedWire): Uint8Array;
//# sourceMappingURL=inspector.d.ts.map