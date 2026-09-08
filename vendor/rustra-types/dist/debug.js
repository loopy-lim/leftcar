let configuredSink;
/** Install a bounded diagnostic sink; passing undefined disables it. */
export function configureDebug(sink) {
    configuredSink = sink;
}
/** Returns true for `RUSTRA_DEBUG=1|true|verbose` or the RN global switch. */
export function isRustraDebugEnabled() {
    const globalValue = globalThis.__RUSTRA_DEBUG__;
    if (globalValue === true)
        return true;
    return shouldDumpWire();
}
/**
 * 순수 env 파싱 — `RUSTRA_DEBUG` 만을 검사한다(RN 글로벌 스위치 불포함).
 * 와이어 바이트 덤프(dumpWire)의 게이트로, 모듈 레벨에서 1회 메모이즈한다
 * (호출당 env 객체 접근 제거). `resetDebugEnvForTests` 가 캐시를 무효화한다.
 */
let cachedShouldDump;
export function shouldDumpWire() {
    if (cachedShouldDump === undefined) {
        const env = globalThis.process
            ?.env;
        cachedShouldDump = ['1', 'true', 'verbose'].includes(String(env?.RUSTRA_DEBUG ?? '').toLowerCase());
    }
    return cachedShouldDump;
}
/**
 * 방향 + 바이트 hex를 stderr로 덤프한다. `RUSTRA_DEBUG` 가 없으면 완전 무음이므로
 * 파이프로 연결된 프로세스에서 폐기되어도 안전하다. 요청/응답 바이트 정합을
 * 눈으로 확인할 때 쓰는 저수준 진단이다(구조화 값은 `configureDebug` 싱크 사용).
 */
export function dumpWire(direction, bytes) {
    if (!shouldDumpWire())
        return;
    // caller-buffer 경로(F2)는 소유 ArrayBuffer 대신 뷰를 넘긴다 — 같은 정합을 지원.
    const view = bytes instanceof ArrayBuffer
        ? new Uint8Array(bytes)
        : new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const hex = Array.from(view.subarray(0, 256), (byte) => byte.toString(16).padStart(2, '0')).join('');
    const truncated = view.byteLength > 256 ? `… (${view.byteLength} bytes total)` : '';
    process.stderr.write(`[rustra:wire] ${direction} ${hex}${truncated}\n`);
}
/** @internal — test-only: clears RUSTRA_DEBUG from env and invalidates the dump-gate memo. Not public API. */
export function resetDebugEnvForTests() {
    const env = globalThis.process
        ?.env;
    delete env?.RUSTRA_DEBUG;
    cachedShouldDump = undefined;
}
/** Emit diagnostics only when explicitly enabled; secrets are never logged by default. */
export function debugRustra(event) {
    if (!isRustraDebugEnabled() && !configuredSink)
        return;
    const safeEvent = {
        ...event,
        value: event.value === undefined ? undefined : snapshot(event.value),
    };
    configuredSink?.(safeEvent);
    if (isRustraDebugEnabled()) {
        const logger = typeof console.debug === 'function' ? console.debug : console.log;
        logger('[rustra:debug]', safeEvent);
    }
}
/** Add a bounded hex preview to an event without retaining the full wire buffer. */
export function debugWire(direction, transport, command, bytes, error) {
    const view = bytes instanceof ArrayBuffer
        ? new Uint8Array(bytes)
        : new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const preview = Array.from(view.subarray(0, 128), (byte) => byte.toString(16).padStart(2, '0')).join('');
    debugRustra({
        direction,
        transport,
        command,
        bytes: preview,
        byteLength: view.byteLength,
        ...(error ? { error } : {}),
    });
}
/**
 * 와이어 왕복 1점의 진단 총괄 — 구조화 이벤트(debugWire)와 바이트 덤프
 * (dumpWire)를 한 번에 거친다. dispatch 의 3경로(tier2/dynamic/tier3)가
 * 요청·응답 각 점에서 이 헬퍼 하나로 커버를 완결한다(호출 수 감소,
 * dumpWire 누락 경로 제거).
 */
export function traceWire(direction, command, bytes) {
    debugWire(direction, 'rkyv', command, bytes);
    dumpWire(direction, bytes);
}
function snapshot(value, depth = 0, budget = { remaining: 2048 }) {
    if (budget.remaining <= 0)
        return '[truncated]';
    if (value === null ||
        typeof value === 'string' ||
        typeof value === 'number' ||
        typeof value === 'boolean') {
        budget.remaining -= typeof value === 'string' ? value.length : 8;
        return value;
    }
    if (typeof value === 'bigint')
        return `${value}n`;
    if (typeof value === 'undefined')
        return undefined;
    if (depth >= 3)
        return '[depth limit]';
    if (Array.isArray(value))
        return value.slice(0, 32).map((item) => snapshot(item, depth + 1, budget));
    if (typeof value === 'object') {
        const result = {};
        for (const [key, item] of Object.entries(value).slice(0, 32))
            result[key] = snapshot(item, depth + 1, budget);
        return result;
    }
    return `[${typeof value}]`;
}
//# sourceMappingURL=debug.js.map