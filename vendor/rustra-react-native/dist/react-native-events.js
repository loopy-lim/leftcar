import { RustraCommandError } from '@rustra/types';
import { getRustraNative } from './react-native-core.js';
export function createChannel(callback, native = getRustraNative()) {
    if (typeof native.createChannel !== 'function' || typeof native.dropChannel !== 'function') {
        throw new RustraCommandError('channel.unavailable', 'native module must expose createChannel() and dropChannel(); channel support is unavailable');
    }
    let closed = false;
    const handle = native.createChannel((payloadJson) => {
        if (closed)
            return;
        try {
            callback(JSON.parse(payloadJson));
        }
        catch {
            callback(null);
        }
    });
    if (!Number.isSafeInteger(handle) || handle < 0)
        throw new RustraCommandError('channel.unavailable', 'native createChannel() returned an invalid handle; expected a non-negative safe integer');
    return { handle, close: () => (closed ? false : ((closed = true), native.dropChannel(handle))) };
}
const nativeListeners = new WeakMap();
export function subscribeEvent(name, cb, options) {
    const native = getRustraNative();
    if (typeof native.onEvent !== 'function') {
        if (options?.allowMissingNative)
            return () => { };
        throw new RustraCommandError('event.unavailable', 'native module does not expose onEvent(); event subscription is unavailable');
    }
    let events = nativeListeners.get(native);
    if (!events)
        nativeListeners.set(native, (events = new Map()));
    let listeners = events.get(name);
    if (!listeners) {
        events.set(name, (listeners = new Set()));
        native.onEvent(name, (json) => {
            let payload = null;
            try {
                if (json)
                    payload = JSON.parse(json);
            }
            catch {
                /* malformed payload stays null */
            }
            for (const listener of events?.get(name) ?? []) {
                try {
                    listener(payload);
                }
                catch (error) {
                    console.error(`Rustra: event listener for "${name}" threw:`, error);
                }
            }
        });
    }
    listeners.add(cb);
    return () => {
        const current = events?.get(name);
        if (!current)
            return;
        current.delete(cb);
        if (current.size === 0) {
            events?.delete(name);
            native.offEvent?.(name);
        }
    };
}
//# sourceMappingURL=react-native-events.js.map