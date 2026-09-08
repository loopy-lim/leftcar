import { CancelledError, RustraCommandError } from './errors.js';
import { invokeCallbackWithAbort, raceAbort } from './cancel.js';
import { encodeTier3Request, decodeTier3Response } from './json-wire.js';
import { tier2Outcome, payloadTooLargeError } from './rkyv-engine-contract.js';
import { createDynamicCodecRuntime } from './rkyv-engine-dynamic-codec.js';
export function createRkyvInvokeRaw(context, dispatch) {
    const { native, registry, schema, payloadLimit } = context;
    const { hasTypedPath, ensureStaticIds } = context.capabilities;
    const { dispatchPromise } = dispatch;
    // (T2-3) dispatch 와 동일한 동적 binary 판정 — 캐시는 엔진별로 독립이지만
    // entry 객체 식별(세대 재조회 시 새 객체)이라 두 캐시가 같은 판정에 수렴한다.
    const dynamicCodecs = createDynamicCodecRuntime(schema);
    const invokeRaw = (command, args, options) => {
        const signal = options?.signal;
        if (signal?.aborted) {
            return Promise.reject(new CancelledError(`invoke("${command}") aborted before dispatch`));
        }
        if (!signal)
            return dispatchPromise(command, args);
        // 네이티브 전파 경로 (T1): JS 코덱(tier 2) 명령이고 invokeAsync +
        // invokeCancel 이 모두 노출되면 Rust 측 체크포인트까지 취소가 닿는다.
        // typed(tier 1)/tier 3 동적 경로는 invokeAsync 가 있어도 얕은 취소로
        // 폴백한다 (설계 노트: 전파는 JS 코덱 경로만).
        const codec = registry.get(command);
        // P0-3: hasStaticCodec JSI 호출 대신 엔진 생애 1회 스윕 캐시 조회.
        const onTypedPath = hasTypedPath && ensureStaticIds()?.has(command) === true;
        if (!onTypedPath && codec && native.invokeAsync && native.invokeCancel) {
            return invokeCallbackWithAbort(command, signal, (resolve, reject, isSettled) => {
                const encoded = codec.encode(args);
                const tooLarge = payloadTooLargeError(encoded.byteLength, payloadLimit);
                if (tooLarge)
                    throw tooLarge;
                return native.invokeAsync(encoded, (resp) => {
                    if (isSettled())
                        return;
                    const outcome = tier2Outcome(codec, resp);
                    if (outcome.ok)
                        resolve(outcome.value);
                    else
                        reject(outcome.error);
                });
            }, (invocationId) => native.invokeCancel(invocationId));
        }
        if (!codec && native.invokeAsync && native.invokeCancel) {
            const cmdId = hasTypedPath ? ensureStaticIds()?.get(command) : undefined;
            const entry = cmdId !== undefined ? { commandId: cmdId } : schema.lookupCachedLiveSchemaEntry(command);
            if (entry) {
                // (T2-3) 동적 명령도 postcard/complex binary 가 가능하면 전파 경로에서
                // binary 프레임을 쓴다 — Rust 핸들러 라우트와의 정합이 취소 전파보다
                // 우선한다(와이어 불일치는 핸들러 오류로 귀결).
                const dynamicCodec = dynamicCodecs.lookupBinaryCodec(entry);
                if (dynamicCodec) {
                    return invokeCallbackWithAbort(command, signal, (resolve, reject, isSettled) => {
                        const encoded = dynamicCodec.encode(args);
                        const tooLarge = payloadTooLargeError(encoded.byteLength, payloadLimit);
                        if (tooLarge)
                            throw tooLarge;
                        return native.invokeAsync(encoded, (resp) => {
                            if (isSettled())
                                return;
                            const outcome = tier2Outcome(dynamicCodec, resp);
                            if (outcome.ok)
                                resolve(outcome.value);
                            else
                                reject(outcome.error);
                        });
                    }, (invocationId) => native.invokeCancel(invocationId));
                }
                return invokeCallbackWithAbort(command, signal, (resolve, reject, isSettled) => {
                    const encoded = encodeTier3Request(entry.commandId, args);
                    const tooLarge = payloadTooLargeError(encoded.byteLength, payloadLimit);
                    if (tooLarge)
                        throw tooLarge;
                    return native.invokeAsync(encoded, (resp) => {
                        if (isSettled())
                            return;
                        const outcome = decodeTier3Response(resp);
                        if (outcome.ok)
                            resolve(outcome.result);
                        else {
                            const e = outcome.error ??
                                { code: 'invoke.failed', message: 'RkyvV2 (tier3) invoke failed' };
                            reject(new RustraCommandError(e.code, e.message, e.retryable ?? false));
                        }
                    });
                }, (invocationId) => native.invokeCancel(invocationId));
            }
        }
        return raceAbort(dispatchPromise(command, args), signal, command);
    };
    return invokeRaw;
}
//# sourceMappingURL=rkyv-engine-async.js.map