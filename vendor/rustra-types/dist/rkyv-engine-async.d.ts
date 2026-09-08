import type { RkyvDispatchRuntime, RkyvEngineContext } from './rkyv-engine-context.js';
export declare function createRkyvInvokeRaw(context: RkyvEngineContext, dispatch: RkyvDispatchRuntime): <T>(command: string, args?: unknown, options?: import('./public.js').InvokeOptions) => Promise<T>;
//# sourceMappingURL=rkyv-engine-async.d.ts.map