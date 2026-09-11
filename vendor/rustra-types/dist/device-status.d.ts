/**
 * 디바이스 역량 조회 표면 (디바이스 역량 계약 레이어 D절).
 *
 * 선언(`#[command(device(...))]`)과 조회는 rustra 몫, OS 권한 요청은 호스트
 * 몫 — 이 모듈은 호스트가 단일 provider 를 등록하고 JS 코드가 역량의
 * 가용성/권한을 물을 수 있는 표면만 제공한다. W3C Permissions API 처럼
 * request() 는 없다: 요청은 point-of-use 에서 호스트가 한다.
 */
/** 역량의 물리적/OS 수준 유무 — `unknown` 은 provider 가 결론을 내리지 못한 것. */
export type DeviceAvailability = 'available' | 'unavailable' | 'unknown';
/** 역량에 대한 권한 상태 — W3C PermissionState 어휘(`prompt` = 아직 물은 적 없음). */
export type DevicePermission = 'granted' | 'denied' | 'prompt' | 'unknown';
/** 역량 조회 1건의 결과 — 가용성 축과 권한 축은 독립이다(같은 OS 안에서도 갈린다). */
export type DeviceStatus = {
    availability: DeviceAvailability;
    permission: DevicePermission;
};
/**
 * 호스트가 제공하는 디바이스 상태 조회 함수. 카탈로그 검증은 Rust 계층
 * 몫이므로 JS 표면은 개방된 string 을 받는다(카탈로그 밖 토큰도 그대로 전달).
 */
export type DeviceStatusProvider = (capability: string) => DeviceStatus | Promise<DeviceStatus>;
/**
 * 전역 1-provider 를 등록한다. 재등록은 마지막이 승리한다(configure 관례).
 * 등록 해제(unregister) 표면은 의도적으로 없다 — 교체는 재등록으로 충분하고
 * 해제 수요가 증명되기 전엔 YAGNI. 파생 provider 팩토리(Tauri 플러그인/RN
 * 라이브러리 매핑)는 후속 트랙.
 */
export declare function registerDeviceStatusProvider(provider: DeviceStatusProvider): void;
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
export declare function getDeviceStatus(capability: string): Promise<DeviceStatus>;
/** @internal — test-only: provider 슬롯과 경고 플래그를 초기화. Not public API. */
export declare function resetDeviceStatusForTests(): void;
//# sourceMappingURL=device-status.d.ts.map