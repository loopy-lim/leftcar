import { decodeUtf8, encodeUtf8 } from './utf8.js';
// ── postcard wire helpers (codegen `_pc*` 미러) ─────────────
const I64_MIN = -(2n ** 63n);
const I64_MAX = 2n ** 63n - 1n;
const U64_MAX = 2n ** 64n - 1n;
function encVarint(n) {
    n = Math.floor(n);
    if (n < 0)
        throw new Error('varint must be non-negative: ' + n);
    if (n === 0)
        return new Uint8Array([0]);
    const bytes = [];
    while (n > 0) {
        let b = n % 128;
        n = Math.floor(n / 128);
        if (n > 0)
            b += 128;
        bytes.push(b);
    }
    return new Uint8Array(bytes);
}
function decVarint(buf, offset) {
    let value = 0;
    let multiplier = 1;
    let bytesRead = 0;
    while (true) {
        const b = buf[offset + bytesRead];
        if (b === undefined)
            throw new Error('varint out of bounds');
        value += (b & 0x7f) * multiplier;
        bytesRead++;
        if ((b & 0x80) === 0)
            break;
        multiplier *= 128;
        if (bytesRead > 10)
            throw new Error('varint too long');
    }
    return { value, bytesRead };
}
function encVarint64(v) {
    if (typeof v === 'number' && Number.isSafeInteger(v) && v >= 0)
        return encVarint(v);
    let value = BigInt(v);
    if (value < 0n)
        throw new Error('varint must be non-negative: ' + value.toString());
    if (value > U64_MAX)
        throw new Error('varint exceeds u64 range: ' + value.toString());
    const bytes = [];
    do {
        let next = Number(value & 0x7fn);
        value >>= 7n;
        if (value !== 0n)
            next |= 0x80;
        bytes.push(next);
    } while (value !== 0n);
    return new Uint8Array(bytes);
}
function decVarint64(buf, offset) {
    let num = 0;
    let multiplier = 1;
    let big = 0n;
    let bytesRead = 0;
    while (true) {
        const b = buf[offset + bytesRead];
        if (b === undefined)
            throw new Error('varint out of bounds');
        bytesRead++;
        if (bytesRead <= 7) {
            num += (b & 0x7f) * multiplier;
            multiplier *= 128;
            if ((b & 0x80) === 0)
                return { value: num, bytesRead };
        }
        else {
            if (bytesRead === 8)
                big = BigInt(num);
            big |= BigInt(b & 0x7f) << BigInt(7 * (bytesRead - 1));
            if ((b & 0x80) === 0) {
                if (bytesRead === 10 && (b & 0x7f) > 0x01)
                    throw new Error('varint exceeds 64 bits');
                const asNumber = Number(big);
                return { value: Number.isSafeInteger(asNumber) ? asNumber : big, bytesRead };
            }
        }
        if (bytesRead >= 10)
            throw new Error('varint too long');
    }
}
function zigzagEncode(n) {
    return n >= 0 ? n * 2 : -n * 2 - 1;
}
function zigzagDecode(n) {
    const negative = n % 2 === 1;
    const magnitude = Math.floor(n / 2);
    return negative ? -magnitude - 1 : magnitude;
}
function encZigzagVarint(n) {
    return encVarint(zigzagEncode(n));
}
function decZigzagVarint(buf, offset) {
    const { value, bytesRead } = decVarint(buf, offset);
    return { value: zigzagDecode(value), bytesRead };
}
function encZigzag64(v) {
    const n = BigInt(v);
    if (n < I64_MIN || n > I64_MAX)
        throw new Error('zigzag64 input outside i64 range: ' + n.toString());
    return encVarint64((n << 1n) ^ (n >> 63n));
}
function decZigzag64(v) {
    const decoded = (BigInt(v) >> 1n) ^ -(BigInt(v) & 1n);
    const asNumber = Number(decoded);
    return Number.isSafeInteger(asNumber) ? asNumber : decoded;
}
function concatBytes(arrays) {
    let totalLen = 0;
    for (const a of arrays)
        totalLen += a.length;
    const result = new Uint8Array(totalLen);
    let offset = 0;
    for (const a of arrays) {
        result.set(a, offset);
        offset += a.length;
    }
    return result;
}
function utf8Encode(s) {
    return encodeUtf8(s);
}
function utf8Decode(bytes) {
    return decodeUtf8(bytes);
}
function encString(s) {
    const bytes = utf8Encode(s);
    return concatBytes([encVarint(bytes.length), bytes]);
}
function decString(buf, offset) {
    const len = decVarint(buf, offset);
    const start = offset + len.bytesRead;
    const end = start + len.value;
    return {
        value: utf8Decode(buf.slice(start, end)),
        bytesRead: len.bytesRead + len.value,
    };
}
function encF64(n) {
    const buf = new ArrayBuffer(8);
    new DataView(buf).setFloat64(0, n, true);
    return new Uint8Array(buf);
}
function decF64(buf, offset) {
    return {
        value: new DataView(buf.buffer, buf.byteOffset + offset, 8).getFloat64(0, true),
        bytesRead: 8,
    };
}
function encF32(n) {
    const buf = new ArrayBuffer(4);
    new DataView(buf).setFloat32(0, n, true);
    return new Uint8Array(buf);
}
function decF32(buf, offset) {
    return {
        value: new DataView(buf.buffer, buf.byteOffset + offset, 4).getFloat32(0, true),
        bytesRead: 4,
    };
}
export { concatBytes, decF32, decF64, decString, decVarint, decVarint64, decZigzag64, decZigzagVarint, encF32, encF64, encString, encVarint, encVarint64, encZigzag64, encZigzagVarint, };
//# sourceMappingURL=schema-postcard-wire.js.map