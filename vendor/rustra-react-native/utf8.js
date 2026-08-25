const nativeEncoder = typeof TextEncoder === 'undefined' ? undefined : new TextEncoder();
const nativeDecoder = typeof TextDecoder === 'undefined' ? undefined : new TextDecoder();
/** UTF-8 encode without assuming WHATWG encoding globals exist (Hermes-safe). */
export function encodeUtf8(input) {
    if (nativeEncoder)
        return nativeEncoder.encode(input);
    let byteLength = 0;
    for (let index = 0; index < input.length; index += 1) {
        const first = input.charCodeAt(index);
        if (first < 0x80)
            byteLength += 1;
        else if (first < 0x800)
            byteLength += 2;
        else if (first >= 0xd800 && first <= 0xdbff) {
            const second = index + 1 < input.length ? input.charCodeAt(index + 1) : -1;
            if (second >= 0xdc00 && second <= 0xdfff) {
                byteLength += 4;
                index += 1;
            }
            else
                byteLength += 3;
        }
        else
            byteLength += 3;
    }
    const output = new Uint8Array(byteLength);
    let cursor = 0;
    for (let index = 0; index < input.length; index += 1) {
        const first = input.charCodeAt(index);
        if (first < 0x80) {
            output[cursor++] = first;
        }
        else if (first < 0x800) {
            output[cursor++] = 0xc0 | (first >> 6);
            output[cursor++] = 0x80 | (first & 0x3f);
        }
        else if (first >= 0xd800 && first <= 0xdbff) {
            const second = index + 1 < input.length ? input.charCodeAt(index + 1) : -1;
            if (second >= 0xdc00 && second <= 0xdfff) {
                index += 1;
                const point = 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00);
                output[cursor++] = 0xf0 | (point >> 18);
                output[cursor++] = 0x80 | ((point >> 12) & 0x3f);
                output[cursor++] = 0x80 | ((point >> 6) & 0x3f);
                output[cursor++] = 0x80 | (point & 0x3f);
            }
            else {
                output.set([0xef, 0xbf, 0xbd], cursor);
                cursor += 3;
            }
        }
        else if (first >= 0xdc00 && first <= 0xdfff) {
            output.set([0xef, 0xbf, 0xbd], cursor);
            cursor += 3;
        }
        else {
            output[cursor++] = 0xe0 | (first >> 12);
            output[cursor++] = 0x80 | ((first >> 6) & 0x3f);
            output[cursor++] = 0x80 | (first & 0x3f);
        }
    }
    return output;
}
/** UTF-8 decode without assuming WHATWG encoding globals exist (Hermes-safe). */
export function decodeUtf8(input) {
    const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
    if (nativeDecoder)
        return nativeDecoder.decode(bytes);
    let output = '';
    let index = 0;
    while (index < bytes.length) {
        const first = bytes[index];
        if (first < 0x80) {
            output += String.fromCharCode(first);
            index += 1;
            continue;
        }
        const width = first >= 0xf0 ? 4 : first >= 0xe0 ? 3 : first >= 0xc2 ? 2 : 0;
        if (width === 0 || index + width > bytes.length) {
            output += '\ufffd';
            index += 1;
            continue;
        }
        let point = first & (0x7f >> width);
        let valid = true;
        for (let offset = 1; offset < width; offset += 1) {
            const next = bytes[index + offset];
            if ((next & 0xc0) !== 0x80) {
                valid = false;
                break;
            }
            point = (point << 6) | (next & 0x3f);
        }
        const minimum = width === 2 ? 0x80 : width === 3 ? 0x800 : 0x10000;
        if (!valid || point < minimum || point > 0x10ffff || (point >= 0xd800 && point <= 0xdfff)) {
            output += '\ufffd';
            index += 1;
            continue;
        }
        if (point <= 0xffff)
            output += String.fromCharCode(point);
        else {
            const adjusted = point - 0x10000;
            output += String.fromCharCode(0xd800 + (adjusted >> 10), 0xdc00 + (adjusted & 0x3ff));
        }
        index += width;
    }
    return output;
}
/** Return an exact ArrayBuffer even when the Uint8Array is a sub-view. */
export function exactArrayBuffer(bytes) {
    if (bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength) {
        return bytes.buffer;
    }
    return bytes.slice().buffer;
}
//# sourceMappingURL=utf8.js.map