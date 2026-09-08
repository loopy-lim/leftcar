export const DEFAULT_MAX_DEPTH = 32;
export const DEFAULT_MAX_PAYLOAD_BYTES = 1024 * 1024;
export const DEFAULT_MAX_COLLECTION_LENGTH = 100_000;
export class ComplexCodecError extends Error {
    constructor(message) {
        super(message);
        this.name = 'ComplexCodecError';
    }
}
//# sourceMappingURL=complex-codec-types.js.map