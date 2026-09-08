/**
 * Codegen import contract — generated clients import these helpers by name
 * (see rustra codegen). They are public in name only; signatures follow the
 * generated calling convention and may change with codegen versions.
 */
import { invokeGeneratedFieldsSync, resolveGeneratedFieldsSync, runtime, } from './global-state.js';
import { ensureConfigured, isLazyConfigured, invokeGenerated } from './global-config.js';
import { RustraCommandError, RustraErrorCode } from './errors.js';
function invokeRoute(route, args, fields) {
    try {
        return Promise.resolve(route(args, fields[0], fields[1], fields[2]));
    }
    catch (error) {
        return Promise.reject(error);
    }
}
function invokeFields(commandId, command, args, fields, count, options) {
    const engine = runtime.engine;
    if (!engine) {
        if (isLazyConfigured())
            return ensureConfigured().then(() => invokeFields(commandId, command, args, fields, count, options));
        return Promise.reject(new RustraCommandError(RustraErrorCode.TransportUnavailable, 'Rustra not configured. Call configure(engine) first.'));
    }
    if (options === undefined) {
        let route = runtime.generatedFieldsRoutes[commandId];
        if (route === undefined) {
            const invoke = engine[resolveGeneratedFieldsSync]?.(commandId, command, count);
            route = invoke ? { command, fieldCount: count, invoke } : null;
            runtime.generatedFieldsRoutes[commandId] = route;
        }
        if (route && route.command === command && route.fieldCount === count)
            return invokeRoute(route.invoke, args, fields);
        const syncInvoke = engine[invokeGeneratedFieldsSync];
        if (syncInvoke) {
            try {
                return Promise.resolve(syncInvoke(commandId, command, args, count, fields[0], fields[1], fields[2]));
            }
            catch (error) {
                return Promise.reject(error);
            }
        }
    }
    return invokeGenerated(commandId, command, args, options);
}
/** @internal — codegen import contract; see note atop global-fields.ts. */
export function invokeGeneratedFields1(commandId, command, args, field0, options) {
    return invokeFields(commandId, command, args, [field0], 1, options);
}
/** @internal — codegen import contract; see note atop global-fields.ts. */
export function invokeGeneratedFields2(commandId, command, args, field0, field1, options) {
    return invokeFields(commandId, command, args, [field0, field1], 2, options);
}
/** @internal — codegen import contract; see note atop global-fields.ts. */
export function invokeGeneratedFields3(commandId, command, args, field0, field1, field2, options) {
    return invokeFields(commandId, command, args, [field0, field1, field2], 3, options);
}
/** @internal — codegen import contract; see note atop global-fields.ts. */
export function createGeneratedFields2(commandId, command, field0Key, field1Key, functionName = command) {
    let routeGeneration = -1;
    let route = null;
    const generated = ((input, options) => {
        const field0 = input[field0Key];
        const field1 = input[field1Key];
        if (!runtime.engine || options !== undefined)
            return invokeGeneratedFields2(commandId, command, input, field0, field1, options);
        if (routeGeneration !== runtime.engineGeneration) {
            route = runtime.engine[resolveGeneratedFieldsSync]?.(commandId, command, 2) ?? null;
            routeGeneration = runtime.engineGeneration;
        }
        if (route)
            return invokeRoute(route, input, [field0, field1]);
        return invokeGeneratedFields2(commandId, command, input, field0, field1);
    });
    Object.defineProperty(generated, 'name', { configurable: true, value: functionName });
    generated.commandId = command;
    return generated;
}
//# sourceMappingURL=global-fields.js.map