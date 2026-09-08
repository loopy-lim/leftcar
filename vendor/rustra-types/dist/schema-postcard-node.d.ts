import type { ComplexSchema } from './complex-codec-types.js';
type Encoder = (value: unknown) => Uint8Array;
type Decoder = (buf: Uint8Array, offset: number) => {
    value: unknown;
    bytesRead: number;
};
type SchemaNode = {
    encode: Encoder;
    decode: Decoder;
};
/** 스키마 노드를 encode/decode 클로저로 컴파일한다. 미지원이면 null. */
declare function compileNode(schema: ComplexSchema, definitions: Record<string, ComplexSchema>, depth: number): SchemaNode | null;
export { compileNode };
//# sourceMappingURL=schema-postcard-node.d.ts.map