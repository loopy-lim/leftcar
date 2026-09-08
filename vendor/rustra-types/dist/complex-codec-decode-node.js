import { ComplexCodecError } from './complex-codec-types.js';
import { toJsInteger } from './complex-codec-schema.js';
/**
 * 컴파일된 IR 을 순회하는 디코더 — 런타임 스키마 재해석 없음. 원본 분기
 * 순서(옵션 태그, oneOf 인덱스, enum 인덱스, 타입 디스패치)와 에러 문자열을
 * 유지한다.
 */
export function decodeNode(reader, node, maxDepth, depth, maxCollectionLength) {
    if (depth > maxDepth)
        throw new ComplexCodecError(`value depth exceeds ${maxDepth}`);
    switch (node.kind) {
        case 'option':
            return reader.byte() === 1
                ? decodeNode(reader, node.inner, maxDepth, depth + 1, maxCollectionLength)
                : null;
        case 'oneof': {
            const index = Number(reader.varint());
            const variant = node.variants[index];
            if (!variant)
                throw new ComplexCodecError('enum variant index out of range');
            return decodeVariant(reader, variant, maxDepth, depth + 1, maxCollectionLength);
        }
        case 'enum': {
            const index = Number(reader.varint());
            if (index < 0 || index >= node.values.length)
                throw new ComplexCodecError('enum index out of range');
            return node.values[index];
        }
        case 'const':
            // const 단독은 와이어 0바이트(원본 decodeVariant 계약). const+type 은
            // 타입만 읽는다(원본 decodeNode 에 const 분기 없음).
            return node.inner
                ? decodeNode(reader, node.inner, maxDepth, depth, maxCollectionLength)
                : node.value;
        case 'boolean': {
            const value = reader.byte();
            if (value > 1)
                throw new ComplexCodecError('invalid boolean value');
            return value === 1;
        }
        case 'integer':
            return toJsInteger(node.unsigned ? reader.varint() : reader.zigzag(), {
                format: node.format,
            });
        case 'number': {
            const bytes = reader.raw(node.single ? 4 : 8);
            const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
            return node.single ? view.getFloat32(0, true) : view.getFloat64(0, true);
        }
        case 'string':
            return reader.string();
        case 'null':
            return null;
        case 'seq': {
            const length = reader.length();
            if (node.tuple) {
                if (node.tuple.length !== length)
                    throw new ComplexCodecError('tuple length mismatch');
                return node.tuple.map((item) => decodeNode(reader, item, maxDepth, depth + 1, maxCollectionLength));
            }
            if (!node.items)
                throw new ComplexCodecError('array schema is missing items');
            const values = Array.from({ length }, () => decodeNode(reader, node.items, maxDepth, depth + 1, maxCollectionLength));
            return node.uniqueItems ? new Set(values) : values;
        }
        case 'struct': {
            const result = {};
            for (const field of node.fields) {
                const present = field.required || readPresence(reader, `optional field ${field.key}`);
                if (present)
                    result[field.key] = decodeNode(reader, field.node, maxDepth, depth + 1, maxCollectionLength);
                else if (field.required)
                    throw new ComplexCodecError(`missing required field ${field.key}`);
            }
            return result;
        }
        case 'map': {
            const result = {};
            const length = reader.length();
            for (let i = 0; i < length; i += 1) {
                const key = reader.string();
                if (Object.prototype.hasOwnProperty.call(result, key))
                    throw new ComplexCodecError(`duplicate map key ${key}`);
                result[key] = decodeNode(reader, node.value, maxDepth, depth + 1, maxCollectionLength);
            }
            return result;
        }
        default:
            throw new ComplexCodecError('unsupported compiled node');
    }
}
function readPresence(reader, label) {
    const tag = reader.byte();
    if (tag === 0)
        return false;
    if (tag === 1)
        return true;
    throw new ComplexCodecError(`invalid ${label} presence tag`);
}
/** 변형 본체 디코드 — 컴파일 시점에 고정된 body 를 실행한다. */
function decodeVariant(reader, variant, maxDepth, depth, maxCollectionLength) {
    switch (variant.body.kind) {
        case 'tagged': {
            const result = {};
            if (variant.tag)
                result[variant.tag.key] = variant.tag.value;
            const struct = variant.body.node;
            if (struct.kind !== 'struct')
                throw new ComplexCodecError('expected object');
            for (const field of struct.fields) {
                if (field.key === variant.body.skipKey)
                    continue;
                const present = field.required || readPresence(reader, `optional field ${field.key}`);
                if (present)
                    result[field.key] = decodeNode(reader, field.node, maxDepth, depth + 1, maxCollectionLength);
                else if (field.required)
                    throw new ComplexCodecError(`missing required field ${field.key}`);
            }
            return result;
        }
        case 'unwrapSingle':
            return {
                [variant.body.key]: decodeNode(reader, variant.body.node, maxDepth, depth, maxCollectionLength),
            };
        case 'constValue':
            return variant.body.value;
        case 'enumFirst':
            return variant.body.value;
        case 'node':
            return decodeNode(reader, variant.body.node, maxDepth, depth, maxCollectionLength);
        default:
            throw new ComplexCodecError('unsupported compiled variant body');
    }
}
//# sourceMappingURL=complex-codec-decode-node.js.map