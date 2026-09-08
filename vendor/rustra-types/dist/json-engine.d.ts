import type { BatchEntry, EngineClientWithBatch, EngineSupports } from './public.js';
/**
 * json-engine 이 이미 아는 와이어 배치 표면 — transport 가 단일 IPC 횡단으로
 * N 개 명령을 실행할 수 있으면 제공한다(트랙 E2). 미제공 시 기존 Promise.all
 * 항목별 폴백으로 동작한다(기존 계약 불변).
 */
export type JsonWireBatchTransport = {
    invoke(command: string, args?: unknown): Promise<unknown> | unknown;
    /** 와이어 배치 — `rustra_dispatch_batch` 커맨드 한 번으로 N 개 명령 실행. */
    invokeBatch?(requests: BatchEntry[]): Promise<unknown[]> | unknown[];
};
export declare function createJsonEngine(transport: ((command: string, args?: unknown) => Promise<unknown> | unknown) | JsonWireBatchTransport, normalizeArgs?: (args?: unknown) => unknown, supports?: EngineSupports): EngineClientWithBatch;
//# sourceMappingURL=json-engine.d.ts.map