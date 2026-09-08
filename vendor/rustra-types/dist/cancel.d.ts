import type { EngineClient, InvokeOptions } from './public.js';
export { invokeByIdWithTimeout } from './cancel-by-id.js';
export { raceAbort } from './cancel-abort.js';
export declare function invokeWithTimeout<T>(engine: EngineClient, command: string, args?: unknown, options?: InvokeOptions): Promise<T>;
export declare function invokeWithTimeoutHandledSignal<T>(engine: EngineClient, command: string, args?: unknown, options?: InvokeOptions): Promise<T>;
export declare function invokeCallbackWithAbort<T>(command: string, signal: AbortSignal, dispatch: (resolve: (value: T) => void, reject: (reason: unknown) => void, isSettled: () => boolean) => number | void, cancel?: (invocationId: number) => void): Promise<T>;
//# sourceMappingURL=cancel.d.ts.map