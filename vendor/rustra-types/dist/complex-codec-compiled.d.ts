import type { ComplexSchema } from './complex-codec-types.js';
/**
 * 컴파일된 complex 스키마 노드 — Rust `complex_schema_ir` 의 JS 미러.
 *
 * `createComplexCodec` 이 코덱 생성 시점에 **한 번** 스키마를 순회해 만든다.
 * 이후 encode/decode 는 `resolvedSchema`($ref/allOf hop + 전체 객체 스캔),
 * `optionInner`(재구성 클론), `variants`(키 유도+사전순 정렬)를 호출마다
 * 재계산하지 않고 컴파일된 결정만 소비한다. 원본이 매 호출 raw 스키마 모양을
 * 보고 내리는 결정(변형 매칭/본체 디스패치, Set 언래핑, uniqueItems)은
 * 컴파일 시점에 스냅샷해 와이어와 관찰 동작을 그대로 유지한다.
 */
export type CompiledNode = {
    kind: 'string' | 'boolean' | 'null';
} | {
    kind: 'integer';
    unsigned: boolean;
    format?: string;
} | {
    kind: 'number';
    single: boolean;
} | {
    kind: 'seq';
    tuple: CompiledNode[] | null;
    items: CompiledNode | null;
    uniqueItems: boolean;
} | {
    kind: 'option';
    inner: CompiledNode;
} | {
    kind: 'struct';
    fields: {
        key: string;
        node: CompiledNode;
        required: boolean;
    }[];
} | {
    kind: 'map';
    value: CompiledNode;
} | {
    kind: 'enum';
    values: unknown[];
} | {
    kind: 'const';
    value: unknown;
    inner: CompiledNode | null;
} | {
    kind: 'oneof';
    variants: CompiledVariant[];
};
/** 변형 — matcher/body 결정을 원본 matchesVariant/encodeVariant/decodeVariant
 * 순서대로 컴파일 시점에 밟아 고정한다. */
export type CompiledVariant = {
    tag: {
        key: string;
        value: unknown;
    } | null;
    matcher: {
        kind: 'discriminator';
    } | {
        kind: 'singleProperty';
        key: string;
    } | {
        kind: 'constEq';
        value: unknown;
    } | {
        kind: 'enumSingle';
        value: unknown;
    } | {
        kind: 'anyString' | 'anyObject' | 'never';
    };
    body: {
        kind: 'tagged';
        node: CompiledNode;
        skipKey: string;
    } | {
        kind: 'unwrapSingle';
        key: string;
        node: CompiledNode;
    } | {
        kind: 'constValue';
        value: unknown;
    } | {
        kind: 'enumFirst';
        value: unknown;
    } | {
        kind: 'node';
        node: CompiledNode;
    };
};
export declare function compileSchema(schema: ComplexSchema, definitions: Record<string, ComplexSchema>): CompiledNode;
//# sourceMappingURL=complex-codec-compiled.d.ts.map