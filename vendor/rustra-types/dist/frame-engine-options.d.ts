/**
 * `createFrameEngine` 옵션. 모두 opt-in 이며 생략 시 하위 호환 동작을 유지한다.
 */
/**
 * (B4) 계약 diff 원인 진단 하나 — `@rustra/cli` `diffSchemas` 의
 * `ContractDiagnosis` 유니언의 **넓어진 구조적 상위 타입**이다 (types 는 leaf
 * 패키지라 cli 를 의존하지 않는다). 모든 변형별 필드가 옵션이라 cli 진단은
 * 항상 이 타입에 대입 가능하지만(단방향), 역방향은 아니다 — 생산자는
 * `diffSchemas` 의 `diagnoses` 를 그대로 실어 보내면 되고, 소비자가 변형별
 * 필드를 정확히 좁히려면 cli 쪽 `ContractDiagnosis` 타입을 참조한다.
 * `detail` 은 사람이 읽는 원인 문장이다.
 */
export type ContractMismatchDiagnosis = {
    code: 'command_id_displaced' | 'alias_missing' | 'wire_type_changed';
    command: string;
    detail: string;
    field?: string;
    from?: string;
    to?: string;
    oldId?: number;
    newId?: number;
    legacyId?: number;
    occupiedBy?: string;
};
export type FrameEngineOptions = {
    /**
     * (F5) 빌드 시점 코드젠이 생성한 계약 해시(`GENERATED_CONTRACT_HASH`).
     * 설정하면 엔진 생성 시 네이티브의 실시간 해시(`getContractHash`)와 비교해
     * 불일치면 즉시 throw 한다 — 생성된 클라이언트와 네이티브 바이너리의 스키마
     * 드리프트를 시작 시점에 잡는다. 미설정 시 검증하지 않는다(기본값).
     * 불일치·미노출 시의 처리는 `contractVerification` 정책을 따른다.
     */
    contractHash?: string;
    /**
     * (A2, OTA) 계약 검증 정책 — `contractHash` 가 설정된 경우 네이티브 해시
     * 대조의 실패 처리를 고른다. 미설정(기본값)은 기존 동작 그대로다.
     *
     * - `undefined`(기본값) / `'strict'` — fail-fast. 불일치 시
     *   onContractMismatch 콜백(설정 시)을 호출한 뒤 degraded 모드로 계속
     *   생성하고, 콜백이 없으면 `contract.mismatch` 를 throw 한다.
     *   `getContractHash` 미노출 네이티브는 검증 가능한 것이 아무것도 없어
     *   정책·콜백과 무관하게 `contract.unenforceable` 을 throw 한다.
     * - `'warn'` — OTA/degraded 배포 탈출구. 구 JS + 신 네이티브(또는 그 반대)
     *   조합이 걸린 OTA 롤아웃에서 앱 전체 마비 대신 부분 동작을 택한다.
     *   불일치 시 콜백(설정 시) 또는 console.warn 으로 경고하고 **엔진은 항상
     *   생성한다**(throw 없음). `getContractHash` 미노출도 throw 대신
     *   console.warn 으로 강등한다 — 경고 기능은 절대 치명적이지 않다는
     *   schemaVersion stale 검사와 같은 non-fatal 계약이다.
     * - `'off'` — 검증 자체를 생략한다. `contractHash` 가 설정돼 있어도 네이티브
     *   해시를 읽지 않는다(미노출 판정조차 하지 않는다). 생성된 호스트 엔트리의
     *   이 값을 한 줄 바꾸는 것이 코드젠 재생성 전까지 유효한 공식 탈출구다.
     *
     * `contractHash` 가 미설정이면 정책과 무관하게 검증하지 않는다(기존 계약 —
     * 정책은 검증의 강도를 고르는 노브이지 검증을 켜는 스위치가 아니다).
     *
     * (한계) 이 정책은 네이티브 Frame 엔진(`createFrameEngine`) 경로에만
     * 적용된다. JSON 엔진(`createJsonEngine`)은 transport 클로저만 받는 순수 JS
     * 경로라 네이티브 핸들·`getContractHash` 가 없어 계약 해시 검증 자체가 이
     * 경로에는 존재하지 않는다 — Node JSON 부트스트랩은 `@rustra/node` 의 스폰
     * handshake(`__rustra_contract` 엔드포인트)에서 별도로 검증한다.
     */
    contractVerification?: 'strict' | 'warn' | 'off';
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
     *
     * (B4) `info.diagnosis.diagnoses` 는 계약 diff 원인 진단 목록이다 —
     * `@rustra/cli` 의 `diffSchemas(old, live)` 가 만드는 `ContractDiagnosis` 가
     * 대입되는 상위 타입(위 `ContractMismatchDiagnosis` 참조). 엔진 자체는
     * mismatch 시점에 live schema 만 있고 빌드 시점 스키마가 없어 diff 를 계산할
     * 수 없으므로 네이티브 코어는 이 필드를 채우지 않는다 (undefined). 생산자가
     * 채우는 경로: 빌드 시점 schema.json 을 들고 있는 호스트(예: OTA 래퍼, dev
     * 서버)가 mismatch 를 감지하면 diffSchemas 를 돌려 `diagnoses` 배열을 이
     * 필드로 전달한다 — 콜백 소비자는
     * `info.diagnosis?.diagnoses.map((d) => d.detail)` 로 원인 문장을 읽는다.
     */
    onContractMismatch?: (info: {
        nativeHash: string;
        expectedHash: string;
        diagnosis?: {
            diagnoses: ContractMismatchDiagnosis[];
        };
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
//# sourceMappingURL=frame-engine-options.d.ts.map