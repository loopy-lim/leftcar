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
import { RustraCommandError, RustraErrorCode } from './errors.js';
import { decodeUtf8, encodeUtf8 } from './utf8.js';
/** 모양 불일치는 JSON 포인터 경로로 밝힌다(loud 계약). */
function unexpectedShape(path, expected, actual) {
    const shown = actual === null ? 'null' : Array.isArray(actual) ? 'array' : typeof actual;
    throw new RustraCommandError(RustraErrorCode.InspectorUnexpectedShape, `snapshot '${path}' must be ${expected}, got ${shown}`);
}
function expectString(path, value) {
    if (typeof value !== 'string')
        unexpectedShape(path, 'a string', value);
    return value;
}
/**
 * 카운터/한도/세대 필드용 — 안전 정수 범위이면서 음수가 아닌 값만 통과한다.
 * 스냅샷의 수치는 전부 Rust 부호 없는 정수(u16/u64/usize)이므로 음수·소수는
 * 문서 손상 신호다.
 */
function expectCounter(path, value) {
    if (typeof value !== 'number' || !Number.isFinite(value)) {
        unexpectedShape(path, 'a finite number', value);
    }
    if (!Number.isSafeInteger(value) || value < 0) {
        throw new RustraCommandError(RustraErrorCode.InspectorUnexpectedShape, `snapshot '${path}' must be a non-negative safe integer, got ${value}`);
    }
    return value;
}
function expectArray(path, value) {
    if (!Array.isArray(value))
        unexpectedShape(path, 'an array', value);
    return value;
}
function expectObject(path, value) {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
        unexpectedShape(path, 'an object', value);
    }
    return value;
}
/**
 * UTF-8 JSON 바이트 또는 문자열 스냅샷을 [`DumpedWire`] 로 엄격하게 검증·파싱한다.
 *
 * 잘린 JSON, 깨진 UTF-8, 모양이 다른 문서(필수 필드 누락/타입 불일치)는
 * `inspector.invalid_snapshot` / `inspector.unexpected_shape` 코드의
 * [`RustraCommandError`] 로 크게 실패한다 — 덤프 도구가 조용히 빈 스냅샷을
 * 렌더하는 것을 막기 위한 계약이다. 카운터·한도·세대 필드는 안전 정수이면서
 * 음수가 아니어야 하고, `commands[].id` 는 추가로 u16 범위여야 한다.
 */
export function parseSnapshot(input) {
    // decodeUtf8 은 throw 하지 않고 U+FFFD 로 대체하므로(Hermes-safe 관례) 깨진
    // UTF-8 은 JSON 파싱 단계에서 malformed 로 귀결된다 — 별도 UTF-8 검증 불필요.
    const text = typeof input === 'string' ? input : decodeUtf8(input, 0, input.length);
    let raw;
    try {
        raw = JSON.parse(text);
    }
    catch (error) {
        // JSON.parse 의 위치 정보(V8: "… in JSON at position 42")를 살린다 —
        // 위치를 알려주지 않는 엔진(Bun/JSC: "Unterminated string")은 문구만
        // 전달된다. 어느 쪽이든 조용히 통과하지는 않는다(loud 계약).
        const position = /position (\d+)/.exec(error instanceof Error ? error.message : '');
        throw new RustraCommandError(RustraErrorCode.InspectorInvalidSnapshot, `invalid snapshot: truncated or malformed JSON${position
            ? ` at byte ${position[1]}`
            : ` (${error instanceof Error ? error.message : 'parse error'})`}`, false, error);
    }
    return validateDocument(expectObject('package', raw));
}
/** 파싱된 문서를 DumpedWire 로 좁힌다 — 모든 필드를 검증하고 모양 불일치는 loud. */
function validateDocument(raw) {
    const commandsRaw = expectArray('commands', raw.commands);
    const commands = commandsRaw.map((item, index) => {
        const command = expectObject(`commands[${index}]`, item);
        const id = expectCounter(`commands[${index}].id`, command.id);
        if (id > 0xffff) {
            throw new RustraCommandError(RustraErrorCode.InspectorUnexpectedShape, `snapshot 'commands[${index}].id' must be a u16 integer, got ${id}`);
        }
        const capability = command.capability;
        if (capability !== null && typeof capability !== 'string') {
            unexpectedShape(`commands[${index}].capability`, 'a string or null', capability);
        }
        return {
            id,
            name: expectString(`commands[${index}].name`, command.name),
            capability,
        };
    });
    const limits = expectObject('limits', raw.limits);
    const stats = expectObject('stats', raw.stats);
    const granted = expectArray('stats.grantedCapabilities', stats.grantedCapabilities);
    const generation = raw.schemaGeneration;
    if (generation !== null && typeof generation !== 'number') {
        unexpectedShape('schemaGeneration', 'a number or null', generation);
    }
    if (typeof generation === 'number') {
        // 세대는 u64 — 부호 없는 안전 정수 범위만 통과한다.
        expectCounter('schemaGeneration', generation);
    }
    const contractHash = raw.contractHash;
    if (contractHash !== null && typeof contractHash !== 'string') {
        unexpectedShape('contractHash', 'a string or null', contractHash);
    }
    const packageId = raw.packageId;
    if (packageId !== null && typeof packageId !== 'string') {
        unexpectedShape('packageId', 'a string or null', packageId);
    }
    const dumped = {
        packageId,
        contractHash,
        schemaGeneration: generation,
        commands,
        limits: {
            maxPayloadBytes: expectCounter('limits.maxPayloadBytes', limits.maxPayloadBytes),
        },
        stats: {
            registeredCommands: expectCounter('stats.registeredCommands', stats.registeredCommands),
            grantedCapabilities: granted.map((cap, index) => expectString(`stats.grantedCapabilities[${index}]`, cap)),
            pendingEvents: expectCounter('stats.pendingEvents', stats.pendingEvents),
            droppedEvents: expectCounter('stats.droppedEvents', stats.droppedEvents),
        },
    };
    // 정합 불변식 — 등록 명령 수는 명령 배열 길이와 항상 같다 (Rust 조립자의
    // 단일 read-lock 판독이 보장하는 것). 어긋나면 문서가 손상됐다는 뜻이다.
    if (dumped.stats.registeredCommands !== commands.length) {
        throw new RustraCommandError(RustraErrorCode.InspectorUnexpectedShape, `snapshot 'stats.registeredCommands' (${dumped.stats.registeredCommands}) must equal 'commands.length' (${commands.length})`);
    }
    return dumped;
}
/**
 * [`parseSnapshot`] 의 역 — 스냅샷을 UTF-8 JSON 바이트로 직렬화한다. 호스트가
 * 덤프 파일을 쓸 때 쓴다(에러 경로 없음 — 스냅샷은 직렬화 가능한 값만 담는다).
 * golden hex 의 정준 소스는 Rust 캡처 fixture 다(이 함수는 그 바이트 계약을
 * 재현하는 보조 수단).
 */
export function serializeSnapshot(snapshot) {
    return encodeUtf8(JSON.stringify(snapshot));
}
//# sourceMappingURL=inspector.js.map