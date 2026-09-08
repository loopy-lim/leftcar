/**
 * Codegen import contract — generated clients import these helpers by name
 * (see rustra codegen). They are public in name only; signatures follow the
 * generated calling convention and may change with codegen versions.
 */
import { type GeneratedCommand } from './global-state.js';
import type { InvokeOptions } from './public.js';
/** @internal — codegen import contract; see note atop global-fields.ts. */
export declare function invokeGeneratedFields1<T>(commandId: number, command: string, args: unknown, field0: unknown, options?: InvokeOptions): Promise<T>;
/** @internal — codegen import contract; see note atop global-fields.ts. */
export declare function invokeGeneratedFields2<T>(commandId: number, command: string, args: unknown, field0: unknown, field1: unknown, options?: InvokeOptions): Promise<T>;
/** @internal — codegen import contract; see note atop global-fields.ts. */
export declare function invokeGeneratedFields3<T>(commandId: number, command: string, args: unknown, field0: unknown, field1: unknown, field2: unknown, options?: InvokeOptions): Promise<T>;
/** @internal — codegen import contract; see note atop global-fields.ts. */
export declare function createGeneratedFields2<TInput extends object, TOutput>(commandId: number, command: string, field0Key: keyof TInput, field1Key: keyof TInput, functionName?: string): GeneratedCommand<TInput, TOutput>;
//# sourceMappingURL=global-fields.d.ts.map