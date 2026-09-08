import { ComplexCodecError } from './complex-codec-types.js';
import { utf8Decode } from './complex-codec-wire.js';
export class Reader {
    bytes;
    maxCollectionLength;
    offset = 0;
    constructor(bytes, maxCollectionLength) {
        this.bytes = bytes;
        this.maxCollectionLength = maxCollectionLength;
    }
    get position() {
        return this.offset;
    }
    get remaining() {
        return this.bytes.length - this.offset;
    }
    byte() {
        this.need(1);
        return this.bytes[this.offset++];
    }
    need(length) {
        if (!Number.isSafeInteger(length) || length < 0 || this.remaining < length) {
            throw new ComplexCodecError('truncated complex payload');
        }
    }
    raw(length) {
        this.need(length);
        const result = this.bytes.slice(this.offset, this.offset + length);
        this.offset += length;
        return result;
    }
    varint() {
        let value = 0n;
        for (let shift = 0n; shift < 70n; shift += 7n) {
            const byte = this.byte();
            value |= BigInt(byte & 0x7f) << shift;
            if ((byte & 0x80) === 0)
                return value;
        }
        throw new ComplexCodecError('varint is too long');
    }
    zigzag() {
        const value = this.varint();
        return (value >> 1n) ^ -(value & 1n);
    }
    length() {
        const value = this.varint();
        if (value > BigInt(this.maxCollectionLength) || value > BigInt(Number.MAX_SAFE_INTEGER)) {
            throw new ComplexCodecError(`collection length exceeds ${this.maxCollectionLength}`);
        }
        return Number(value);
    }
    string() {
        return utf8Decode(this.raw(this.length()));
    }
}
//# sourceMappingURL=complex-codec-reader.js.map