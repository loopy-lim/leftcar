import { CODEC_BUFFER, CODEC_POSITIONAL, CODEC_RAW, isNativeByteBuffer } from './global.js';
export function createRkyvRouteRuntime(context, dispatchById) {
    const { native } = context;
    const { hasBufferPath, hasByIdPath, hasRawPath, hasPositionalPath, getStaticCommandName, getStaticCommandCapabilities, ensureStaticIds, } = context.capabilities;
    const resolveGeneratedFieldsRoute = (commandId, command, fieldCount) => {
        if (getStaticCommandName(commandId) !== command)
            return undefined;
        const capabilities = getStaticCommandCapabilities(commandId);
        let fallback;
        if (hasPositionalPath && (capabilities & CODEC_POSITIONAL) !== 0) {
            fallback =
                fieldCount === 1
                    ? (_args, field0) => native.invokeTypedPos(commandId, field0)
                    : fieldCount === 2
                        ? (_args, field0, field1) => native.invokeTypedPos(commandId, field0, field1)
                        : (_args, field0, field1, field2) => native.invokeTypedPos(commandId, field0, field1, field2);
        }
        else if (hasByIdPath) {
            fallback = (args) => native.invokeTypedById(commandId, args);
        }
        if (hasRawPath && (capabilities & CODEC_RAW) !== 0) {
            if (!fallback) {
                return fieldCount === 1
                    ? (_args, field0) => native.invokeTypedRaw(commandId, field0)
                    : fieldCount === 2
                        ? (_args, field0, field1) => native.invokeTypedRaw(commandId, field0, field1)
                        : (_args, field0, field1, field2) => native.invokeTypedRaw(commandId, field0, field1, field2);
            }
            const rawFallback = fallback;
            return fieldCount === 1
                ? (args, field0) => {
                    const result = native.invokeTypedRaw(commandId, field0);
                    return typeof result === 'number' && Number.isNaN(result)
                        ? rawFallback(args, field0)
                        : result;
                }
                : fieldCount === 2
                    ? (args, field0, field1) => {
                        const result = native.invokeTypedRaw(commandId, field0, field1);
                        return typeof result === 'number' && Number.isNaN(result)
                            ? rawFallback(args, field0, field1)
                            : result;
                    }
                    : (args, field0, field1, field2) => {
                        const result = native.invokeTypedRaw(commandId, field0, field1, field2);
                        return typeof result === 'number' && Number.isNaN(result)
                            ? rawFallback(args, field0, field1, field2)
                            : result;
                    };
        }
        return fallback;
    };
    const resolveGeneratedBytesRoute = (commandId, command) => {
        if (getStaticCommandName(commandId) !== command)
            return undefined;
        const capabilities = getStaticCommandCapabilities(commandId);
        const fallback = resolveGeneratedFieldsRoute(commandId, command, 1);
        if (!hasBufferPath || (capabilities & CODEC_BUFFER) === 0)
            return fallback;
        return (args, value) => isNativeByteBuffer(value)
            ? native.invokeTypedBuffer(commandId, value)
            : fallback
                ? fallback(args, value)
                : dispatchById(commandId, command, args);
    };
    ensureStaticIds();
    return { resolveGeneratedFieldsRoute, resolveGeneratedBytesRoute };
}
//# sourceMappingURL=rkyv-engine-routes.js.map