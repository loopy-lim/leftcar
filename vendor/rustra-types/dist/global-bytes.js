import { invokeGeneratedBytesSync, runtime, resolveGeneratedBytesSync } from './global-state.js';
import { ensureConfigured, isLazyConfigured } from './global-config.js';
import { invokeGeneratedFields1 } from './global-fields.js';
import { RustraCommandError, RustraErrorCode } from './errors.js';
/** @internal — codegen import contract; see note atop global-fields.ts. */
export function invokeGeneratedBytes(commandId, command, args, value, options) {
    const engine = runtime.engine;
    if (!engine) {
        if (isLazyConfigured())
            return ensureConfigured().then(() => invokeGeneratedBytes(commandId, command, args, value, options));
        return Promise.reject(new RustraCommandError(RustraErrorCode.TransportUnavailable, 'Rustra not configured. Call configure(engine) first.'));
    }
    if (options === undefined) {
        let route = runtime.generatedBytesRoutes[commandId];
        if (route === undefined) {
            const invoke = engine[resolveGeneratedBytesSync]?.(commandId, command);
            route = invoke ? { command, invoke } : null;
            runtime.generatedBytesRoutes[commandId] = route;
        }
        if (route && route.command === command) {
            try {
                return Promise.resolve(route.invoke(args, value));
            }
            catch (error) {
                return Promise.reject(error);
            }
        }
        const syncInvoke = engine[invokeGeneratedBytesSync];
        if (syncInvoke) {
            try {
                return Promise.resolve(syncInvoke(commandId, command, args, value));
            }
            catch (error) {
                return Promise.reject(error);
            }
        }
    }
    return invokeGeneratedFields1(commandId, command, args, value, options);
}
//# sourceMappingURL=global-bytes.js.map