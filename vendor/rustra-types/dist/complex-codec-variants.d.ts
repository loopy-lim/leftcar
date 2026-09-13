import type { ComplexSchema } from './complex-codec-types.js';
export declare function variantKey(schema: ComplexSchema): string | null;
export declare function discriminator(schema: ComplexSchema): {
    key: string;
    value: unknown;
} | null;
export declare function singleEnumTag(schema: ComplexSchema): {
    key: string;
    value: unknown;
} | null;
//# sourceMappingURL=complex-codec-variants.d.ts.map