import { parseLiveSchemaDocument, } from './live-schema.js';
export function createRkyvSchemaRuntime(native) {
    let liveSchemaCache;
    /** (T0-3) 캐시가 빌드된 시점의 세대 — 네이티브 폴링 값과 비교한다. */
    let cachedGeneration;
    /**
     * (T0-3 후속) 재동기화 에포크 — live schema 를 새로 읽을 때마다 1씩 오른다.
     * 파생 캐시(동적 코덱 캐시)가 자신이 빌드된 에포크를 비교해 세대 변경 시
     * 구 entry 코덱을 비울 수 있게 한다(구 코덱 누적 방지).
     */
    let resyncEpoch = 0;
    const readLiveSchemaDocument = () => {
        const document = parseLiveSchemaDocument(native);
        liveSchemaCache = document.commands;
        cachedGeneration = document.schemaGeneration;
        resyncEpoch += 1;
        return document;
    };
    const refreshLiveSchema = () => {
        return readLiveSchemaDocument().commands;
    };
    const lookupCachedLiveSchemaEntry = (command) => {
        // (T0-3) 단일 진입 게이트 — 모든 조회 경로(동기 dispatch, async 전파)가
        // 세대 재동기화를 통과한 뒤 캐시를 읽는다. 호출자별 수동 resyncIfStale 은
        // 중복이므로 두지 않는다(정확히 1회 게이트 보장).
        resyncIfStale();
        const cached = liveSchemaCache?.get(command);
        if (cached)
            return cached;
        try {
            return refreshLiveSchema().get(command);
        }
        catch {
            return undefined;
        }
    };
    /**
     * (T0-3) 세대 게이트 — 네이티브가 `getSchemaGeneration` 을 노출하고 캐시된
     * 세대와 다르면 live schema 를 재조회해 캐시와 세대를 재동기화한다.
     * 미노출 호스트는 폴링을 건너뛴다(현상 유지). 동적 명령 Tier 3 진입점 앞에서
     * 호출한다 — 실패한 조회는 조용히 캐시를 유지한다(다음 호출이 재시도).
     */
    const resyncIfStale = () => {
        if (typeof native.getSchemaGeneration !== 'function')
            return;
        let current;
        try {
            current = native.getSchemaGeneration();
        }
        catch {
            return; // 폴링 실패는 치명적이지 않다 — 기존 캐시로 진행.
        }
        if (cachedGeneration !== undefined && current === cachedGeneration)
            return;
        try {
            refreshLiveSchema();
        }
        catch {
            // 재조회 실패 시 기존 캐시 유지 — 시끄러운 실패는 invoke 결과에서 낸다.
        }
    };
    return {
        refreshLiveSchema,
        readLiveSchemaDocument,
        lookupCachedLiveSchemaEntry,
        resyncIfStale,
        get resyncEpoch() {
            return resyncEpoch;
        },
    };
}
//# sourceMappingURL=rkyv-engine-schema.js.map