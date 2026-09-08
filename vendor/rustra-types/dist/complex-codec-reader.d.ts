export declare class Reader {
    private readonly bytes;
    private readonly maxCollectionLength;
    private offset;
    constructor(bytes: Uint8Array, maxCollectionLength: number);
    get position(): number;
    get remaining(): number;
    byte(): number;
    need(length: number): void;
    raw(length: number): Uint8Array;
    varint(): bigint;
    zigzag(): bigint;
    length(): number;
    string(): string;
}
//# sourceMappingURL=complex-codec-reader.d.ts.map