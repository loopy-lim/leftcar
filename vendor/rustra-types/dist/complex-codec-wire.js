import { ComplexCodecError } from './complex-codec-types.js';
export function utf8Encode(value) {
    if (typeof TextEncoder !== 'undefined')
        return new TextEncoder().encode(value);
    const bytes = unescape(encodeURIComponent(value));
    const output = new Uint8Array(bytes.length);
    for (let i = 0; i < bytes.length; i += 1)
        output[i] = bytes.charCodeAt(i);
    return output;
}
export function utf8Decode(value) {
    if (typeof TextDecoder !== 'undefined')
        return new TextDecoder('utf-8', { fatal: true }).decode(value);
    let binary = '';
    for (const byte of value)
        binary += String.fromCharCode(byte);
    try {
        return decodeURIComponent(escape(binary));
    }
    catch {
        throw new ComplexCodecError('invalid UTF-8 string');
    }
}
export function sortedKeys(value) {
    return Object.keys(value).sort(compareUtf8);
}
export function compareUtf8(left, right) {
    const a = utf8Encode(left);
    const b = utf8Encode(right);
    const length = Math.min(a.length, b.length);
    for (let i = 0; i < length; i += 1) {
        if (a[i] !== b[i])
            return a[i] - b[i];
    }
    return a.length - b.length;
}
export class Writer {
    maxPayloadBytes;
    parts = [];
    length = 0;
    constructor(maxPayloadBytes) {
        this.maxPayloadBytes = maxPayloadBytes;
    }
    push(bytes) {
        if (this.length + bytes.length > this.maxPayloadBytes) {
            throw new ComplexCodecError(`payload exceeds ${this.maxPayloadBytes} bytes`);
        }
        this.parts.push(bytes);
        this.length += bytes.length;
    }
    byte(value) {
        this.push(new Uint8Array([value]));
    }
    varint(value) {
        if (value < 0n)
            throw new ComplexCodecError('varint cannot be negative');
        const bytes = [];
        do {
            let next = Number(value & 0x7fn);
            value >>= 7n;
            if (value !== 0n)
                next |= 0x80;
            bytes.push(next);
        } while (value !== 0n);
        this.push(new Uint8Array(bytes));
    }
    zigzag(value) {
        this.varint(value >= 0n ? value * 2n : -value * 2n - 1n);
    }
    string(value) {
        const bytes = utf8Encode(value);
        this.varint(BigInt(bytes.length));
        this.push(bytes);
    }
    finish() {
        const output = new Uint8Array(this.length);
        let offset = 0;
        for (const part of this.parts) {
            output.set(part, offset);
            offset += part.length;
        }
        return output.buffer;
    }
}
//# sourceMappingURL=complex-codec-wire.js.map