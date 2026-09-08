import { ComplexCodecError } from './complex-codec-types.js';
export function isUnsigned(schema) {
    return typeof schema.format === 'string' && schema.format.startsWith('uint');
}
export function toInteger(value) {
    if (typeof value === 'bigint')
        return value;
    if (typeof value === 'number' && Number.isSafeInteger(value))
        return BigInt(value);
    throw new ComplexCodecError('integer must be a safe number or bigint');
}
export function integerBounds(schema) {
    switch (schema.format) {
        case 'uint8':
            return { min: 0n, max: 0xffn };
        case 'uint16':
            return { min: 0n, max: 0xffffn };
        case 'uint32':
            return { min: 0n, max: 0xffffffffn };
        case 'uint64':
            return { min: 0n, max: 0xffffffffffffffffn };
        case 'int8':
            return { min: -0x80n, max: 0x7fn };
        case 'int16':
            return { min: -0x8000n, max: 0x7fffn };
        case 'int32':
            return { min: -0x80000000n, max: 0x7fffffffn };
        case 'int64':
        default:
            return { min: -0x8000000000000000n, max: 0x7fffffffffffffffn };
    }
}
export function validateInteger(value, schema) {
    const { min, max } = integerBounds(schema);
    if (value < min || value > max) {
        throw new ComplexCodecError(`integer is outside ${String(min)}..${String(max)}`);
    }
    return value;
}
export function toJsInteger(value, schema) {
    validateInteger(value, schema);
    if (schema.format === 'int64' || schema.format === 'uint64') {
        const asNumber = Number(value);
        return Number.isSafeInteger(asNumber) ? asNumber : value;
    }
    const asNumber = Number(value);
    if (!Number.isSafeInteger(asNumber))
        throw new ComplexCodecError('decoded integer exceeds JavaScript safe range');
    return asNumber;
}
export function refName(ref) {
    return ref.slice(ref.lastIndexOf('/') + 1);
}
export function optionInner(schema) {
    if (Array.isArray(schema.type)) {
        const nonNull = schema.type.filter((type) => type !== 'null');
        if (schema.type.length === 2 && nonNull.length === 1)
            return { ...schema, type: nonNull[0] };
    }
    if (schema.anyOf?.length === 2) {
        const nonNull = schema.anyOf.filter((item) => item.type !== 'null');
        if (nonNull.length === 1)
            return nonNull[0];
    }
    return null;
}
//# sourceMappingURL=complex-codec-schema.js.map