import { CancelledError, invokeCallbackWithAbort, invokeWithTimeoutHandledSignal, parseRustraErrorString, } from '@rustra/types';
import { createFastEngine, REACT_NATIVE_RKYV_V2_ENGINE_SUPPORTS, } from './react-native-core.js';
export function createAsyncEngine(native, options) {
    const syncEngine = createFastEngine(native, options);
    if (typeof native.invokeTypedAsync !== 'function') {
        console.warn('[rustra/react-native] createAsyncEngine is using the synchronous fallback; rebuild the native module with invokeTypedAsync for JS-thread offload.');
        return syncEngine;
    }
    const invokeTypedAsync = native.invokeTypedAsync.bind(native);
    // G2 — 정적 id 캐시 (ensureStaticIds 선례): registry 기준 1회 스윕.
    // registry 에 있는 이름 = 코드젠 정적 명령 = C++ encode_by_id 로 접근 가능.
    const hasByIdPath = typeof native.invokeTypedAsyncById === 'function';
    const invokeTypedAsyncById = hasByIdPath ? native.invokeTypedAsyncById.bind(native) : null;
    const staticIds = new Map();
    if (hasByIdPath) {
        for (const [name, codec] of options.rkyvV2Codecs) {
            staticIds.set(name, codec.commandId);
        }
    }
    const transport = {
        invoke(command, args, invokeOptions) {
            const signal = invokeOptions?.signal;
            if (signal?.aborted)
                return Promise.reject(new CancelledError(`invoke("${command}") aborted before dispatch`));
            const staticId = hasByIdPath ? staticIds.get(command) : undefined;
            const dispatch = (resolve, reject) => staticId !== undefined
                ? invokeTypedAsyncById(staticId, args, (result) => resolve(result), (message) => reject(parseRustraErrorString(message)))
                : invokeTypedAsync(command, args, (result) => resolve(result), (message) => reject(parseRustraErrorString(message)));
            if (!signal) {
                return new Promise((resolve, reject) => {
                    void dispatch(resolve, reject);
                });
            }
            return invokeCallbackWithAbort(command, signal, (resolve, reject) => dispatch(resolve, reject), typeof native.invokeCancel === 'function' ? (id) => native.invokeCancel(id) : undefined);
        },
    };
    const engine = {
        // A02 — async 엔진은 sync rkyv V2 엔진의 지표를 상속하되 배치만 재정의한다:
        // 아래 invokeBatch 는 항목별 Promise.all 폴백이므로 `single-crossing` 이
        // 아니다(리뷰 정정). 취소는 invokeCancel 노출 시 전파 — 존재는 위 transport
        // 경로가 판별하므로 `cooperative` 상속이 참이다. syncEngine.supports 는
        // 옵셔널이라 스프레드 대신 상수에 재정의를 얹어 완전한 EngineSupports 를 만든다.
        supports: { ...REACT_NATIVE_RKYV_V2_ENGINE_SUPPORTS, batch: 'per-entry' },
        invoke(command, args, invokeOptions) {
            return invokeWithTimeoutHandledSignal(transport, command, args, invokeOptions);
        },
        invokeBatch(entries) {
            return Promise.all(entries.map((entry) => engine.invoke(entry.command, entry.args, entry.options)));
        },
    };
    return engine;
}
//# sourceMappingURL=react-native-async.js.map