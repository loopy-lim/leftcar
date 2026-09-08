import type { RkyvV2SchemaNative } from './live-schema.js';
import type { RkyvV2Codec, RkyvV2Engine } from './public.js';
export type { RkyvV2EngineOptions, ContractMismatchDiagnosis } from './rkyv-engine-options.js';
import type { RkyvV2EngineOptions } from './rkyv-engine-options.js';
/**
 * rkyv V2 네이티브 모듈로 EngineClient을 생성한다.
 *
 * 정적 명령은 codegen codec registry 로 fast-path(postcard). registry 에 없는
 * 동적(런타임 등록) 명령은 live schema 에서 commandId 를 조회해 Tier 3(JSON) 로
 * fallback 한다. 단일 엔진이 정적 + 동적 모두 처리한다.
 */
export declare function createRkyvV2Engine(native: RkyvV2SchemaNative, registry: Map<string, RkyvV2Codec<unknown, unknown>>, options?: RkyvV2EngineOptions): RkyvV2Engine;
//# sourceMappingURL=rkyv-engine.d.ts.map