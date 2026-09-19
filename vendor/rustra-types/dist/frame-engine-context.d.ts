import type { GeneratedBytesRoute, GeneratedFieldsRoute } from './global.js';
import type { LiveSchemaDocument, LiveSchemaEntry, FrameSchemaNative } from './live-schema.js';
import type { FrameCodec } from './public.js';
export type FrameSchemaRuntime = {
    refreshLiveSchema(): Map<string, LiveSchemaEntry>;
    readLiveSchemaDocument(): LiveSchemaDocument;
    lookupCachedLiveSchemaEntry(command: string): LiveSchemaEntry | undefined;
    /** (T0-3) 세대 불일치 시 live schema 캐시를 재동기화한다(미노출 호스트는 no-op). */
    resyncIfStale(): void;
    /**
     * (T0-3 후속) live schema 재조회 때마다 오르는 에포크 — 파생 캐시가 세대
     * 변경을 감지해 스스로를 비우는 데 쓴다(동적 코덱 캐시 prune).
     */
    readonly resyncEpoch: number;
};
export type FrameCapabilityRuntime = {
    hasCapabilityPath: boolean;
    hasTypedPath: boolean;
    hasByIdPath: boolean;
    hasRawPath: boolean;
    hasPositionalPath: boolean;
    hasBufferPath: boolean;
    hasBatchPath: boolean;
    hasBatchByIdPath: boolean;
    ensureStaticIds(): Map<string, number> | null;
    getStaticCommandName(commandId: number): string | undefined;
    getStaticCommandCapabilities(commandId: number): number;
    isVerifiedStaticId(commandId: number, command: string): boolean;
};
export type FrameEngineContext = {
    native: FrameSchemaNative;
    registry: Map<string, FrameCodec<unknown, unknown>>;
    schema: FrameSchemaRuntime;
    capabilities: FrameCapabilityRuntime;
    payloadLimit: number | undefined;
};
export type FrameDispatchRuntime = {
    dispatch<T>(command: string, args?: unknown): T;
    dispatchPromise<T>(command: string, args?: unknown): Promise<T>;
    dispatchById<T>(commandId: number, command: string, args?: unknown): T;
    dispatchPromiseById<T>(commandId: number, command: string, args?: unknown): Promise<T>;
    dispatchGeneratedFields<T>(commandId: number, command: string, args: unknown, fieldCount: 1 | 2 | 3, field0: unknown, field1?: unknown, field2?: unknown): T;
};
export type FrameRouteRuntime = {
    resolveGeneratedFieldsRoute(commandId: number, command: string, fieldCount: 1 | 2 | 3): GeneratedFieldsRoute | undefined;
    resolveGeneratedBytesRoute(commandId: number, command: string): GeneratedBytesRoute | undefined;
};
//# sourceMappingURL=frame-engine-context.d.ts.map