import type { FrameSchemaNative } from './live-schema.js';
import type { FrameCodec, FrameEngine } from './public.js';
export type { FrameEngineOptions, ContractMismatchDiagnosis } from './frame-engine-options.js';
import type { FrameEngineOptions } from './frame-engine-options.js';
/**
 * Frame 네이티브 모듈로 EngineClient을 생성한다.
 *
 * 정적 명령은 codegen codec registry 로 fast-path(postcard). registry 에 없는
 * 동적(런타임 등록) 명령은 live schema 에서 commandId 를 조회해 Tier 3(JSON) 로
 * fallback 한다. 단일 엔진이 정적 + 동적 모두 처리한다.
 */
export declare function createFrameEngine(native: FrameSchemaNative, registry: Map<string, FrameCodec<unknown, unknown>>, options?: FrameEngineOptions): FrameEngine;
//# sourceMappingURL=frame-engine.d.ts.map