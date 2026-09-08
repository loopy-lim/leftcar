export declare function utf8Encode(value: string): Uint8Array;
export declare function utf8Decode(value: Uint8Array): string;
export declare function sortedKeys(value: Record<string, unknown>): string[];
export declare function compareUtf8(left: string, right: string): number;
export declare class Writer {
    private readonly maxPayloadBytes;
    private readonly parts;
    private length;
    constructor(maxPayloadBytes: number);
    push(bytes: Uint8Array): void;
    byte(value: number): void;
    varint(value: bigint): void;
    zigzag(value: bigint): void;
    string(value: string): void;
    finish(): ArrayBuffer;
}
//# sourceMappingURL=complex-codec-wire.d.ts.map