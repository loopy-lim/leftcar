import type { BatchEntry, BootstrapState, EngineClient as EngineClientType, EngineSupports, RkyvV2Engine, RkyvV2EngineOptions, RkyvV2SchemaNative, RustraNative } from '@rustra/types';
export type ReactNativeEngine = EngineClientType & {
    invokeBatch<T>(entries: BatchEntry[]): Promise<T[]>;
};
export type RustraJSINative = RkyvV2SchemaNative & {
    invoke(payload: ArrayBuffer): ArrayBuffer;
    onEvent?(name: string, callback: (payloadJson: string) => void): void;
    offEvent?(name: string): void;
    drainEvents?(): number;
    createChannel?(callback: (payloadJson: string) => void): number;
    dropChannel?(handle: number): boolean;
};
/**
 * RN JSON 어댑터의 기술적 지표(A02) — compatibility-matrix.md 의 RN
 * `createReactNativeEngine` 열 셀을 그대로 옮긴 것: in-flight 취소는 얕은
 * 취소(JS 프라미스만 거부), 배치는 per-entry 폴백, 이벤트 미지원(❌ JSON
 * adapter), 채널은 JSI handle + close(), 동기 native 호출은 실행 중 선점 불가
 * (timeoutMs 레이스 없음 — 유일한 false 셀).
 */
export declare const REACT_NATIVE_JSON_ENGINE_SUPPORTS: EngineSupports;
/**
 * RN rkyv V2 엔진의 기술적 지표(A02) — compatibility-matrix.md 의 RN
 * `createRkyvV2Engine` 열 셀을 그대로 옮긴 것: 취소는 조건부 전파(JS 코덱 +
 * invokeAsync/invokeCancel 확인 시 Rust 체크포인트까지 — 정적 typed 경로는
 * 얕은 취소 폴백), 배치는 정적 명령 단일 횡단(signal 항목은 항목별 라우팅),
 * 이벤트 푸시(CallInvoker 자동 drain), 채널 JSI handle, timeoutMs 레이스 있음.
 */
export declare const REACT_NATIVE_RKYV_V2_ENGINE_SUPPORTS: EngineSupports;
export declare function createReactNativeEngine(native: {
    invoke(payload: ArrayBuffer): ArrayBuffer;
}): ReactNativeEngine;
export type FastEngineOptions = {
    rkyvV2Codecs: Map<string, import('@rustra/types').RkyvV2Codec<unknown, unknown>>;
} & RkyvV2EngineOptions;
export type RustraBootstrapOptions = FastEngineOptions & {
    install(): Promise<void>;
    getNative(): RustraJSINative;
};
export type RustraBootstrap = {
    /**
     * bootstrap 수명 상태(A05) — 공용 `BootstrapState`(@rustra/types).
     * dispose 는 멱등이고 dispose 후 ready 는 loud-fail 한다.
     */
    readonly state: BootstrapState;
    ready(): Promise<RkyvV2Engine>;
    /** (A05) dispose-once — 두 번째 호출은 no-op. JS reload 는 네이티브 drift 를 못 고친다. */
    dispose(): void;
};
export declare function createRustraBootstrap(options: RustraBootstrapOptions): RustraBootstrap;
export declare function getRustraNative(): RustraJSINative & RustraNative;
export declare function createFastEngine(native: RustraJSINative, options: FastEngineOptions): RkyvV2Engine;
//# sourceMappingURL=react-native-core.d.ts.map