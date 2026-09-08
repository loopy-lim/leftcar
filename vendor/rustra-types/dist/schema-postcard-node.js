import { optionInner } from './complex-codec-schema.js';
import { concatBytes, decF32, decF64, decString, decVarint, decVarint64, decZigzag64, decZigzagVarint, encF32, encF64, encString, encVarint, encVarint64, encZigzag64, encZigzagVarint, } from './schema-postcard-wire.js';
// TODO(json-wire): resolveRef 의 접두사 판정('#/definitions/' 가 아닌 슬래시
// 경로는 null 로 폴백)은 postcard 지원 판정의 일부라 refName 로 바꾸지 않는다
// — json-wire 쪽 공용 export 가 준비되면 이동한다.
function resolveRef(schema, definitions) {
    const ref = schema.$ref;
    if (!ref)
        return schema;
    const name = ref.startsWith('#/definitions/') ? ref.slice('#/definitions/'.length) : ref;
    return definitions[name] ?? null;
}
/**
 * Option<T> 언래핑 — `complex-codec-schema.optionInner` 를 재사용하되 postcard
 * 판정(anyOf 조건)은 그대로 유지한다: anyOf 멤버 중 null 타입이 하나 있고,
 * 나머지가 중첩 anyOf 가 아닌 단일 non-null 일 때만 Option 으로 인정한다.
 * 와이어는 type-array 분기(공통)와 anyOf 분기(로컬)가 서로 다른 판정을 낸다.
 */
