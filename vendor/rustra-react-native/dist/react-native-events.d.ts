export type RustraEventNative = {
    onEvent?(name: string, callback: (payloadJson: string) => void): void;
    offEvent?(name: string): void;
    /** JS 폴링 drain(CallInvoker 없는 호스트). 처리된 이벤트+채널 프레임 수 반환. */
    drainEvents?(): number;
};
export type RustraChannelNative = {
    createChannel?(callback: (payloadJson: string) => void): number;
    /** 바이너리 채널 — 콜백이 임의 바이트를 받는다(C++ createChannelBytes HostFunction). */
    createChannelBytes?(callback: (payload: ArrayBuffer | Uint8Array) => void): number;
    dropChannel?(handle: number): boolean;
    /** JS 폴링 drain(CallInvoker 없는 호스트) — 이벤트와 채널 프레임을 함께
     * 소비하고 처리한 프레임 수를 반환한다(C++ drainEvents HostFunction). */
    drainEvents?(): number;
};
export declare function createChannel(callback: (payload: unknown) => void, native?: RustraChannelNative, options?: PollingDrainOptions): {
    readonly handle: number;
    close(): boolean;
};
/**
 * 바이너리 채널 생성 — 콜백은 rkyv V2 프레임 등 임의 바이트(ArrayBuffer)를
 * 받는다. JSON 경로(`createChannel`)와 동일한 핸들/close 계약, 한 핸들은 한
 * 경로로만 동작한다. 네이티브가 `createChannelBytes` 를 노출하지 않으면
 * `channel.unavailable` 로 loud-fail 한다.
 */
export declare function createBytesChannel(callback: (payload: Uint8Array) => void, native?: RustraChannelNative, options?: PollingDrainOptions): {
    readonly handle: number;
    close(): boolean;
};
type PollingDrainOptions = {
    /**
     * 폴링 drain 간격(ms) — `drainEvents` 를 노출하는 네이티브(CallInvoker 없는
     * 호스트)에서 JS 측 폴링 루프를 켠다. C++ 디스패처는 CallInvoker 없으면 큐에
     * 쌓아두고 JS 의 `drainEvents()` 폴링을 기다린다(RustraJSIBridge.cpp) — 이
     * 옵션이 없으면 그 큐가 영원히 소비되지 않는다. 기본 꺼짐(CallInvoker 호스트에
     * 서 drain 폴링이 불필요하고, 푸시 경로와 병행해도 무해하다 — drain 이 비어
     * 있으면 0).
     */
    pollMs?: number;
};
type SubscribeOptions = {
    allowMissingNative?: boolean;
} & PollingDrainOptions;
export declare function subscribeEvent(name: string, cb: (payload: unknown) => void, options?: SubscribeOptions): () => void;
/** 동기 invoke 표면의 최소 구조 — 네이티브 전체(RustraJSINative) 없이도 테스트/부분 목(mock)이 가능하다. */
export type RustraSyncNative = {
    invokeTyped?(name: string, args: unknown): unknown;
};
/**
 * 동기 typed invoke — UI 핫패스 등 Promise 오버헤드를 제거하는 경로.
 * C++ `invokeTyped` fast path(encode → FFI → decode)를 그대로 쓰며 반환값은
 * 디코딩된 출력 그 자체다. 정적 코덱이 없는 명령/네이티브는
 * `sync.unavailable` 로 loud-fail 한다(폴백 정책은 호출자 소관).
 *
 * 계약: JS 런타임 스레드에서만 호출(JSI 스레드 친화성 — 다른 네이티브 경로와
 * 동일). 커맨드 에러는 `RustraCommandError`(code/message 유지)로 재발행된다.
 */
export declare function invokeTypedSync<T = unknown>(name: string, args?: unknown, native?: RustraSyncNative): T;
export {};
//# sourceMappingURL=react-native-events.d.ts.map