import { DEFAULT_MAX_COLLECTION_LENGTH } from './complex-codec-types.js';
import { Reader } from './complex-codec-reader.js';
export function decodeErrorFrame(bytes) {
    if (bytes.length < 10)
        return { code: 'invoke.malformed', message: 'complex response error frame is truncated' };
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const errorLength = view.getUint16(8, true);
    if (errorLength === 0)
        return { code: 'invoke.failed', message: 'complex invoke failed' };
    try {
        const reader = new Reader(bytes.slice(10, 10 + errorLength), DEFAULT_MAX_COLLECTION_LENGTH);
        const code = reader.string();
        const message = reader.string();
        return { code, message };
    }
    catch {
        return { code: 'invoke.malformed', message: 'complex response error is malformed' };
    }
}
//# sourceMappingURL=complex-codec-error.js.map