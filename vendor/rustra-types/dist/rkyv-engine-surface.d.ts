import type { InternalEngineClient } from './global.js';
import type { InvokeOptions, RkyvV2Engine } from './public.js';
import type { RkyvDispatchRuntime, RkyvEngineContext, RkyvRouteRuntime } from './rkyv-engine-context.js';
export declare function createRkyvEngineSurface(context: RkyvEngineContext, dispatch: RkyvDispatchRuntime, routes: RkyvRouteRuntime, invokeRaw: <T>(command: string, args?: unknown, options?: InvokeOptions) => Promise<T>): RkyvV2Engine & InternalEngineClient;
//# sourceMappingURL=rkyv-engine-surface.d.ts.map