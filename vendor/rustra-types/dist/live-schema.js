import { RustraCommandError } from './errors.js';
import { decodeUtf8 } from './utf8.js';
/** getLiveSchema 의 파싱 내부 — 엔진 생성 시 schemaVersion 까지 읽는다 (T2). */
export function parseLiveSchemaDocument(native) {
    if (!native.getSchema) {
        // (의미론 마감) 네이티브가 getSchema 를 노출하지 않으면 live schema 자체를
        // 얻을 수 없다 — 빈 Map 을 돌려주면 Tier 3 동적 명령이 command.not_found 로
        // 오해받는다. 스키마 조회가 실제로 필요한 호출자가 즉시 실패하도록 명시적
        // 에러를 던진다 (엔진 생성 시 schemaVersion 협상은 선택적이라 try/catch 로
        // 이미 흡수된다).
        throw new RustraCommandError('schema.unavailable', 'native module does not expose getSchema(); live schema is unavailable');
    }
    const bytes = native.getSchema();
    const u = new Uint8Array(bytes);
    const json = decodeUtf8(u, 0, u.length);
    const parsed = JSON.parse(json);
    const map = new Map();
    for (const c of parsed.commands ?? []) {
        map.set(c.name, {
            commandId: c.commandId,
            inputSchema: c.inputSchema,
            outputSchema: c.outputSchema,
            definitions: c.definitions,
        });
    }
    const doc = { commands: map };
    if (typeof parsed.schemaVersion === 'number' && Number.isFinite(parsed.schemaVersion)) {
        doc.schemaVersion = parsed.schemaVersion;
    }
    if (typeof parsed.schemaGeneration === 'number' && Number.isFinite(parsed.schemaGeneration)) {
        doc.schemaGeneration = parsed.schemaGeneration;
    }
    return doc;
}
/**
 * 네이티브 getSchema() 로부터 현재 명령 스키마를 조회한다 (정적 + 동적 명령 포함).
 * 동적 명령의 commandId/타입을 알아내 rkyvV2 Tier 3 fallback 에 사용된다.
 * getSchema 미노출 네이티브에서는 schema.unavailable 에러를 던진다.
 */
export function getLiveSchema(native) {
    return parseLiveSchemaDocument(native).commands;
}
//# sourceMappingURL=live-schema.js.map