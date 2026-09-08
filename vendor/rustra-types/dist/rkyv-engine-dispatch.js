import { isRetryableCode, RustraCommandError } from './errors.js';
import { CODEC_RAW, CODEC_POSITIONAL } from './global.js';
import { decodeTier3Response, encodeTier3Request } from './json-wire.js';
import { traceWire } from './debug.js';
import { tier2Outcome, payloadTooLargeError } from './rkyv-engine-contract.js';
import { createDynamicCodecRuntime } from './rkyv-engine-dynamic-codec.js';
export function createRkyvDispatchRuntime(context) {
    const { native, registry, schema, payloadLimit } = context;
    const dynamicCodecs = createDynamicCodecRuntime(schema);
    const { hasTypedPath, hasByIdPath, hasRawPath, hasPositionalPath, ensureStaticIds, getStaticCommandCapabilities, isVerifiedStaticId, } = context.capabilities;
    // 신호 없는 기본 3-티어 dispatch (T1 리팩터링 — 로직은 기존 그대로).
    // encodeInto 재사용 버퍼 풀 — 커맨드 이름별 최근 버퍼 1개(단일 진입 dispatch
    // 의 직렬 인코딩 전제). 미사용 시 맵은 비어 있어 오버헤드 0이다.
    const encodeIntoBuffers = new Map();
    const dispatch = (command, args) => {
        // 1순위: C++ fast path (RN JSI). 정적 명령만. JS 측 인코딩이 없어
        // maxPayloadBytes 검사를 건너뛴다 — 네이티브 한도가 그대로 적용된다.
        // byId 진입이 가능하면 JSI 1회 + u16 디스패치 (P0-3).
        if (hasTypedPath) {
            const cmdId = ensureStaticIds()?.get(command);
            if (cmdId !== undefined) {
                if (hasByIdPath) {
                    return native.invokeTypedById(cmdId, args);
                }
                return native.invokeTyped(command, args);
            }
        }
        // 2순위: JS codec (Node/Bun/Tauri 또는 typed 누락 시). 정적 명령.
        const codec = registry.get(command);
        if (codec) {
            // encodeInto(재사용 버퍼)가 있으면 호출당 신규 할당을 피한다. 버퍼는
            // 커맨드별로 1개(단일 진입 dispatch 는 동시에 한 요청만 인코딩한다)다.
            // invokeRkyvV2 는 왕복 전에 버퍼를 소비하므로 재진입 안전하다.
            let encoded;
            if (codec.encodeInto) {
                const bucket = encodeIntoBuffers;
                const reuse = bucket.get(command);
                const written = codec.encodeInto(args, reuse);
                if (written.buffer !== reuse?.buffer)
                    bucket.set(command, written);
                encoded = written.buffer;
                if (written.byteOffset !== 0 || written.byteLength !== written.buffer.byteLength) {
                    // 재사용 버퍼가 subarray 라면 정확한 슬라이스 ArrayBuffer 로 사본을
                    // 만든다(첫 호출 grow 후엔 byteOffset 0/full-length 로 수렴한다).
                    encoded = written.slice().buffer;
                }
            }
            else {
                encoded = codec.encode(args);
            }
            // (T3) 네이티브 왕복 전에 크기 검사 — 초과면 invokeRkyvV2 를 부르지 않는다.
            const tooLarge = payloadTooLargeError(encoded.byteLength, payloadLimit);
            if (tooLarge)
                throw tooLarge;
            traceWire('request', command, encoded);
            const resultBytes = native.invokeRkyvV2(encoded);
            traceWire('response', command, resultBytes);
            // Reject (do not throw) so the declared Promise<T> contract holds and
            // callers can use .catch() / await-try-consistently for command errors.
            const outcome = tier2Outcome(codec, resultBytes);
            if (!outcome.ok)
                throw outcome.error;
            return outcome.value;
        }
        // 3순위: 동적 명령 → live schema 의 commandId 사용. (T2-3) Rust registry 의
        // 3-way 판정을 미러해 binary 코덱(postcard → complex)이 가능한 스키마는
        // binary 로, 둘 다 거부하는 스키마만 Tier 3(JSON-in-binary) 로 보낸다.
        // getSchema 미노출 네이티브에서 cached lookup은 undefined 를
        // 돌려주므로 기존 command.not_found 계약이 그대로 유지된다.
        // (T0-3) 세대 게이트는 lookupCachedLiveSchemaEntry 내부에서 1회 흡수된다.
        const entry = schema.lookupCachedLiveSchemaEntry(command);
        if (!entry) {
            throw new RustraCommandError('command.not_found', `RkyvV2: no codec and not in live schema for "${command}"`);
        }
        const dynamicCodec = dynamicCodecs.lookupBinaryCodec(entry);
        if (dynamicCodec) {
            // tier 2(정적 코덱)와 동일한 왕복 계약 — encode/검사/invoke/decode.
            const encoded = dynamicCodec.encode(args);
            const tooLarge = payloadTooLargeError(encoded.byteLength, payloadLimit);
            if (tooLarge)
                throw tooLarge;
            traceWire('request', command, encoded);
            const resultBytes = native.invokeRkyvV2(encoded);
            traceWire('response', command, resultBytes);
            const outcome = tier2Outcome(dynamicCodec, resultBytes);
            if (!outcome.ok)
                throw outcome.error;
            return outcome.value;
        }
        const tier3Request = encodeTier3Request(entry.commandId, args);
        // (T3) tier 2 와 동일한 사전 검사 — 네이티브 호출 전에 조기 실패.
        const tooLarge = payloadTooLargeError(tier3Request.byteLength, payloadLimit);
        if (tooLarge)
            throw tooLarge;
        traceWire('request', command, tier3Request);
        const tier3Response = native.invokeRkyvV2(tier3Request);
        traceWire('response', command, tier3Response);
        const resp = decodeTier3Response(tier3Response);
        if (!resp.ok) {
            const e = resp.error ?? { code: 'invoke.failed', message: 'RkyvV2 (tier3) invoke failed' };
            throw new RustraCommandError(e.code, e.message, e.retryable ?? isRetryableCode(e.code));
        }
        return resp.result;
    };
    // 공개 EngineClient는 항상 Promise를 반환하지만 RN JSI/Node/Bun의 기본
    // dispatch는 동기다. 단 한 번만 Promise로 승격해 async dispatch가 만들던
    // 불필요한 Promise/microtask를 제거하고, 동기 throw는 rejected Promise로
    // 바꿔 기존 호출 계약을 유지한다.
    const dispatchPromise = (command, args) => {
        try {
            return Promise.resolve(dispatch(command, args));
        }
        catch (error) {
            return Promise.reject(error);
        }
    };
    const dispatchById = (commandId, command, args) => {
        if (hasByIdPath && isVerifiedStaticId(commandId, command)) {
            return native.invokeTypedById(commandId, args);
        }
        return dispatch(command, args);
    };
    const dispatchGeneratedFields = (commandId, command, args, fieldCount, field0, field1, field2) => {
        if (isVerifiedStaticId(commandId, command)) {
            const capabilities = getStaticCommandCapabilities(commandId);
            if (hasRawPath && (capabilities & CODEC_RAW) !== 0) {
                const result = fieldCount === 1
                    ? native.invokeTypedRaw(commandId, field0)
                    : fieldCount === 2
                        ? native.invokeTypedRaw(commandId, field0, field1)
                        : native.invokeTypedRaw(commandId, field0, field1, field2);
                // New hosts return the declared output shape. NaN is the defensive
                // fallback marker used when the Rust registry cannot service raw even
                // though generated metadata advertised it.
                if (!(typeof result === 'number' && Number.isNaN(result)))
                    return result;
            }
            if (hasPositionalPath && (capabilities & CODEC_POSITIONAL) !== 0) {
                return (fieldCount === 1
                    ? native.invokeTypedPos(commandId, field0)
                    : fieldCount === 2
                        ? native.invokeTypedPos(commandId, field0, field1)
                        : native.invokeTypedPos(commandId, field0, field1, field2));
            }
        }
        return dispatchById(commandId, command, args);
    };
    const dispatchPromiseById = (commandId, command, args) => {
        try {
            return Promise.resolve(dispatchById(commandId, command, args));
        }
        catch (error) {
            return Promise.reject(error);
        }
    };
    return {
        dispatch,
        dispatchPromise,
        dispatchById,
        dispatchPromiseById,
        dispatchGeneratedFields,
    };
}
//# sourceMappingURL=rkyv-engine-dispatch.js.map