/** UTF-8 encode without assuming WHATWG encoding globals exist (Hermes-safe). */
export declare function encodeUtf8(input: string): Uint8Array;
/** UTF-8 decode without assuming WHATWG encoding globals exist (Hermes-safe). */
export declare function decodeUtf8(input: ArrayBuffer | Uint8Array, start?: number, end?: number): string;
/** Return an exact ArrayBuffer even when the Uint8Array is a sub-view. */
export declare function exactArrayBuffer(bytes: Uint8Array): ArrayBuffer;
//# sourceMappingURL=utf8.d.ts.map