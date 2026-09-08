import { validateRkyvEngineOptions } from './rkyv-engine-contract.js';
import { createRkyvSchemaRuntime } from './rkyv-engine-schema.js';
import { createRkyvCapabilityRuntime } from './rkyv-engine-capabilities.js';
import { createRkyvDispatchRuntime } from './rkyv-engine-dispatch.js';
import { createRkyvRouteRuntime } from './rkyv-engine-routes.js';
import { createRkyvInvokeRaw } from './rkyv-engine-async.js';
import { createRkyvEngineSurface } from './rkyv-engine-surface.js';
/**
 * rkyv V2 네이티브 모듈로 EngineClient을 생성한다.
 *
 * 정적 명령은 codegen codec registry 로 fast-path(postcard). registry 에 없는
 * 동적(런타임 등록) 명령은 live schema 에서 commandId 를 조회해 Tier 3(JSON) 로
 * fallback 한다. 단일 엔진이 정적 + 동적 모두 처리한다.
 */
export function createRkyvV2Engine(native, registry, options) {
    const schema = createRkyvSchemaRuntime(native);
    validateRkyvEngineOptions(native, options, schema);
    const capabilities = createRkyvCapabilityRuntime(native, registry);
    const context = {
        native,
        registry,
        schema,
        capabilities,
        payloadLimit: options?.maxPayloadBytes,
    };
    const dispatch = createRkyvDispatchRuntime(context);
    const routes = createRkyvRouteRuntime(context, dispatch.dispatchById);
    const invokeRaw = createRkyvInvokeRaw(context, dispatch);
    return createRkyvEngineSurface(context, dispatch, routes, invokeRaw);
}
//# sourceMappingURL=rkyv-engine.js.map