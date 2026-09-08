/**
 * @internal — engine dispatch protocol: versioned `Symbol.for` keys the facade
 * uses to probe an engine's byId/positional/bytes fast paths (see
 * InternalEngineClient). Plumbing between this package's facade and its
 * engines; not public API.
 */
export const invokeByIdSync = Symbol.for('dev.rustra.types.v0.4.0.invokeByIdSync');
/** @internal — dispatch protocol key, see {@link invokeByIdSync}. */
export const invokeGeneratedFieldsSync = Symbol.for('dev.rustra.types.v0.4.0.invokeGeneratedFieldsSync');
/** @internal — dispatch protocol key, see {@link invokeByIdSync}. */
export const resolveGeneratedFieldsSync = Symbol.for('dev.rustra.types.v0.4.0.resolveGeneratedFieldsSync');
/** @internal — dispatch protocol key, see {@link invokeByIdSync}. */
export const invokeGeneratedBytesSync = Symbol.for('dev.rustra.types.v0.4.0.invokeGeneratedBytesSync');
/** @internal — dispatch protocol key, see {@link invokeByIdSync}. */
export const resolveGeneratedBytesSync = Symbol.for('dev.rustra.types.v0.4.0.resolveGeneratedBytesSync');
/** @internal — capability bitmask consumed by the engine fast-path dispatch (see InternalEngineClient). */
export const CODEC_TYPED = 1 << 0;
export const CODEC_POSITIONAL = 1 << 1;
export const CODEC_RAW = 1 << 2;
export const CODEC_BUFFER = 1 << 3;
/** @internal — byte-field detection used by the engine dispatch routes; not public API. */
export function isNativeByteBuffer(value) {
    if (typeof ArrayBuffer === 'undefined' || typeof value !== 'object' || value === null)
        return false;
    if (value instanceof ArrayBuffer)
        return true;
    return (ArrayBuffer.isView(value) && value.BYTES_PER_ELEMENT === 1);
}
/** @internal — module-global engine/route registry (shared across duplicate copies via Symbol.for). Not public API. */
export const runtime = (() => {
    const key = Symbol.for('dev.rustra.types.v0.4.0.runtimeState');
    const global = globalThis;
    const existing = global[key];
    if (existing)
        return existing;
    const value = {
        engine: null,
        engineGeneration: 0,
        engineInitializerConsumed: false,
        generatedFieldsRoutes: [],
        generatedBytesRoutes: [],
    };
    global[key] = value;
    return value;
})();
/** @internal — invalidates cached engine fast-path routes on (re)configure; not public API. */
export function resetConfiguredRoutes() {
    runtime.engineGeneration += 1;
    runtime.generatedFieldsRoutes = [];
    runtime.generatedBytesRoutes = [];
}
//# sourceMappingURL=global-state.js.map