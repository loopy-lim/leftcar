import type { EngineClient } from './public.js';
/**
 * @internal — engine dispatch protocol: versioned `Symbol.for` keys the facade
 * uses to probe an engine's byId/positional/bytes fast paths (see
 * InternalEngineClient). Plumbing between this package's facade and its
 * engines; not public API.
 */
export declare const invokeByIdSync: unique symbol;
/** @internal — dispatch protocol key, see {@link invokeByIdSync}. */
export declare const invokeGeneratedFieldsSync: unique symbol;
/** @internal — dispatch protocol key, see {@link invokeByIdSync}. */
export declare const resolveGeneratedFieldsSync: unique symbol;
/** @internal — dispatch protocol key, see {@link invokeByIdSync}. */
export declare const invokeGeneratedBytesSync: unique symbol;
/** @internal — dispatch protocol key, see {@link invokeByIdSync}. */
export declare const resolveGeneratedBytesSync: unique symbol;
/** @internal — capability bitmask consumed by the engine fast-path dispatch (see InternalEngineClient). */
export declare const CODEC_TYPED: number;
export declare const CODEC_POSITIONAL: number;
export declare const CODEC_RAW: number;
export declare const CODEC_BUFFER: number;
/** @internal — byte-field detection used by the engine dispatch routes; not public API. */
export declare function isNativeByteBuffer(value: unknown): value is Uint8Array | ArrayBuffer;
/** @internal — shapes behind the engine fast-path protocol; not public API. */
export type GeneratedFieldsRoute = (args: unknown, field0: unknown, field1?: unknown, field2?: unknown) => unknown;
/** @internal — see {@link GeneratedFieldsRoute}. */
export type GeneratedBytesRoute = (args: unknown, value: unknown) => unknown;
/** @internal — see {@link GeneratedFieldsRoute}. */
export type CachedGeneratedFieldsRoute = {
    command: string;
    fieldCount: 1 | 2 | 3;
    invoke: GeneratedFieldsRoute;
};
/** @internal — see {@link GeneratedFieldsRoute}. */
export type CachedGeneratedBytesRoute = {
    command: string;
    invoke: GeneratedBytesRoute;
};
/** @internal — the commandId-carrying function shape codegen emits; not public API. */
export type GeneratedCommand<TInput, TOutput> = ((input: TInput, options?: import('./public.js').InvokeOptions) => Promise<TOutput>) & {
    commandId: string;
};
/** @internal — engine contract extended with the optional sync fast-path symbols; not public API. */
export type InternalEngineClient = EngineClient & {
    [invokeByIdSync]?<T>(commandId: number, command: string, args?: unknown): T;
    [invokeGeneratedFieldsSync]?<T>(commandId: number, command: string, args: unknown, fieldCount: 1 | 2 | 3, field0: unknown, field1?: unknown, field2?: unknown): T;
    [resolveGeneratedFieldsSync]?(commandId: number, command: string, fieldCount: 1 | 2 | 3): GeneratedFieldsRoute | undefined;
    [invokeGeneratedBytesSync]?<T>(commandId: number, command: string, args: unknown, value: unknown): T;
    [resolveGeneratedBytesSync]?(commandId: number, command: string): GeneratedBytesRoute | undefined;
};
/** @internal — module-global engine/route registry (shared across duplicate copies via Symbol.for). Not public API. */
export declare const runtime: {
    engine: InternalEngineClient | null;
    engineInitializer?: () => EngineClient | Promise<EngineClient>;
    engineInitialization?: Promise<InternalEngineClient>;
    /**
     * @internal — R08 소유권: 현재 pending lazy 등록이 ensureConfigured 에 의해
     * 소비를 시작했는지. 소비 전 경쟁 등록만 loud-fail 한다(소비 뒤 교체·복구는
     * 기존 계약 유지). configure/configureLazy 의 새 등록에서 리셋된다.
     */
    engineInitializerConsumed: boolean;
    /** @internal — R08 소유권: pending 등록자 식별(진단 메시지용). */
    engineOwnerId?: string;
    engineGeneration: number;
    generatedFieldsRoutes: Array<CachedGeneratedFieldsRoute | null | undefined>;
    generatedBytesRoutes: Array<CachedGeneratedBytesRoute | null | undefined>;
};
/** @internal — invalidates cached engine fast-path routes on (re)configure; not public API. */
export declare function resetConfiguredRoutes(): void;
//# sourceMappingURL=global-state.d.ts.map