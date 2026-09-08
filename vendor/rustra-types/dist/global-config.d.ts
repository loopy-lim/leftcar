import type { EngineClient, InvokeOptions } from './public.js';
/** configure/configureLazy 의 선택적 등록자 식별 — 충돌 진단 메시지에만 쓰인다. */
export type ConfigureOptions = {
    /** 등록 주체(호스트/어댑터) 식별자 — 경쟁 등록 거부 시 양쪽 주체를 보고한다. */
    ownerId?: string;
};
export declare function configure(engine: EngineClient, options?: ConfigureOptions): void;
export declare function configureLazy(initializer: () => EngineClient | Promise<EngineClient>, options?: ConfigureOptions): void;
export declare function isLazyConfigured(): boolean;
export declare function ensureConfigured(): Promise<EngineClient>;
export declare function resolveCommandId(commandFn: (...args: never[]) => unknown): string;
export declare function invoke<T>(command: string, args?: unknown, options?: InvokeOptions): Promise<T>;
/** @internal — codegen import contract; see note atop global-fields.ts. */
export declare function invokeGenerated<T>(commandId: number, command: string, args?: unknown, options?: InvokeOptions): Promise<T>;
//# sourceMappingURL=global-config.d.ts.map