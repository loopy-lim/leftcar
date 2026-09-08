import type { RkyvV2Codec } from './public.js';
import type { LiveSchemaEntry } from './live-schema.js';
import type { RkyvSchemaRuntime } from './rkyv-engine-context.js';
/**
 * (T2-3) 동적 명령 binary 코덱 캐시 — Rust registry 의 3-way 판정
 * (command_build.rs)을 JS 엔진이 미러한다:
 *
 *   1. postcard 지원 스키마 → `createSchemaPostcardCodec` 인터프리터
 *      (Rust `js_codec_supported` 라우트와 동일 와이어).
 *   2. postcard 미지원이지만 complex 지원(oneOf payload enum 등) →
 *      `createComplexCodec` (Rust complex binary 라우트와 동일 와이어 —
 *      dynamic_oneof_schema_gets_complex_binary_handler 계약).
 *   3. 둘 다 거부(anyOf 3항 untagged 등) → null — 호출자가 기존 Tier 3
 *      (JSON-in-binary) 경로로 폴백한다. Rust `rkyv_v2_tier3=true` 와 정합.
 *
 * 캐시는 **entry 객체 식별**으로 무효화한다. generation 게이트(T0-3)가 live
 * schema 를 재조회하면 entry 도 새 객체가 되어 compute-if-absent 가 자연히
 * 새 스키마를 다시 판정한다 — replace/unregister 로 스키마 형태가 바뀌어도
 * 캐시가 스테일 와이어를 내지 않는다. 또한 조회마다 스키마 런타임의
 * `resyncEpoch` 를 확인해 세대가 바뀐(에포크가 오른) 경우 이전 세대의
 * 구 entry 코덱을 통째로 비운다 — dev 치환 반복에서 구 코덱이 누적되지
 * 않는다(강한 참조 맵이므로 GC 만으로는 회수되지 않는다).
 */
export type DynamicCodecRuntime = {
    /** 동적 명령의 binary 코덱 — postcard/complex 순 판정, 미지원 null. */
    lookupBinaryCodec(entry: LiveSchemaEntry): RkyvV2Codec<unknown, unknown> | null;
    /** 캐시된 코덱 수 — 세대 prune 검증용 관찰 지점. */
    readonly size: number;
};
export declare function createDynamicCodecRuntime(schema: RkyvSchemaRuntime): DynamicCodecRuntime;
//# sourceMappingURL=rkyv-engine-dynamic-codec.d.ts.map