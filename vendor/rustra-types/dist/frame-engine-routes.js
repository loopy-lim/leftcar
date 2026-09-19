import { CODEC_BUFFER, CODEC_POSITIONAL, CODEC_RAW, isNativeByteBuffer } from './global.js';
export function createFrameRouteRuntime(context, dispatchById) {
    const { native } = context;
    const { hasBufferPath, hasByIdPath, hasRawPath, hasPositionalPath, getStaticCommandName, getStaticCommandCapabilities, ensureStaticIds, } = context.capabilities;
    const resolveGeneratedFieldsRoute = (commandId, command, fieldCount) => {
        if (getStaticCommandName(commandId) !== command)
            return undefined;
        const capabilities = getStaticCommandCapabilities(commandId);
        let fallback;
        if (hasPositionalPath && (capabilities & CODEC_POSITIONAL) !== 0) {
            if (fieldCount === 1)
                fallback = (_args, field0) => native.invokeTypedPos(commandId, field0);
            else if (fieldCount === 2)
                fallback = (_args, field0, field1) => native.invokeTypedPos(commandId, field0, field1);
            else
                fallback = (_args, field0, field1, field2) => native.invokeTypedPos(commandId, field0, field1, field2);
        }
        else if (hasByIdPath) {
            fallback = (args) => native.invokeTypedById(commandId, args);
        }
        if (hasRawPath && (capabilities & CODEC_RAW) !== 0) {
            if (!fallback) {
                if (fieldCount === 1)
                    return (_args, field0) => native.invokeTypedRaw(commandId, field0);
                if (fieldCount === 2)
                    return (_args, field0, field1) => native.invokeTypedRaw(commandId, field0, field1);
                return (_args, field0, field1, field2) => native.invokeTypedRaw(commandId, field0, field1, field2);
            }
            const rawFallback = fallback;
            if (fieldCount === 1) {
                return (args, field0) => {
                    const result = native.invokeTypedRaw(commandId, field0);
                    return typeof result === 'number' && Number.isNaN(result)
                        ? rawFallback(args, field0)
                        : result;
                };
            }
            if (fieldCount === 2) {
                return (args, field0, field1) => {
                    const result = native.invokeTypedRaw(commandId, field0, field1);
                    return typeof result === 'number' && Number.isNaN(result)
                        ? rawFallback(args, field0, field1)
                        : result;
                };
            }
            return (args, field0, field1, field2) => {
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
        return (args, value) => {
            if (isNativeByteBuffer(value))
                return native.invokeTypedBuffer(commandId, value);
            if (fallback)
                return fallback(args, value);
            return dispatchById(commandId, command, args);
        };
    };
    ensureStaticIds();
    return { resolveGeneratedFieldsRoute, resolveGeneratedBytesRoute };
}
//# sourceMappingURL=frame-engine-routes.js.map