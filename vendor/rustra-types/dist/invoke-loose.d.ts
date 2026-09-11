/**
 * 느슨한 커맨드 호출 — 개발 중 프로토타이핑의 공식 표면 (Dev Tier A절).
 *
 * 새 매커니즘이 아니라 명명된 위임이다 — 엔진의 이름 기반 `invoke`가 정적
 * 코덱 fast-path → live schema commandId 조회 → Tier 3(JSON) 폴백 체인을
 * 이미 판정한다(rkyv 엔진은 `rkyv-engine-async.ts`, json 엔진은 JSON 경로).
 * Rust 핸들러를 debug 빌드에서 런타임 등록한 뒤 코드젠 없이 곧장 호출할 때
 * 쓴다. 정적 승격은 평소처럼 `rustra codegen` — 반환 타입 기본값은
 * `unknown`, 호출자가 좁힌다.
 */
import type { EngineClient, InvokeOptions } from './public.js';
export declare function invokeLoose<T = unknown>(client: EngineClient, command: string, args?: unknown, options?: InvokeOptions): Promise<T>;
//# sourceMappingURL=invoke-loose.d.ts.map