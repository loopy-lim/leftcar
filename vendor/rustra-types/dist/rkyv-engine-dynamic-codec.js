import { createSchemaPostcardCodec } from './schema-postcard-codec.js';
import { createComplexCodec } from './complex-codec.js';
export function createDynamicCodecRuntime(schema) {
    const cache = new Map();
    let builtAtEpoch = schema.resyncEpoch;
    const lookupBinaryCodec = (entry) => {
        if (builtAtEpoch !== schema.resyncEpoch) {
            cache.clear();
            builtAtEpoch = schema.resyncEpoch;
        }
        if (cache.has(entry))
            return cache.get(entry);
        const compiled = compileDynamicCodec(entry);
        cache.set(entry, compiled);
        return compiled;
    };
    return {
        lookupBinaryCodec,
        get size() {
            return cache.size;
        },
    };
}
function compileDynamicCodec(entry) {
    const inputSchema = entry.inputSchema;
    const outputSchema = entry.outputSchema;
    if (!inputSchema || !outputSchema)
        return null;
    const definitions = entry.definitions ?? {};
    // 1순위: postcard 인터프리터 — 정적 명령 코드젠 코덱과 바이트 동일(PINNED
    // hex 교차 테스트). 미지원(oneOf/mixed/깊이 초과)이면 null.
    const postcard = createSchemaPostcardCodec(entry.commandId, inputSchema, outputSchema, definitions);
    if (postcard)
        return postcard;
    // 2순위: complex 코덱 — Rust 가 oneOf payload enum 을 complex binary 로
    // 승격하는 것과 동일 판정. createComplexCodec 은 미지원 스키마를 **생성 시
    // throw** 하므로 try 로 잡아 Tier 3 폴백을 유지한다(양쪽 미러 불일치는
    // 안전 실패 — 와이어 오염 없음).
    try {
        return createComplexCodec({
            commandId: entry.commandId,
            inputSchema,
            outputSchema,
            definitions,
        });
    }
    catch {
        return null;
    }
}
//# sourceMappingURL=rkyv-engine-dynamic-codec.js.map