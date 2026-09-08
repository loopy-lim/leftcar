import { RustraCommandError, TimeoutError, RustraErrorCode } from './errors.js';
import { ensureConfigured, isLazyConfigured } from './global-config.js';
import { runtime } from './global-state.js';
export function invokeBatch(entries) {
    const engine = runtime.engine;
    if (!engine) {
        if (isLazyConfigured())
            return ensureConfigured().then(() => invokeBatch(entries));
        return Promise.reject(new RustraCommandError(RustraErrorCode.TransportUnavailable, 'Rustra not configured. Call configure(engine) first.'));
    }
    if (!engine.invokeBatch)
        return Promise.reject(new Error('Configured engine does not support invokeBatch.'));
    const timeout = entries.reduce((min, entry) => {
        const ms = entry.options?.timeoutMs;
        return ms === undefined ? min : min === undefined || ms < min ? ms : min;
    }, undefined);
    if (timeout === undefined) {
        try {
            return Promise.resolve(engine.invokeBatch(entries));
        }
        catch (error) {
            return Promise.reject(error);
        }
    }
    const stripped = entries.map((entry) => entry.options?.timeoutMs === undefined
        ? entry
        : { ...entry, options: { ...entry.options, timeoutMs: undefined } });
    let promise;
    try {
        promise = Promise.resolve(engine.invokeBatch(stripped));
    }
    catch (error) {
        return Promise.reject(error);
    }
    void promise.catch(() => { });
    let timer;
    return Promise.race([
        promise,
        new Promise((_, reject) => {
            timer = setTimeout(() => reject(new TimeoutError(`invokeBatch(${entries.length} entries) timed out after ${timeout}ms`)), timeout);
        }),
    ]).finally(() => {
        if (timer !== undefined)
            clearTimeout(timer);
    });
}
//# sourceMappingURL=global-batch.js.map