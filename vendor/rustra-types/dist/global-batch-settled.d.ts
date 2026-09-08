import type { BatchEntry, BatchSettledEntry } from './public.js';
/**
 * allSettled 형태 opt-in batch — 기존 `invokeBatch` 의 reject·순서·fail-fast
 * 정책은 무변경이며, 이 함수는 그 옆에 붙는 별도 표면이다.
 *
 * **실행 정책 — 항상 per-entry, 항상 순차.** transport 가 와이어 배치(단일
 * 횡단 `invokeBatch`)를 지원해도 이 표면은 절대 그걸 쓰지 않는다: 와이어 배치는
 * 원자적 all-or-nothing 의미라 부분 성공 관측이 불가능하다. 부분 성공 관측이
 * 이 API의 목적이므로 per-entry 경로만 유의미하다. 또한 per-entry 실행은
 * `invokeBatch` 폴백(`Promise.all` — 동시 시작)과 달리 **순차**다: 항목 i 가
 * reject 하면 그 이후 항목은 dispatch 자체가 일어나지 않은 채 `unexecuted` 가
 * 된다. 동시 실행이면 reject 시점에 이후 항목이 이미 진행 중이라 "실행 안 됨"
 * 이라는 상태를 정확히 표시할 수 없다 — 이 순차 정책이 이 API의 핵심 가치인
 * "실패≠미실행" 구분을 가능하게 한다.
 *
 * 항목 실행은 단건 경로(`invokeWithTimeout`)와 동일한 장비를 그대로 재사용한다
 * — 항목별 `signal`/`timeoutMs` 존중, dispatch 중 동기 abort 관측(R05), 동기
 * throw 의 rejected 정규화가 모두 그대로 적용된다. per-entry deadline 은
 * 결정적으로 reject 하므로(timeout → `transport.timeout`, abort → `cancelled`)
 * settled 결과에서 "결과 불명" 항목은 존재하지 않는다 — reason 으로 판독한다.
 *
 * 순서는 entry 순서와 동일함이 보장된다. 빈 batch 는 `[]` 로 resolve 한다.
 * 미구성 시 `invokeBatch` 와 동일한 `transport.unavailable` loud-reject, lazy
 * 설정 시 `ensureConfigured` 재진입 후 실행한다.
 */
export declare function invokeBatchSettled<T>(entries: BatchEntry[]): Promise<Array<BatchSettledEntry<T>>>;
//# sourceMappingURL=global-batch-settled.d.ts.map