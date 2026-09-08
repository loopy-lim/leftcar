import { ComplexCodecError, DEFAULT_MAX_DEPTH } from './complex-codec-types.js';
import { compareUtf8 } from './complex-codec-wire.js';
import { isUnsigned, optionInner, refName } from './complex-codec-schema.js';
import { discriminator, variantKey } from './complex-codec-variants.js';
const MAX_DEPTH = DEFAULT_MAX_DEPTH;
export function compileSchema(schema, definitions) {
    return compileNode(schema, definitions, new Map(), 0);
}
function compileNode(schema, definitions, refs, depth) {
    if (depth > MAX_DEPTH)
        throw new ComplexCodecError('schema reference depth exceeded');
    // resolvedSchema 미러 — $ref/allOf 전개.
    if (schema.$ref) {
        const name = refName(schema.$ref);
        const resolved = definitions[name];
        if (!resolved)
            throw new ComplexCodecError(`missing schema definition ${schema.$ref}`);
        const memo = refs.get(name);
        // 진행 중(사이클) 재진입은 아직 완성되지 않은 노드를 재사용해 끊는다 —
        // 컴파일 산물은 불변 트리이므로 부분 완성 노드의 재진입 참조도 안전.
        if (memo)
            return memo;
        // 플레이스홀더를 먼저 놓지 않는 대신, 완성 후 맵에 넣는다. 사이클은
        // depth 한도로도 방어된다(원본 resolved_schema 와 동일 정책).
        const compiled = compileNode(resolved, definitions, refs, depth + 1);
        refs.set(name, compiled);
        return compiled;
    }
    if (schema.allOf) {
        if (schema.allOf.length !== 1)
            throw new ComplexCodecError('complex codec does not support multi-entry allOf');
        return compileNode(schema.allOf[0], definitions, refs, depth + 1);
    }
    // optionInner 미러.
    const option = optionInner(schema);
    if (option)
        return { kind: 'option', inner: compileNode(option, definitions, refs, depth + 1) };
    if (schema.oneOf)
        return compileOneOf(schema, definitions, refs, depth);
    if (schema.enum)
        return { kind: 'enum', values: schema.enum };
    if (schema.const !== undefined) {
        const inner = schema.type !== undefined
            ? compileNode({ ...schema, const: undefined }, definitions, refs, depth)
            : null;
        return { kind: 'const', value: schema.const, inner };
    }
    switch (schema.type) {
        case 'boolean':
            return { kind: 'boolean' };
        case 'integer':
            return { kind: 'integer', unsigned: isUnsigned(schema), format: schema.format };
        case 'number':
            return { kind: 'number', single: schema.format === 'float' };
        case 'string':
            return { kind: 'string' };
        case 'null':
            return { kind: 'null' };
        case 'array': {
            const items = schema.items;
            if (Array.isArray(items)) {
                return {
                    kind: 'seq',
                    tuple: items.map((item) => compileNode(item, definitions, refs, depth + 1)),
                    items: null,
                    uniqueItems: schema.uniqueItems === true,
                };
            }
            if (!items)
                throw new ComplexCodecError('array schema is missing items');
            return {
                kind: 'seq',
                tuple: null,
                items: compileNode(items, definitions, refs, depth + 1),
                uniqueItems: schema.uniqueItems === true,
            };
        }
        case 'object': {
            if (schema.additionalProperties !== undefined && !schema.properties) {
                if (!schema.additionalProperties || typeof schema.additionalProperties === 'boolean')
                    throw new ComplexCodecError('map schema is missing value type');
                return {
                    kind: 'map',
                    value: compileNode(schema.additionalProperties, definitions, refs, depth + 1),
                };
            }
            const required = new Set(schema.required ?? []);
            return {
                kind: 'struct',
                fields: Object.entries(schema.properties ?? {}).map(([key, fieldSchema]) => ({
                    key,
                    node: compileNode(fieldSchema, definitions, refs, depth + 1),
                    required: required.has(key),
                })),
            };
        }
        default:
            throw new ComplexCodecError(`unsupported schema type ${String(schema.type)}`);
    }
}
function compileOneOf(schema, definitions, refs, depth) {
    const explicit = schema['x-rustra-variant-order'];
    const choices = schema.oneOf ?? [];
    if (explicit &&
        (explicit.length !== choices.length || new Set(explicit).size !== explicit.length)) {
        throw new ComplexCodecError('x-rustra-variant-order must contain unique keys for every variant');
    }
    const keyed = choices.map((variant, index) => {
        const key = explicit?.[index] ?? variantKey(variant);
        if (key === null)
            throw new ComplexCodecError('enum variants require a stable key or explicit metadata');
        return { schema: variant, key };
    });
    keyed.sort((left, right) => compareUtf8(left.key, right.key));
    if (new Set(keyed.map((variant) => variant.key)).size !== keyed.length) {
        throw new ComplexCodecError('enum variant keys must be unique');
    }
    return {
        kind: 'oneof',
        variants: keyed.map(({ schema: variant }) => compileVariant(variant, definitions, refs, depth)),
    };
}
function compileVariant(variant, definitions, refs, depth) {
    const tag = discriminator(variant);
    const properties = variant.properties;
    // O(1) 조회 — required 배열을 필드 순회마다 includes 로 훑지 않는다.
    const requiredSet = new Set(variant.required ?? []);
    // matchesVariant 순서: discriminator → 단일 프로퍼티 → const → 단일 enum →
    // type 폴백(string/object) → never.
    const matcher = tag
        ? { kind: 'discriminator' }
        : properties && Object.keys(properties).length === 1
            ? { kind: 'singleProperty', key: Object.keys(properties)[0] }
            : variant.const !== undefined
                ? { kind: 'constEq', value: variant.const }
                : variant.enum?.length === 1
                    ? { kind: 'enumSingle', value: variant.enum[0] }
                    : variant.type === 'string'
                        ? { kind: 'anyString' }
                        : variant.type === 'object'
                            ? { kind: 'anyObject' }
                            : { kind: 'never' };
    // encodeVariant/decodeVariant 순서: discriminator(tag+object) → 단일 프로퍼티
    // → const/enum → 폴스루.
    const body = tag && variant.type === 'object'
        ? {
            kind: 'tagged',
            skipKey: tag.key,
            node: {
                kind: 'struct',
                fields: Object.entries(properties ?? {}).map(([key, fieldSchema]) => ({
                    key,
                    node: compileNode(fieldSchema, definitions, refs, depth + 1),
                    required: requiredSet.has(key),
                })),
            },
        }
        : properties && Object.keys(properties).length === 1
            ? {
                kind: 'unwrapSingle',
                key: Object.keys(properties)[0],
                node: compileNode(properties[Object.keys(properties)[0]], definitions, refs, depth + 1),
            }
            : variant.const !== undefined
                ? { kind: 'constValue', value: variant.const }
                : variant.enum
                    ? { kind: 'enumFirst', value: variant.enum[0] }
                    : { kind: 'node', node: compileNode(variant, definitions, refs, depth + 1) };
    return { tag, matcher, body };
}
//# sourceMappingURL=complex-codec-compiled.js.map