function unwrapOption(schema) {
    if (Array.isArray(schema.type))
        return optionInner(schema);
    if (Array.isArray(schema.anyOf) && schema.anyOf.length === 2) {
        const nonNull = schema.anyOf.filter((item) => item.type !== 'null' && !('anyOf' in item));
        return nonNull.length === 1 && schema.anyOf.some((item) => item.type === 'null')
            ? nonNull[0]
            : null;
    }
    return null;
}
function isPlainMap(schema) {
    return (schema.type === 'object' &&
        !!schema.additionalProperties &&
        typeof schema.additionalProperties === 'object' &&
        !schema.properties);
}
/** 스키마 노드를 encode/decode 클로저로 컴파일한다. 미지원이면 null. */
function compileNode(schema, definitions, depth) {
    if (depth > 8)
        return null;
    // tuple newtype (schemars: single-entry allOf + $ref) — 내부로 투명하게.
    if (Array.isArray(schema.allOf) && schema.allOf.length === 1) {
        return compileNode(schema.allOf[0], definitions, depth + 1);
    }
    // string enum — variant index varint (선언순).
    if (schema.type === 'string' && Array.isArray(schema.enum) && schema.enum.length > 0) {
        const variants = schema.enum.filter((v) => typeof v === 'string');
        if (variants.length !== schema.enum.length)
            return null;
        return {
            encode: (value) => {
                const idx = variants.indexOf(value);
                if (idx < 0)
                    throw new Error('invalid enum value: ' + String(value));
                return encVarint(idx);
            },
            decode: (buf, offset) => {
                const v = decVarint(buf, offset);
                const variant = variants[v.value];
                if (variant === undefined)
                    throw new Error('enum index out of range: ' + v.value);
                return { value: variant, bytesRead: v.bytesRead };
            },
        };
    }
    // $ref 해결.
    if (schema.$ref) {
        const resolved = resolveRef(schema, definitions);
        if (!resolved)
            return null;
        if (resolved.type === 'object' && resolved.properties && !resolved.additionalProperties) {
            return compileStruct(resolved, definitions, depth);
        }
        return compileNode(resolved, definitions, depth + 1);
    }
    if (schema.type === 'boolean') {
        return {
            encode: (v) => new Uint8Array([v ? 1 : 0]),
            decode: (buf, offset) => ({ value: buf[offset] === 1, bytesRead: 1 }),
        };
    }
    if (schema.type === 'integer') {
        if (schema.format === 'uint64') {
            return {
                encode: (v) => encVarint64(v),
                decode: (buf, offset) => {
                    const v = decVarint64(buf, offset);
                    return { value: v.value, bytesRead: v.bytesRead };
                },
            };
        }
        if (schema.format === 'int64') {
            return {
                encode: (v) => encZigzag64(v),
                decode: (buf, offset) => {
                    const v = decVarint64(buf, offset);
                    return { value: decZigzag64(v.value), bytesRead: v.bytesRead };
                },
            };
        }
        const unsigned = ['uint8', 'uint16', 'uint32'].includes(schema.format ?? '');
        if (unsigned) {
            return {
                encode: (v) => encVarint(v),
                decode: (buf, offset) => {
                    const v = decVarint(buf, offset);
                    return { value: v.value, bytesRead: v.bytesRead };
                },
            };
        }
        return {
            encode: (v) => encZigzagVarint(v),
            decode: (buf, offset) => {
                const v = decZigzagVarint(buf, offset);
                return { value: v.value, bytesRead: v.bytesRead };
            },
        };
    }
    if (schema.type === 'number') {
        return schema.format === 'float'
            ? {
                encode: (v) => encF32(v),
                decode: (buf, offset) => {
                    const v = decF32(buf, offset);
                    return { value: v.value, bytesRead: v.bytesRead };
                },
            }
            : {
                encode: (v) => encF64(v),
                decode: (buf, offset) => {
                    const v = decF64(buf, offset);
                    return { value: v.value, bytesRead: v.bytesRead };
                },
            };
    }
    if (schema.type === 'string') {
        return {
            encode: (v) => encString(v),
            decode: (buf, offset) => {
                const v = decString(buf, offset);
                return { value: v.value, bytesRead: v.bytesRead };
            },
        };
    }
    // Option<T> — type:["T","null"] 또는 anyOf:[T, null].
    const optionInner = unwrapOption(schema);
    if (optionInner) {
        const inner = compileNode(optionInner, definitions, depth + 1);
        if (!inner)
            return null;
        return {
            encode: (v) => v === null || v === undefined
                ? new Uint8Array([0])
                : concatBytes([new Uint8Array([1]), inner.encode(v)]),
            decode: (buf, offset) => {
                const tag = buf[offset];
                if (tag === undefined)
                    throw new Error('option tag out of bounds');
                if (tag === 0)
                    return { value: null, bytesRead: 1 };
                const v = inner.decode(buf, offset + 1);
                return { value: v.value, bytesRead: 1 + v.bytesRead };
            },
        };
    }
    // 배열 — tuple(items 배열) 우선, 그다음 단일 items.
    if (schema.type === 'array' && schema.items !== undefined) {
        if (Array.isArray(schema.items)) {
            const minItems = schema.minItems;
            const maxItems = schema.maxItems;
            if (minItems === maxItems && minItems !== undefined && minItems > 0) {
                const items = schema.items.map((item) => compileNode(item, definitions, depth + 1));
                if (items.some((n) => n === null))
                    return null;
                const nodes = items;
                return {
                    encode: (v) => {
                        const arr = v;
                        return concatBytes(arr.map((el, i) => nodes[i].encode(el)));
                    },
                    decode: (buf, offset) => {
                        let bytesRead = 0;
                        const out = [];
                        for (const node of nodes) {
                            const v = node.decode(buf, offset + bytesRead);
                            out.push(v.value);
                            bytesRead += v.bytesRead;
                        }
                        return { value: out, bytesRead };
                    },
                };
            }
            return null;
        }
        const items = schema.items;
        const uniqueItems = schema.uniqueItems === true;
        // bytes 특례 — Vec<u8> 은 len + raw.
        if (!uniqueItems && items.type === 'integer' && items.format === 'uint8') {
            return {
                encode: (v) => {
                    const u = v instanceof Uint8Array ? v : new Uint8Array(v);
                    return concatBytes([encVarint(u.length), u]);
                },
                decode: (buf, offset) => {
                    const len = decVarint(buf, offset);
                    const start = offset + len.bytesRead;
                    return {
                        value: buf.slice(start, start + len.value),
                        bytesRead: len.bytesRead + len.value,
                    };
                },
            };
        }
        const element = compileNode(items, definitions, depth + 1);
        if (!element)
            return null;
        if (uniqueItems) {
            return {
                encode: (v) => {
                    const arr = [...v];
                    return concatBytes([encVarint(arr.length), ...arr.map((el) => element.encode(el))]);
                },
                decode: (buf, offset) => {
                    const len = decVarint(buf, offset);
                    let cursor = offset + len.bytesRead;
                    const values = [];
                    for (let i = 0; i < len.value; i++) {
                        const v = element.decode(buf, cursor);
                        values.push(v.value);
                        cursor += v.bytesRead;
                    }
                    return { value: new Set(values), bytesRead: cursor - offset };
                },
            };
        }
        return {
            encode: (v) => {
                const arr = v;
                return concatBytes([encVarint(arr.length), ...arr.map((el) => element.encode(el))]);
            },
            decode: (buf, offset) => {
                const len = decVarint(buf, offset);
                let cursor = offset + len.bytesRead;
                const values = [];
                for (let i = 0; i < len.value; i++) {
                    const v = element.decode(buf, cursor);
                    values.push(v.value);
                    cursor += v.bytesRead;
                }
                return { value: values, bytesRead: cursor - offset };
            },
        };
    }
    // 원시값 map — additionalProperties (properties 없음).
    if (isPlainMap(schema)) {
        const valueNode = compileNode(schema.additionalProperties, definitions, depth + 1);
        if (!valueNode)
            return null;
        return {
            encode: (v) => {
                const map = v;
                const keys = Object.keys(map).sort();
                return concatBytes([
                    encVarint(keys.length),
                    ...keys.flatMap((k) => [encString(k), valueNode.encode(map[k])]),
                ]);
            },
            decode: (buf, offset) => {
                const len = decVarint(buf, offset);
                let cursor = offset + len.bytesRead;
                const map = {};
                for (let i = 0; i < len.value; i++) {
                    const k = decString(buf, cursor);
                    cursor += k.bytesRead;
                    const v = valueNode.decode(buf, cursor);
                    cursor += v.bytesRead;
                    map[k.value] = v.value;
                }
                return { value: map, bytesRead: cursor - offset };
            },
        };
    }
    // struct — properties 선언순(postcard 필드순).
    if (schema.type === 'object' && schema.properties && !schema.additionalProperties) {
        return compileStruct(schema, definitions, depth);
    }
    return null;
}
function compileStruct(schema, definitions, depth) {
    const names = Object.keys(schema.properties ?? {});
    const nodes = [];
    for (const name of names) {
        const node = compileNode(schema.properties[name], definitions, depth + 1);
        if (!node)
            return null;
        nodes.push(node);
    }
    return {
        encode: (v) => {
            const obj = v;
            return concatBytes(nodes.map((n, i) => n.encode(obj[names[i]])));
        },
        decode: (buf, offset) => {
            let cursor = offset;
            const obj = {};
            for (let i = 0; i < names.length; i++) {
                const v = nodes[i].decode(buf, cursor);
                cursor += v.bytesRead;
                obj[names[i]] = v.value;
            }
            return { value: obj, bytesRead: cursor - offset };
        },
    };
}
export { compileNode };
// ── RkyvV2Codec 조립 ────────────────────────────────────────
//# sourceMappingURL=schema-postcard-node.js.map