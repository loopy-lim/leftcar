import { RustraCommandError, RustraErrorCode } from './errors.js';
/** dispose 후 ready 재진입 loud-fail 계약의 공용 에러 — 어댑터별 메시지 접두 포함. */
export function disposedBootstrapError(adapter, detail) {
    const suffix = detail ? ` ${detail}` : '';
    return new RustraCommandError(RustraErrorCode.TransportUnavailable, `This ${adapter} bootstrap has been disposed. Create a new bootstrap to re-initialize ` +
        `the engine — ready() after dispose() is rejected instead of silently re-resolving.` +
        suffix);
}
//# sourceMappingURL=bootstrap-lifecycle.js.map