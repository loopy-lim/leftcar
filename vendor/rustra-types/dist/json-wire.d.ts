import type { RustraError } from './errors.js';
export declare function encodeTier3Request(commandId: number, args: unknown): ArrayBuffer;
export declare function decodeTier3Response(bytes: ArrayBuffer | ArrayBufferView): {
    ok: boolean;
    result?: unknown;
    error?: RustraError;
};
//# sourceMappingURL=json-wire.d.ts.map