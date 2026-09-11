/**
 * 디바이스 역량 조회 표면 (디바이스 역량 계약 레이어 D절).
 *
 * 선언(`#[command(device(...))]`)과 조회는 rustra 몫, OS 권한 요청은 호스트
 * 몫 — 이 모듈은 호스트가 단일 provider 를 등록하고 JS 코드가 역량의
 * 가용성/권한을 물을 수 있는 표면만 제공한다. W3C Permissions API 처럼
 * request() 는 없다: 요청은 point-of-use 에서 호스트가 한다.
 */
const state = (() => {
    const key = Symbol.for('dev.rustra.types.v0.4.0.deviceStatusState');
    const global = globalThis;
    const existing = global[key];
    if (existing)
        return existing;
    const value = { noProviderWarned: false };
    global[key] = value;
    return value;
})();
/**
 * 전역 1-provider 를 등록한다. 재등록은 마지막이 승리한다(configure 관례).
 * 등록 해제(unregister) 표면은 의도적으로 없다 — 교체는 재등록으로 충분하고
 * 해제 수요가 증명되기 전엔 YAGNI. 파생 provider 팩토리(Tauri 플러그인/RN
 * 라이브러리 매핑)는 후속 트랙.
 */
export function registerDeviceStatusProvider(provider) {
    state.provider = provider;
}
/** fail-open 기본값 — 매 조회 새 객체로 반환해 호출자의 mutation 이 슬롯에 스미지 않게 한다. */
function unknownStatus() {
    return { availability: 'unknown', permission: 'unknown' };
}
/** debug.ts 진단 관례 — console.debug 미지원 환경은 console.log 폴백. */
function logDeviceDiagnostic(message, error) {
    const logger = typeof console.debug === 'function' ? console.debug : console.log;
    logger('[rustra:device]', message, ...(error === undefined ? [] : [error]));
}
/**
 * 역량의 가용성/권한을 조회한다.
 *
 * - provider 미등록: fail-open — `{availability:'unknown', permission:'unknown'}`
 *   반환 + debug 경고 1회(모듈당 플래그). 호스트가 등록하지 않아도 앱은
 *   깨지지 않고 안내만 된다.
 * - provider 예외: 전파하지 않고 fail-open 결과로 회복한다. 조회 API 가
 *   호출자의 제어 흐름을 거부로 바꾸는 부수효과를 만들지 않기 위함이고,
 *   회복이 조용하면 고장 provider 가 보이지 않으므로 예외는 debug 진단으로
 *   관측한다. 미등록 경고와 달리 매 회 관측한다 — 발생마다 원인 에러가 다르다.
 */
export async function getDeviceStatus(capability) {
    const provider = state.provider;
    if (!provider) {
        if (!state.noProviderWarned) {
            state.noProviderWarned = true;
            logDeviceDiagnostic(`no DeviceStatusProvider registered — returning unknown status for '${capability}'. ` +
                `Call registerDeviceStatusProvider() from the host app to provide device availability/permission.`);
        }
        return unknownStatus();
    }
    try {
        return await provider(capability);
    }
    catch (error) {
        logDeviceDiagnostic(`DeviceStatusProvider threw for '${capability}' — returning unknown status.`, error);
        return unknownStatus();
    }
}
/** @internal — test-only: provider 슬롯과 경고 플래그를 초기화. Not public API. */
export function resetDeviceStatusForTests() {
    state.provider = undefined;
    state.noProviderWarned = false;
}
//# sourceMappingURL=device-status.js.map