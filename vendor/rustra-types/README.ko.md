# @rustra/types

rustra-bridge의 핵심 타입 패키지입니다. 모든 플랫폼 어댑터(Node, Bun, Tauri,
React Native)가 공유하는 `EngineClient` 인터페이스, 에러 타입, rkyv V2
코덱, Tauri-like 글로벌 invoke를 제공합니다.

## 공개 API 개요

```ts
// 플랫폼별 엔진을 한 번만 설정
import { configure } from '@rustra/types';
import { createRkyvV2Engine } from '@rustra/react-native';
configure(createRkyvV2Engine(native, registry));

// 어디서든 타입 안전 호출 (generated 클라이언트 내부에서 사용)
import { addNumbers } from './generated/commands.js';
const result = await addNumbers({ a: 42, b: 58 });
```

주요 익스포트:

- `EngineClient` — `invoke<T>()` (+선택적 `invokeBatch`) 공통 인터페이스
- `configure()` / `invoke()` — 글로벌 invoke (Tauri-like 단일 진입점)
- `InvokeOptions.signal` — AbortSignal — abort 시 프라미스 즉시 거부 + 네이티브
  취소 전파(`invokeAsync`/`invokeCancel` 노출 시), 에러 코드 `cancelled`
- `invokeBatch()` / `invokeBatchSettled()` — 배치 호출. settled 형태는 항상 per-entry
  순차 실행(원자적 와이어 배치 미사용)이며 항목별로 `fulfilled` / `rejected` /
  `unexecuted` 를 보고해, 실패한 항목과 그 뒤에 dispatch 되지 않은 항목을 구별한다
  (`docs/rust-api-guide.ko.md` "타임아웃·취소·재시도 의미" 절 참고)
- `withRetry(fn, options?)` — retryable 실패에 한한 지수 백오프 재시도(`retries` 기본 2,
  `baseDelayMs` 기본 100, `retryIf` 는 기본 판정을 대체, `signal` abort 는 백오프
  중에도 즉시 `CancelledError`); 마지막 에러는 원본 그대로 재던진다
- `configureDebug(sink)` / `RustraDebugEvent` — opt-in 구조화 진단 싱크.
  `RUSTRA_DEBUG=1|true|verbose` 면 추가로 모든 이벤트를 `[rustra:debug]` 로 로그하고
  와이어 바이트를 hex 로 stderr 에 덤프한다(`[rustra:wire]`); React Native 에서는
  `globalThis.__RUSTRA_DEBUG__ = true` 가 이벤트 로그를 켠다. 진단 이벤트는 선택
  필드 `kind`/`reason` 을 싣는다(JSON 엔진의 `response.shape`, `@rustra/node` 의
  `ndjson.unparsed`)
- `RustraCommandError` — 직렬화 가능 에러 + `parseRustraErrorString`
- rkyv V2 코덱 — Rust `invoke_rkyv_v2` 왕복용 pure-JS 인코더/디코더
- `contractHash` 검증 — 빌드 시 계약과 런타임 계약 일치 확인
- `RkyvV2EngineOptions` — 엔진 옵션: `onContractMismatch`(해시 불일치 시
  degraded 모드 opt-in), `schemaVersion`/`onSchemaStale`(JS > native stale 경고),
  `maxPayloadBytes`(인코딩 직후 페이로드 크기 사전 검사)
- `invokeLoose()` — 생성 클라이언트 없이 이름으로 호출하는 동적 개발 티어 표면
- `registerDeviceStatusProvider()` / `getDeviceStatus()` — 호스트 등록 provider 기반
  fail-open 디바이스 가용성/권한 조회

## 관련 문서

- [rustra-bridge](https://github.com/loopy-lim/rustra#readme)
- `docs/architecture.md`, `docs/compatibility-contract.md`
