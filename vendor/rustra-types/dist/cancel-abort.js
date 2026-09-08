import { CancelledError } from './errors.js';
export function raceAbort(promise, signal, command) {
    return new Promise((resolve, reject) => {
        const onAbort = () => reject(new CancelledError(`invoke("${command}") aborted`));
        signal.addEventListener('abort', onAbort, { once: true });
        promise.then((value) => {
            signal.removeEventListener('abort', onAbort);
            resolve(value);
        }, (error) => {
            signal.removeEventListener('abort', onAbort);
            reject(error);
        });
    });
}
//# sourceMappingURL=cancel-abort.js.map