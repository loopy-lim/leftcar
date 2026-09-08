declare function encVarint(n: number): Uint8Array;
declare function decVarint(buf: Uint8Array, offset: number): {
    value: number;
    bytesRead: number;
};
declare function encVarint64(v: number | bigint): Uint8Array;
declare function decVarint64(buf: Uint8Array, offset: number): {
    value: number | bigint;
    bytesRead: number;
};
declare function encZigzagVarint(n: number): Uint8Array;
declare function decZigzagVarint(buf: Uint8Array, offset: number): {
    value: number;
    bytesRead: number;
};
declare function encZigzag64(v: number | bigint): Uint8Array;
declare function decZigzag64(v: number | bigint): number | bigint;
declare function concatBytes(arrays: Uint8Array[]): Uint8Array;
declare function encString(s: string): Uint8Array;
declare function decString(buf: Uint8Array, offset: number): {
    value: string;
    bytesRead: number;
};
declare function encF64(n: number): Uint8Array;
declare function decF64(buf: Uint8Array, offset: number): {
    value: number;
    bytesRead: number;
};
declare function encF32(n: number): Uint8Array;
declare function decF32(buf: Uint8Array, offset: number): {
    value: number;
    bytesRead: number;
};
export { concatBytes, decF32, decF64, decString, decVarint, decVarint64, decZigzag64, decZigzagVarint, encF32, encF64, encString, encVarint, encVarint64, encZigzag64, encZigzagVarint, };
//# sourceMappingURL=schema-postcard-wire.d.ts.map