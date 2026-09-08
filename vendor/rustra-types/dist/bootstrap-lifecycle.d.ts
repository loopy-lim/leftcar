import { RustraCommandError } from './errors.js';
/**
 * bootstrap 수명 상태(A05) — R08 loud-fail 가드 위의 로컬 상태 3종.
 * 모든 어댑터 부트스트랩(node/bun/tauri/react-native)이 이 정의를 공유한다.
 *
 * - `dispose()` 는 멱등 — 두 번째 호출은 no-op.
 * - dispose 뒤의 `ready()` 는 loud-fail(아래 `disposedBootstrapError`).
 * - reload 의 내부 리셋은 사용자 dispose 와 다른 상태 의미를 갖는다: 재초기화가
 *   곧 진행되므로 `initializing` 을 유지해야 한다. `disposed` 로 놓으면
 *   reload 자신의 리셋을 사용자 dispose 로 오판해 좀비·벽돌이 된다.
 * - reload 가 실패하면 상태는 `initializing`(재시도 가능)을 유지하고 원본
 *   에러로 reject 한다 — 절대 `disposed` 로 벽돌화하지 않는다.
 */
export type BootstrapState = 'initializing' | 'ready' | 'disposed';
/** dispose 후 ready 재진입 loud-fail 계약의 공용 에러 — 어댑터별 메시지 접두 포함. */
export declare function disposedBootstrapError(adapter: string, detail?: string): RustraCommandError;
//# sourceMappingURL=bootstrap-lifecycle.d.ts.map