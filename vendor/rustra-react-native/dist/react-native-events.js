import { parseRustraErrorString, RustraCommandError } from '@rustra/types';
import { getRustraNative } from './react-native-core.js';
export function createChannel(callback, native = getRustraNative(), options) {
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
    // 폴링 drain — CallInvoker 없는 호스트의 채널 큐 소비(SubscribeOptions.pollMs
    // 와 동일 계약). 수명은 채널에 귀속 — close 가 수요를 해제한다.
    const pollMs = options?.pollMs;
    if (pollMs !== undefined)
        acquirePollingDemand(native, pollMs);
    return {
        handle,
        close: () => {
            if (closed)
                return false;
            closed = true;
            if (pollMs !== undefined)
                releasePollingDemand(native);
            return native.dropChannel(handle);
        },
    };
}
/**
 * 바이너리 채널 생성 — 콜백은 rkyv V2 프레임 등 임의 바이트(ArrayBuffer)를
 * 받는다. JSON 경로(`createChannel`)와 동일한 핸들/close 계약, 한 핸들은 한
 * 경로로만 동작한다. 네이티브가 `createChannelBytes` 를 노출하지 않으면
 * `channel.unavailable` 로 loud-fail 한다.
 */
export function createBytesChannel(callback, native = getRustraNative(), options) {
    if (typeof native.createChannelBytes !== 'function' || typeof native.dropChannel !== 'function') {
        throw new RustraCommandError('channel.unavailable', 'native module must expose createChannelBytes() and dropChannel(); binary channel support is unavailable');
    }
    let closed = false;
    const handle = native.createChannelBytes((payload) => {
        if (closed)
            return;
        callback(payload instanceof Uint8Array ? payload : new Uint8Array(payload));
    });
    if (!Number.isSafeInteger(handle) || handle < 0)
        throw new RustraCommandError('channel.unavailable', 'native createChannelBytes() returned an invalid handle; expected a non-negative safe integer');
    // 폴링 drain — JSON 경로와 동일 계약/수명(close 가 수요를 해제한다).
    const pollMs = options?.pollMs;
    if (pollMs !== undefined)
        acquirePollingDemand(native, pollMs);
    return {
        handle,
        close: () => {
            if (closed)
                return false;
            closed = true;
            if (pollMs !== undefined)
                releasePollingDemand(native);
            return native.dropChannel(handle);
        },
    };
}
const nativeListeners = new WeakMap();
/** 폴링 drain 루프 — 네이티브 인스턴스당 1개(WeakMap). drainEvents 가 존재하고
 * pollMs > 0 인 소비자(이벤트 구독/채널)가 1개라도 있으면 가동한다. 간격은 첫
 * 수요자의 pollMs 를 따른다. */
const pollTimers = new WeakMap();
/** 네이티브 인스턴스당 폴링 수요 수 — 이벤트 구독 해제/채널 close 로 0 이 되면
 * 루프를 정지한다. */
const pollingDemand = new WeakMap();
function acquirePollingDemand(native, pollMs) {
    if (typeof native.drainEvents !== 'function' || pollMs <= 0)
        return;
    const demand = pollingDemand.get(native) ?? 0;
    pollingDemand.set(native, demand + 1);
    if (demand > 0)
        return; // 이미 가동 중 — 수요만 증가.
    const tick = () => {
        if ((pollingDemand.get(native) ?? 0) === 0)
            return; // 수요 소멸 — 정지.
        try {
            native.drainEvents();
        }
        catch (error) {
            console.error('Rustra: drainEvents failed:', error);
        }
        pollTimers.set(native, setTimeout(tick, pollMs));
    };
    pollTimers.set(native, setTimeout(tick, pollMs));
}
function releasePollingDemand(native) {
    const demand = pollingDemand.get(native) ?? 0;
    if (demand === 0)
        return; // 수요 없음 — drainEvents 미노출 no-op acquire 짝.
    if (demand > 1) {
        pollingDemand.set(native, demand - 1);
        return;
    }
    pollingDemand.set(native, 0);
    const timer = pollTimers.get(native);
    if (timer != null) {
        clearTimeout(timer);
        pollTimers.set(native, null);
    }
}
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
            // Array.from — Expo 의 ES5 타겟에서 Set 순회는 downlevelIteration 이 필요하다.
            // forEach — 이벤트마다 Array.from 이 배열을 할당하지 않는다(핫패스).
            const listeners = events?.get(name);
            listeners?.forEach((listener) => {
                try {
                    listener(payload);
                }
                catch (error) {
                    console.error(`Rustra: event listener for "${name}" threw:`, error);
                }
            });
        });
    }
    listeners.add(cb);
    // 폴링 drain 옵션 — CallInvoker 없는 호스트(C++ 큐가 JS 폴링 대기)를 위한
    // JS 측 소비 루프. onEvent 푸시와 병행 무해(drain 이 비어 있으면 0).
    const pollMs = options?.pollMs;
    let pollingHeld = false;
    if (pollMs !== undefined) {
        pollingHeld = true;
        acquirePollingDemand(native, pollMs);
    }
    return () => {
        const current = events?.get(name);
        if (current) {
            current.delete(cb);
            if (current.size === 0) {
                events?.delete(name);
                native.offEvent?.(name);
            }
        }
        // pollMs 구독의 수요 해제 — 1회만(해제 후 수요 0 이면 루프 정지).
        if (pollingHeld) {
            pollingHeld = false;
            releasePollingDemand(native);
        }
    };
}
/**
 * 동기 typed invoke — UI 핫패스 등 Promise 오버헤드를 제거하는 경로.
 * C++ `invokeTyped` fast path(encode → FFI → decode)를 그대로 쓰며 반환값은
 * 디코딩된 출력 그 자체다. 정적 코덱이 없는 명령/네이티브는
 * `sync.unavailable` 로 loud-fail 한다(폴백 정책은 호출자 소관).
 *
 * 계약: JS 런타임 스레드에서만 호출(JSI 스레드 친화성 — 다른 네이티브 경로와
 * 동일). 커맨드 에러는 `RustraCommandError`(code/message 유지)로 재발행된다.
 */
export function invokeTypedSync(name, args, native = getRustraNative()) {
    if (typeof native.invokeTyped !== 'function') {
        throw new RustraCommandError('sync.unavailable', 'native module does not expose invokeTyped(); synchronous invoke is unavailable');
    }
    try {
        return native.invokeTyped(name, args ?? {});
    }
    catch (error) {
        if (error instanceof RustraCommandError)
            throw error;
        const message = error instanceof Error ? error.message : String(error);
        // C++ HostFunction 은 "code: message" 문자열 JSError 를 던진다 —
        // 안정 코드로 복원한다(파서는 JSON/"code: message" 양쪽 계약 지원).
        throw parseRustraErrorString(message);
    }
}
//# sourceMappingURL=react-native-events.js.map