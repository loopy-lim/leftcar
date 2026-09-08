import { CODEC_TYPED } from './global.js';
export function createRkyvCapabilityRuntime(native, registry) {
    // B1 fast path: 네이티브가 C++ typed 코덱(invokeTyped + hasStaticCodec)을 노출하면
    // 정적 명령을 C++에서 postcard 인코딩/디코딩한다 (JS codec 왕복 ~3.4µs 제거).
    const hasLegacyTypedPath = !!(native.invokeTyped && native.hasStaticCodec);
    const hasCapabilityPath = typeof native.getCodecCapabilities === 'function' &&
        typeof native.invokeTypedById === 'function';
    const hasTypedPath = hasLegacyTypedPath || hasCapabilityPath;
    // P0-3: byId 진입(invokeTypedById)이 가능한지 — 가능하면 dispatch 1순위가
    // JSI 1회 횡단 + u16 디스패치로 바뀐다. 미노출이면 이름 기반 invokeTyped 유지.
    const hasByIdPath = hasTypedPath && typeof native.invokeTypedById === 'function';
    const hasRawPath = hasCapabilityPath && typeof native.invokeTypedRaw === 'function';
    const hasPositionalPath = hasCapabilityPath && typeof native.invokeTypedPos === 'function';
    const hasBufferPath = hasCapabilityPath && typeof native.invokeTypedBuffer === 'function';
    // P0-2: 단일 횡단 배치가 가능하려면 invokeTypedBatch 도 필요.
    const hasBatchPath = hasTypedPath && !!native.invokeTypedBatch;
    // P0-2 byId: 배치 진입의 cmd_id 배열 변형 — 문자열 마샬링 N 회 제거.
    const hasBatchByIdPath = hasBatchPath && typeof native.invokeTypedBatchById === 'function';
    // 정적 명령 집합 JS 캐시 (P0-3) — hasStaticCodec JSI 호출을 호출당 1회에서
    // 엔진 생애 1회 스윕으로 축소한다. 불변식: 코드젠 시점 정적 명령은 항상
    // registry 에 있다 (registry 도 코드젠 산출물). registry 에 없는 이름은
    // 동적 명령 → Tier 3 경로. 스윕은 registry 를 기준으로 하므로 C++ 코덱만
    // 있고 registry 에 빠진 정적 명령(불변식 위반)도 자연스럽게 Tier 3 로 간다.
    // 스윕 도중 예외 시 부분 맵 재사용 — 미스윕 항목은 registry 안 이름이므로
    // Tier 2(JS codec)로 라우팅된다. Tier 3는 registry 밖 동적 명령 전용 경로다.
    let staticCommandIds = null;
    let staticCommandNamesById = null;
    let staticCommandCapabilitiesById = null;
    const ensureStaticIds = () => {
        if (staticCommandIds !== null || !hasTypedPath)
            return staticCommandIds;
        staticCommandIds = new Map();
        staticCommandNamesById = [];
        staticCommandCapabilitiesById = [];
        for (const [name, codec] of registry) {
            const capabilities = hasCapabilityPath
                ? native.getCodecCapabilities(codec.commandId)
                : native.hasStaticCodec(name)
                    ? CODEC_TYPED
                    : 0;
            if ((capabilities & CODEC_TYPED) !== 0) {
                staticCommandIds.set(name, codec.commandId);
                staticCommandNamesById[codec.commandId] = name;
                staticCommandCapabilitiesById[codec.commandId] = capabilities;
            }
        }
        return staticCommandIds;
    };
    const isVerifiedStaticId = (commandId, command) => {
        ensureStaticIds();
        return staticCommandNamesById?.[commandId] === command;
    };
    const getStaticCommandName = (commandId) => {
        ensureStaticIds();
        return staticCommandNamesById?.[commandId];
    };
    const getStaticCommandCapabilities = (commandId) => {
        ensureStaticIds();
        return staticCommandCapabilitiesById?.[commandId] ?? 0;
    };
    return {
        hasCapabilityPath,
        hasTypedPath,
        hasByIdPath,
        hasRawPath,
        hasPositionalPath,
        hasBufferPath,
        hasBatchPath,
        hasBatchByIdPath,
        ensureStaticIds,
        getStaticCommandName,
        getStaticCommandCapabilities,
        isVerifiedStaticId,
    };
}
//# sourceMappingURL=rkyv-engine-capabilities.js.map