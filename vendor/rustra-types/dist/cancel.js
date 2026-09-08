import { CancelledError, RustraCommandError, TimeoutError } from './errors.js';
export { invokeByIdWithTimeout } from './cancel-by-id.js';
export { raceAbort } from './cancel-abort.js';
export function invokeWithTimeout(engine, command, args, options) {
    return invokeWithTimeoutInternal(engine, command, args, options, true);
}
export function invokeWithTimeoutHandledSignal(engine, command, args, options) {
    return invokeWithTimeoutInternal(engine, command, args, options, false);
}
function invokeWithTimeoutInternal(engine, command, args, options, handleSignal) {
    const signal = handleSignal ? options?.signal : undefined;
    if (signal?.aborted)
        return Promise.reject(new CancelledError(`invoke("${command}") aborted before dispatch`));
    let promise;
    try {
        promise = Promise.resolve(engine.invoke(command, args, options));
    }
    catch (error) {
        return Promise.reject(error);
    }
    const ms = options?.timeoutMs;
    if (ms === undefined && signal === undefined)
        return promise;
    void promise.catch(() => { });
    // dispatch 동기 구간 안에서 abort 가 이미 발화했을 수 있다(transport 가
    // invoke 내부에서 controller.abort() 후 정상 반환). 리스너 등록은 dispatch
    // 뒤에 있으므로 abort 이벤트를 놓쳤다 — 등록 직후 재검사가 이 갭을 닫는다.
    // 재검사는 레이스 참여가 아니라 선정착 반환이어야 한다: 이미 fulfill 된
    // promise 가 races[0] 로 먼저 연결되면 Promise.race 는 동시 정착에서
    // 그쪽을 이기게 하므로, 같은 틱에 reject 되는 레이스 항목은 무시된다.
    // 선정착 반환은 위의 흡수자 뒤에 둔다 — dispatch promise 가 나중에
    // reject 해도 unhandled rejection 이 되지 않는다(레이스 경로와 동일).
    if (signal?.aborted)
        return Promise.reject(new CancelledError(`invoke("${command}") aborted`));
    let timer;
    let onAbort;
    const races = [promise];
    if (signal)
        races.push(new Promise((_, reject) => {
            onAbort = () => reject(new CancelledError(`invoke("${command}") aborted`));
            signal.addEventListener('abort', onAbort, { once: true });
        }));
    if (ms !== undefined)
        races.push(new Promise((_, reject) => {
            timer = setTimeout(() => reject(new TimeoutError(`invoke("${command}") timed out after ${ms}ms`)), ms);
        }));
    return Promise.race(races).finally(() => {
        if (timer !== undefined)
            clearTimeout(timer);
        if (signal && onAbort)
            signal.removeEventListener('abort', onAbort);
    });
}
export function invokeCallbackWithAbort(command, signal, dispatch, cancel) {
    if (signal.aborted)
        return Promise.reject(new CancelledError(`invoke("${command}") aborted before dispatch`));
    return new Promise((resolve, reject) => {
        let settled = false;
        let invocationId = -1;
        const cleanup = () => signal.removeEventListener('abort', onAbort);
        const finish = (outcome) => {
            if (settled)
                return;
            settled = true;
            cleanup();
            outcome();
        };
        const onAbort = () => finish(() => {
            let cancelFailure;
            if (cancel && invocationId >= 0) {
                try {
                    cancel(invocationId);
                }
                catch (error) {
                    cancelFailure = error; // JS 결과 확정을 native cancel 성공에 묶지 않는다
                }
            }
            reject(new CancelledError(`invoke("${command}") aborted`, cancelFailure));
            // cancel 실패는 cause로 보존 — 통합 문서 "별도 관측 정보"의 최소 구현.
        });
        signal.addEventListener('abort', onAbort, { once: true });
        try {
            const id = dispatch((value) => finish(() => resolve(value)), (reason) => finish(() => reject(reason)), () => settled);
            if (typeof id === 'number')
                invocationId = id;
        }
        catch (error) {
            finish(() => reject(error instanceof Error
                ? error
                : new RustraCommandError('invoke.failed', `invoke("${command}") dispatch failed: ${String(error)}`)));
        }
    });
}
//# sourceMappingURL=cancel.js.map