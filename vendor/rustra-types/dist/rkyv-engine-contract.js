import { isRetryableCode, RustraCommandError } from './errors.js';
import { decodeUtf8 } from './utf8.js';
/**
 * tier 2(JS 코덱) 응답 프레임을 결과/에러로 환산한다 — `dispatch` 와 전파
 * 경로 콜백이 공유하는 유일 경로 (T1 리뷰). `codec.decode` 가 잘못된 프레임으로
 * throw 하면 그 예외를 reject 값으로 돌린다(비-Error 는 `invoke.failed` 로
 * 래핑): 전파 경로의 콜백은 네이티브 트램펄린 안에서 실행되므로 예외가
 * 새어나가면 프라미스가 영원히 정착하지 않는다. 이 함수 자체는 throw 하지 않는다.
 */
export function tier2Outcome(codec, frame) {
    let response;
    try {
        response = codec.decode(frame);
    }
    catch (err) {
        return {
            ok: false,
            error: err instanceof Error
                ? err
                : new RustraCommandError('invoke.failed', `codec decode failed: ${String(err)}`),
        };
    }
    if (!response.ok) {
        const e = response.error ?? { code: 'invoke.failed', message: 'RkyvV2 invoke failed' };
        return {
            ok: false,
            error: new RustraCommandError(e.code, e.message, e.retryable ?? isRetryableCode(e.code)),
        };
    }
    return { ok: true, value: response.result };
}
/**
 * (T3) 인코딩된 페이로드의 크기 사전 검사 — JS 코덱(tier 2)/tier 3 경로가
 * 네이티브를 호출하기 직전에 공유한다. `limit` 이 undefined 면 검사하지 않는다
 * (네이티브의 동적 한도가 최종 게이트). 초과 시 `payload.too_large`
 * (non-retryable — 결정론적 클라이언트 조건) 를 반환하고 호출자는 네이티브
 * 왕복 없이 즉시 reject 한다.
 */
export function payloadTooLargeError(encodedBytes, limit) {
    if (limit === undefined || encodedBytes <= limit)
        return undefined;
    return new RustraCommandError('payload.too_large', `encoded payload ${encodedBytes}B exceeds maxPayloadBytes ${limit}B`, false);
}
export function validateRkyvEngineOptions(native, options, schema) {
    // F5 (opt-in): 계약 해시 검증. 빌드 시점 hash 와 네이티브 실시간 hash 가 다르면
    // 기본적으로 엔진을 만들지 않고 즉시 실패(fail-fast)한다. T2 onContractMismatch
    // 콜백을 설정하면 불일치 시 throw 대신 콜백 호출 후 degraded 모드로 계속 생성한다.
    if (options?.contractHash !== undefined) {
        if (typeof native.getContractHash !== 'function') {
            // unenforceable 은 콜백과 무관하게 항상 throw — native hash 가 없으면
            // degraded 모드가 무의미하다 (검증 가능한 것이 아무것도 없다).
            throw new RustraCommandError('contract.unenforceable', 'contractHash option was set but the native module does not expose ' +
                'getContractHash(); cannot verify schema drift. Check that the current generated codecs ' +
                'and Rust native archive were both compiled into the installed app.');
        }
        const hashBytes = new Uint8Array(native.getContractHash());
        const nativeHash = decodeUtf8(hashBytes, 0, hashBytes.length).trim();
        if (nativeHash !== options.contractHash) {
            if (!options.onContractMismatch) {
                throw new RustraCommandError('contract.mismatch', `contract hash mismatch: native="${nativeHash.slice(0, 16)}…" vs ` +
                    `expected="${options.contractHash.slice(0, 16)}…" — generated client ` +
                    `and native binary are out of sync; regenerate the TypeScript and native codecs, ` +
                    `rebuild the Rust archive, then rebuild the native app`);
            }
            options.onContractMismatch({ nativeHash, expectedHash: options.contractHash });
        }
    }
    // T2 (opt-in): schemaVersion staleness 검사. JS > native 면 경고만 한다
    // (fatal 아님 — OTA 롤백/지연 배포 상황에서도 앱은 동작해야 한다).
    // getSchema 미노출 구 네이티브는 조용히 건너뛴다 (비교할 것이 없다).
    if (options?.schemaVersion !== undefined && typeof native.getSchema === 'function') {
        // 구 네이티브(pre-Task-8)의 schema JSON 에는 schemaVersion 이 없다 —
        // CLI old-schema 관례대로 1 로 취급한다. 이 기능의 대상이 되는 정확히 그
        // 구 바이너리를 향한 스퓨리어스 경고를 막는 디폴트다.
        //
        // 스키마 파싱(getSchema 호출 자체의 실패 포함)은 절대 치명적이지 않다 —
        // 파싱이 throw 하면 staleness 검사를 조용히 건너뛴다 (getSchema 미노출
        // 경우와 동일한 취급). 경고 기능이 엔진 생성을 깨뜨리면 "fatal 아님"
        // 계약 자체가 위반된다. invoke 시점의 tier-3 스키마 파싱(getLiveSchema)은
        // 별개의 기존 동작 — malformed 스키마에서 동적 명령 호출 시 JSON.parse 가
        // dispatch 밖으로 동기 throw 할 수 있다.
        let nativeVersion;
        try {
            const document = schema.readLiveSchemaDocument();
            nativeVersion = document.schemaVersion ?? 1;
        }
        catch {
            nativeVersion = undefined;
        }
        if (nativeVersion !== undefined && options.schemaVersion > nativeVersion) {
            const info = { nativeVersion, jsVersion: options.schemaVersion };
            if (options.onSchemaStale) {
                options.onSchemaStale(info);
            }
            else {
                console.warn(`[rustra] schema stale: JS bundle schemaVersion=${info.jsVersion} > ` +
                    `native schemaVersion=${info.nativeVersion} — native binary is older ` +
                    `than the JS bundle (OTA rollback / delayed rollout); newer commands ` +
                    `may fail until native catches up`);
            }
        }
    }
}
//# sourceMappingURL=rkyv-engine-contract.js.map