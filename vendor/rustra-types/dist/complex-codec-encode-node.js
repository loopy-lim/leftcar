import { ComplexCodecError } from './complex-codec-types.js';
import { sortedKeys } from './complex-codec-wire.js';
import { toInteger, validateInteger } from './complex-codec-schema.js';
/**
 * 컴파일된 IR 을 순회하는 인코더 — 스키마 해석(`resolvedSchema`/`optionInner`/
 * `variants`)을 호출마다 재계산하지 않고 노드 결정만 소비한다. 와이어는 기존
 * `&ComplexSchema` 해석 버전과 바이트 단위로 동일하다(원본 분기 순서·에러
 * 문자열 유지).
 */
export function encodeNode(writer, node, value, maxDepth, depth, maxCollectionLength) {
    if (depth > maxDepth)
        throw new ComplexCodecError(`value depth exceeds ${maxDepth}`);
    switch (node.kind) {
        case 'option': {
            if (value === null || value === undefined) {
                writer.byte(0);
            }
            else {
                writer.byte(1);
                encodeNode(writer, node.inner, value, maxDepth, depth + 1, maxCollectionLength);
            }
            return;
        }
        case 'oneof': {
            const selected = node.variants.findIndex((variant) => variantMatches(variant, value));
            if (selected < 0)
                throw new ComplexCodecError('value does not match any enum variant');
            writer.varint(BigInt(selected));
            encodeVariant(writer, node.variants[selected], value, maxDepth, depth + 1, maxCollectionLength);
            return;
        }
        case 'enum': {
            const index = node.values.findIndex((candidate) => Object.is(candidate, value));
            if (index < 0)
                throw new ComplexCodecError('value is not a member of enum');
            writer.varint(BigInt(index));
            return;
        }
        case 'const': {
            if (!Object.is(node.value, value))
                throw new ComplexCodecError(`value does not match const ${String(node.value)}`);
            if (node.inner)
                encodeNode(writer, node.inner, value, maxDepth, depth, maxCollectionLength);
            return;
        }
        case 'boolean': {
            if (typeof value !== 'boolean')
                throw new ComplexCodecError('expected boolean');
            writer.byte(value ? 1 : 0);
            return;
        }
        case 'integer': {
            const integer = validateInteger(toInteger(value), { format: node.format });
            if (node.unsigned)
                writer.varint(integer);
            else
                writer.zigzag(integer);
            return;
        }
        case 'number': {
            if (typeof value !== 'number' || !Number.isFinite(value))
                throw new ComplexCodecError('expected finite number');
            const buffer = new ArrayBuffer(node.single ? 4 : 8);
            const view = new DataView(buffer);
            if (buffer.byteLength === 4)
                view.setFloat32(0, value, true);
            else
                view.setFloat64(0, value, true);
            writer.push(new Uint8Array(buffer));
            return;
        }
        case 'string': {
            if (typeof value !== 'string')
                throw new ComplexCodecError('expected string');
            writer.string(value);
            return;
        }
        case 'null': {
            if (value !== null)
                throw new ComplexCodecError('expected null');
            return;
        }
        case 'seq': {
            const values = value instanceof Set ? [...value] : value;
            if (!Array.isArray(values))
                throw new ComplexCodecError('expected array or Set');
            if (values.length > maxCollectionLength)
                throw new ComplexCodecError(`collection length exceeds ${maxCollectionLength}`);
            writer.varint(BigInt(values.length));
            if (node.tuple) {
                if (node.tuple.length !== values.length)
                    throw new ComplexCodecError('tuple length mismatch');
                node.tuple.forEach((item, index) => encodeNode(writer, item, values[index], maxDepth, depth + 1, maxCollectionLength));
                return;
            }
            if (!node.items)
                throw new ComplexCodecError('array schema is missing items');
            values.forEach((item) => encodeNode(writer, node.items, item, maxDepth, depth + 1, maxCollectionLength));
            return;
        }
        case 'struct': {
            if (typeof value !== 'object' || value === null || Array.isArray(value))
                throw new ComplexCodecError('expected object');
            encodeStruct(writer, node, value, maxDepth, depth, maxCollectionLength);
            return;
        }
        case 'map': {
            if (typeof value !== 'object' || value === null || Array.isArray(value))
                throw new ComplexCodecError('expected object map');
            const object = value;
            const keys = sortedKeys(object);
            if (keys.length > maxCollectionLength)
                throw new ComplexCodecError(`collection length exceeds ${maxCollectionLength}`);
            writer.varint(BigInt(keys.length));
            for (const key of keys) {
                writer.string(key);
                encodeNode(writer, node.value, object[key], maxDepth, depth + 1, maxCollectionLength);
            }
            return;
        }
        default:
            throw new ComplexCodecError('unsupported compiled node');
    }
}
/** 변형 매칭 — 컴파일 시점에 고정된 matcher 를 값에 적용한다. */
function variantMatches(variant, value) {
    switch (variant.matcher.kind) {
        case 'discriminator': {
            if (!variant.tag || typeof value !== 'object' || value === null)
                return false;
            return Object.is(value[variant.tag.key], variant.tag.value);
        }
        case 'singleProperty':
            return (typeof value === 'object' &&
                value !== null &&
                Object.prototype.hasOwnProperty.call(value, variant.matcher.key));
        case 'constEq':
            return Object.is(variant.matcher.value, value);
        case 'enumSingle':
            return Object.is(variant.matcher.value, value);
        case 'anyString':
            return typeof value === 'string';
        case 'anyObject':
            return typeof value === 'object' && value !== null && !Array.isArray(value);
        default:
            return false;
    }
}
/** 변형 본체 인코딩 — 컴파일 시점에 고정된 body 를 실행한다. */
function encodeVariant(writer, variant, value, maxDepth, depth, maxCollectionLength) {
    switch (variant.body.kind) {
        case 'tagged': {
            if (typeof value !== 'object' || value === null)
                throw new ComplexCodecError('expected enum object');
            const struct = variant.body.node;
            if (struct.kind !== 'struct')
                throw new ComplexCodecError('expected object');
            encodeStruct(writer, struct, value, maxDepth, depth, maxCollectionLength, variant.body.skipKey);
            return;
        }
        case 'unwrapSingle':
            encodeNode(writer, variant.body.node, value?.[variant.body.key], maxDepth, depth, maxCollectionLength);
            return;
        case 'constValue':
        case 'enumFirst':
            return;
        case 'node':
            encodeNode(writer, variant.body.node, value, maxDepth, depth, maxCollectionLength);
            return;
        default:
            throw new ComplexCodecError('unsupported compiled variant body');
    }
}
/** struct 인코딩 — 프로퍼티 declaration 순서, 선택 필드 presence 태그. */
function encodeStruct(writer, node, value, maxDepth, depth, maxCollectionLength, skipKey) {
    for (const field of node.fields) {
        if (field.key === skipKey)
            continue;
        const present = Object.prototype.hasOwnProperty.call(value, field.key) && value[field.key] !== undefined;
        if (!field.required)
            writer.byte(present ? 1 : 0);
        if (present)
            encodeNode(writer, field.node, value[field.key], maxDepth, depth + 1, maxCollectionLength);
        else if (field.required)
            throw new ComplexCodecError(`missing required field ${field.key}`);
    }
}
//# sourceMappingURL=complex-codec-encode-node.js.map