import { validateFrameEngineOptions } from './frame-engine-contract.js';
import { createFrameSchemaRuntime } from './frame-engine-schema.js';
import { createFrameCapabilityRuntime } from './frame-engine-capabilities.js';
import { createFrameDispatchRuntime } from './frame-engine-dispatch.js';
import { createFrameRouteRuntime } from './frame-engine-routes.js';
import { createFrameInvokeRaw } from './frame-engine-async.js';
import { createFrameEngineSurface } from './frame-engine-surface.js';
/**
 * Frame 네이티브 모듈로 EngineClient을 생성한다.
 *
 * 정적 명령은 codegen codec registry 로 fast-path(postcard). registry 에 없는
 * 동적(런타임 등록) 명령은 live schema 에서 commandId 를 조회해 Tier 3(JSON) 로
 * fallback 한다. 단일 엔진이 정적 + 동적 모두 처리한다.
 */
export function createFrameEngine(native, registry, options) {
    const schema = createFrameSchemaRuntime(native);
    validateFrameEngineOptions(native, options, schema);
    const capabilities = createFrameCapabilityRuntime(native, registry);
    const context = {
        native,
        registry,
        schema,
        capabilities,
        payloadLimit: options?.maxPayloadBytes,
    };
    const dispatch = createFrameDispatchRuntime(context);
    const routes = createFrameRouteRuntime(context, dispatch.dispatchById);
    const invokeRaw = createFrameInvokeRaw(context, dispatch);
    return createFrameEngineSurface(context, dispatch, routes, invokeRaw);
}
//# sourceMappingURL=frame-engine.js.map