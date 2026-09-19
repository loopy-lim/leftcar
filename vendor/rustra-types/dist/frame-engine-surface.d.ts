import type { InternalEngineClient } from './global.js';
import type { InvokeOptions, FrameEngine } from './public.js';
import type { FrameDispatchRuntime, FrameEngineContext, FrameRouteRuntime } from './frame-engine-context.js';
export declare function createFrameEngineSurface(context: FrameEngineContext, dispatch: FrameDispatchRuntime, routes: FrameRouteRuntime, invokeRaw: <T>(command: string, args?: unknown, options?: InvokeOptions) => Promise<T>): FrameEngine & InternalEngineClient;
//# sourceMappingURL=frame-engine-surface.d.ts.map