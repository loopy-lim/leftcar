import type { ComplexSchema } from './complex-codec-types.js';
export declare function isUnsigned(schema: ComplexSchema): boolean;
export declare function toInteger(value: unknown): bigint;
export declare function integerBounds(schema: ComplexSchema): {
    min: bigint;
    max: bigint;
};
export declare function validateInteger(value: bigint, schema: ComplexSchema): bigint;
export declare function toJsInteger(value: bigint, schema: ComplexSchema): number | bigint;
export declare function refName(ref: string): string;
export declare function optionInner(schema: ComplexSchema): ComplexSchema | null;
//# sourceMappingURL=complex-codec-schema.d.ts.map