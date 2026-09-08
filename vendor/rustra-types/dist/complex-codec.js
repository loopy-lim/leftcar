import { ComplexCodecError, DEFAULT_MAX_COLLECTION_LENGTH, DEFAULT_MAX_DEPTH, DEFAULT_MAX_PAYLOAD_BYTES, } from './complex-codec-types.js';
import { Reader } from './complex-codec-reader.js';
import { Writer } from './complex-codec-wire.js';
import { decodeErrorFrame } from './complex-codec-error.js';
import { decodeNode } from './complex-codec-decode-node.js';
import { encodeNode } from './complex-codec-encode-node.js';
import { compileSchema } from './complex-codec-compiled.js';
export function createComplexCodec(options) {
    const definitions = options.definitions ?? {};
    const maxDepth = options.maxDepth ?? DEFAULT_MAX_DEPTH;
    const maxPayloadBytes = options.maxPayloadBytes ?? DEFAULT_MAX_PAYLOAD_BYTES;
    const maxCollectionLength = options.maxCollectionLength ?? DEFAULT_MAX_COLLECTION_LENGTH;
    if (!Number.isSafeInteger(options.commandId) ||
        options.commandId < 0 ||
        options.commandId > 0xffff) {
        throw new ComplexCodecError('command id must fit u16');
    }
    // 스키마를 코덱 생성 시점에 **한 번** 컴파일한다 — 이후 호출은 컴파일된
    // 결정만 순회한다 (Rust complex_schema_ir 와 동일 전략).
    const inputIr = compileSchema(options.inputSchema, definitions);
    const outputIr = compileSchema(options.outputSchema, definitions);
    const encode = (args) => {
        const writer = new Writer(maxPayloadBytes);
        writer.byte(options.commandId & 0xff);
        writer.byte((options.commandId >> 8) & 0xff);
        encodeNode(writer, inputIr, args, maxDepth, 0, maxCollectionLength);
        return writer.finish();
    };
    return {
        commandId: options.commandId,
        encode,
        encodeInto(args, reuse) {
            const encoded = new Uint8Array(encode(args));
            if (reuse && reuse.length >= encoded.length) {
                reuse.set(encoded);
                return reuse.subarray(0, encoded.length);
            }
            return encoded;
        },
        decode(buffer) {
            try {
                const bytes = buffer instanceof ArrayBuffer
                    ? new Uint8Array(buffer)
                    : new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
                if (bytes.length < 8) {
                    return {
                        ok: false,
                        error: { code: 'invoke.too_short', message: 'response too short' },
                    };
                }
                if (bytes[0] !== 1)
                    return { ok: false, error: decodeErrorFrame(bytes) };
                const reader = new Reader(bytes, maxCollectionLength);
                reader.raw(8);
                const result = decodeNode(reader, outputIr, maxDepth, 0, maxCollectionLength);
                if (reader.remaining !== 0)
                    throw new ComplexCodecError('trailing bytes in complex response');
                return { ok: true, result };
            }
            catch (error) {
                return {
                    ok: false,
                    error: {
                        code: 'invoke.malformed',
                        message: error instanceof Error ? error.message : String(error),
                    },
                };
            }
        },
    };
}
export { ComplexCodecError } from './complex-codec-types.js';
//# sourceMappingURL=complex-codec.js.map