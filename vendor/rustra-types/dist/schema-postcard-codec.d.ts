import type { ComplexSchema } from './complex-codec-types.js';
import type { RkyvV2Codec } from './public.js';
/**
 * live_schema 명령 엔트리로부터 postcard 코덱을 생성한다.
 * 입력/출력 스키마 중 하나라도 postcard 미지원 형태면 null — 호출자(엔진)는
 * 그 명령을 Tier 3(JSON-in-binary)로 폴백한다.
 */
export declare function createSchemaPostcardCodec(commandId: number, inputSchema: ComplexSchema, outputSchema: ComplexSchema, definitions?: Record<string, ComplexSchema>): RkyvV2Codec<unknown, unknown> | null;
//# sourceMappingURL=schema-postcard-codec.d.ts.map