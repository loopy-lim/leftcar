import { type FastEngineOptions, type ReactNativeEngine, type RustraJSINative } from './react-native-core.js';
export type RustraJSIAsyncNative = RustraJSINative & {
    invokeTypedAsync?(name: string, args: unknown, onSuccess: (result: unknown) => void, onError: (message: string) => void): number | void;
    /**
     * (G2) id 인덱싱 async 진입 — 이름 문자열 마샬링 제거. 정적 명령(코드젠
     * registry 안)은 byId 우선, 미노출 또는 registry 밖 동적 명령은 이름 경로
     * 폴백(P0-3 sync byId 패턴과 동일 계약).
     */
    invokeTypedAsyncById?(commandId: number, args: unknown, onSuccess: (result: unknown) => void, onError: (message: string) => void): number | void;
    invokeCancel?(invocationId: number): boolean;
};
export declare function createAsyncEngine(native: RustraJSIAsyncNative, options: FastEngineOptions): ReactNativeEngine;
//# sourceMappingURL=react-native-async.d.ts.map