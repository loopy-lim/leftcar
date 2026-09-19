import type { FrameDispatchRuntime, FrameEngineContext } from './frame-engine-context.js';
export declare function createFrameInvokeRaw(context: FrameEngineContext, dispatch: FrameDispatchRuntime): <T>(command: string, args?: unknown, options?: import('./public.js').InvokeOptions) => Promise<T>;
//# sourceMappingURL=frame-engine-async.d.ts.map