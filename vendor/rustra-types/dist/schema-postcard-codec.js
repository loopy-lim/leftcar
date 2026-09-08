// ── T2-2: 스키마→postcard 코덱 인터프리터 ────────────────────
//
// live_schema 의 inputSchema/outputSchema(JSON Schema 노드)로부터 런타임에
// RkyvV2Codec 을 생성한다 — 코드젠(@rustra/cli)이 하는 일을 스키마 인터프리터로
// 재현해, **동적 명령**(register/replace)도 postcard fast-path 를 쓸 수 있게 한다.
//
// 와이어 패리티 계약:
//   - 이 모듈의 헬퍼(_scEncode*/_scDecode*)는 코드젠이 생성 파일에 인라인하는
//     `_pc*` 헬퍼(postcardPrimitive/Wide/Text/FloatSource)와 동일 알고리즘이다.
//     중복 구현이 아니라 단일 알고리즘의 두 번째 서빙 지점이며, 바이트 동일성은
//     PINNED hex(wire_fixtures.rs 와 공유) 교차 테스트로 고정한다.
//   - 필드 판정은 @rustra/cli classifyPostcardField 의 미러다(Rust 측
//     js_postcard_codec_supported_with_defs 도 동일 판정). 미러 간 불일치는
//     Tier 3 로 안전하게 실패한다 — 와이어 오염이 없다.
//   - 미지원 노드는 null 을 반환하고, 엔진은 그 명령을 Tier 3(JSON) 로 폴백한다.
//
// 지원 형태: zigzag/uvar(32bit), zigzag64/uvar64, f32/f64, bool, String,
// bytes(Vec<u8>), Vec/set 원시, 원시값 map, tuple(2..), string enum,
// struct(재귀 $ref 포함), Option. 미지원: payload enum(oneOf — Rust 는
// complex 라우트로 승격하므로 JS 인터프리터도 만들지 않는다), 3항 anyOf,
// 혼합 object, uniqueItems string set 등.
import { compileNode } from './schema-postcard-node.js';
import { concatBytes, decString } from './schema-postcard-wire.js';
function decodeErrorFrame(u8, view) {
    const errLen = view.getUint16(8, true);
    let err = { code: 'invoke.failed', message: 'invoke failed' };
    if (errLen > 0) {
        // postcard({ code: String, message: String })
        const c = decString(u8, 10);
        const m = decString(u8, 10 + c.bytesRead);
        err = { code: c.value, message: m.value };
    }
    return { ok: false, error: err };
}
/**
 * live_schema 명령 엔트리로부터 postcard 코덱을 생성한다.
 * 입력/출력 스키마 중 하나라도 postcard 미지원 형태면 null — 호출자(엔진)는
 * 그 명령을 Tier 3(JSON-in-binary)로 폴백한다.
 */
export function createSchemaPostcardCodec(commandId, inputSchema, outputSchema, definitions = {}) {
    const input = compileNode(inputSchema, definitions, 0);
    if (!input)
        return null;
    const output = compileNode(outputSchema, definitions, 0);
    if (!output)
        return null;
    return {
        commandId,
        encode(args) {
            // [cmd_id: u16 LE][postcard(Input)]
            const cmdId = new Uint8Array(2);
            new DataView(cmdId.buffer).setUint16(0, commandId, true);
            return concatBytes([cmdId, input.encode(args)]).buffer;
        },
        decode(buf) {
            const view = buf instanceof ArrayBuffer
                ? new DataView(buf)
                : new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
            const u8 = buf instanceof ArrayBuffer
                ? new Uint8Array(buf)
                : new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
            if (view.byteLength < 8) {
                return { ok: false, error: { code: 'invoke.too_short', message: 'response too short' } };
            }
            if (u8[0] !== 1)
                return decodeErrorFrame(u8, view);
            // postcard output at offset 8
            try {
                const v = output.decode(u8, 8);
                return { ok: true, result: v.value };
            }
            catch {
                return {
                    ok: false,
                    error: { code: 'invoke.failed', message: 'response postcard decode failed' },
                };
            }
        },
    };
}
//# sourceMappingURL=schema-postcard-codec.js.map