import { CancelledError } from './errors.js';
import { invokeWithTimeoutHandledSignal } from './cancel.js';
import { raceAbort } from './cancel.js';
import { invokeByIdSync, invokeGeneratedBytesSync, invokeGeneratedFieldsSync, resolveGeneratedBytesSync, resolveGeneratedFieldsSync, } from './global.js';
export function createRkyvEngineSurface(context, dispatch, routes, invokeRaw) {
    const { native, schema, capabilities } = context;
    const { dispatchById, dispatchPromiseById, dispatchGeneratedFields } = dispatch;
    const { resolveGeneratedFieldsRoute, resolveGeneratedBytesRoute } = routes;
    const { hasByIdPath, hasBatchPath, hasBatchByIdPath, ensureStaticIds, isVerifiedStaticId } = capabilities;
    const invokeEngine = { invoke: invokeRaw };
    return {
        refreshLiveSchema: schema.refreshLiveSchema,
        [invokeByIdSync](commandId, command, args) {
            return dispatchById(commandId, command, args);
        },
        [invokeGeneratedFieldsSync](commandId, command, args, fieldCount, field0, field1, field2) {
            return dispatchGeneratedFields(commandId, command, args, fieldCount, field0, field1, field2);
        },
        [resolveGeneratedFieldsSync](commandId, command, fieldCount) {
            return resolveGeneratedFieldsRoute(commandId, command, fieldCount);
        },
        [invokeGeneratedBytesSync](commandId, command, args, value) {
            const route = resolveGeneratedBytesRoute(commandId, command);
            return route
                ? route(args, value)
                : dispatchGeneratedFields(commandId, command, args, 1, value);
        },
        [resolveGeneratedBytesSync](commandId, command) {
            return resolveGeneratedBytesRoute(commandId, command);
        },
        invoke(command, args, options) {
            return invokeWithTimeoutHandledSignal(invokeEngine, command, args, options);
        },
        invokeById(commandId, command, args, options) {
            const signal = options?.signal;
            if (signal?.aborted) {
                return Promise.reject(new CancelledError(`invoke("${command}") aborted before dispatch`));
            }
            if (!signal) {
                return invokeWithTimeoutHandledSignal({
                    invoke: () => dispatchPromiseById(commandId, command, args),
                }, command, args, options);
            }
            // 검증된 typed-by-id 명령은 기존 invoke의 typed 경로와 동일하게 얕은
            // 취소를 적용한다. 검증 실패/구 네이티브는 기존 이름 경로가 취소 전파
            // 가능 여부를 판단하도록 위임한다.
            if (hasByIdPath && isVerifiedStaticId(commandId, command)) {
                return invokeWithTimeoutHandledSignal({
                    invoke: () => raceAbort(dispatchPromiseById(commandId, command, args), signal, command),
                }, command, args, options);
            }
            return invokeRaw(command, args, options);
        },
        invokeBatch(entries) {
            // 계약: 단일 JSI 횡단 배치(invokeTypedBatch[ById])는 취소를 지원하지
            // 않는다 — signal 이 붙은 항목이 하나라도 있으면 자동으로 항목별
            // invoke 경로(각자의 전파/얕은 취소 정책)로 라우팅된다. 배치 자체의
            // 항목별 취소 지원은 명시적 미지원 계약 (followup-3 유예 유지).
            //
            // 모든 항목이 정적 코덱이고 signal 이 없어야 단일 JSI 횡단으로 일괄 처리.
            // 단일 횡단 진입은 2단계: byId 배치(invokeTypedBatchById) 가 우선, 미노출이면
            // 이름 기반 invokeTypedBatch(아래 분기 참조). 정적 여부/id 조사는 캐시
            // 조회로 한다 (P0-3: hasStaticCodec JSI 호출 N 회 → 엔진 생애 1회 스윕).
            const staticIds = hasBatchPath && entries.length > 0 ? ensureStaticIds() : null;
            if (staticIds &&
                entries.every((e) => staticIds.has(e.command)) &&
                entries.every((e) => !e.options?.signal)) {
                const args = entries.map((e) => e.args);
                // byId 진입(P0-2 후속): 네이티브가 cmd_id 배열 배치를 노출하면 문자열
                // 배열 마샬링 없이 id 로 단일 횡단. 모든 항목의 id 가 캐시에 있는 위의
                // every 검사가 이미 조립 가능성을 보장한다.
                try {
                    if (hasBatchByIdPath) {
                        const ids = entries.map((e) => staticIds.get(e.command));
                        const results = native.invokeTypedBatchById(ids, args);
                        return Promise.resolve(results);
                    }
                    const names = entries.map((e) => e.command);
                    const results = native.invokeTypedBatch(names, args);
                    return Promise.resolve(results);
                }
                catch (error) {
                    return Promise.reject(error);
                }
            }
            // 동적 명령/시그널 항목이 섞였거나 배치 미지원 → 항목별 라우팅.
            // 항목의 options(signal) 를 그대로 실어 보내 항목 단위 취소가 각자의
            // 취소 정책(전파/얕은)을 따르게 한다 (T1 후속).
            return Promise.all(entries.map((e) => this.invoke(e.command, e.args, e.options)));
        },
    };
}
//# sourceMappingURL=rkyv-engine-surface.js.map