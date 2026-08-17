/**
 * @rustra/types — rustra 브릿지의 핵심 타입 및 글로벌 invoke
 *
 * 모든 플랫폼 어댑터(Node, Bun, Tauri, React Native)가 공유하는
 * EngineClient 인터페이스, 에러 타입, rkyv V2 코덱,
 * 그리고 Tauri-like 글로벌 invoke를 제공합니다.
 *
 * @example
 * ```ts
 * // 설정 (플랫폼별, 한 번만)
 * import { configure } from '@rustra/types';
 * import { createRkyvV2Engine } from '@rustra/react-native';
 * configure(createRkyvV2Engine(native, registry));
 *
 * // 사용 (어디서든, 타입 안전)
 * import { addNumbers } from './generated/commands.js';
 * const result = await addNumbers({ a: 42, b: 58 });
 * ```
 */
export class RustraCommandError extends Error {
    code;
    /** 재시도 가능한 에러인지 — Rust `RustraError::is_retryable` 와이어 값을 그대로 노출 */
    retryable;
    constructor(code, message, retryable = false) {
        super(message);
        this.name = 'RustraCommandError';
        this.code = code;
        this.retryable = retryable;
    }
}
/**
 * Rust `RustraError::Display` 포맷(`"{code}: {message}"`)의 평탄화된 문자열을
 * [`RustraCommandError`]로 파싱한다. JSON fallback 경로(RN/Lynx)에서 사용 —
 * rkyv V2 경로(Node/Tauri)는 구조화된 `{code, message}` 객체를 받으므로 불필요.
 *
 * `": "` 앞이 dot-notation 코드 토큰(`command.not_found`, `internal`,
 * `math.divide_by_zero` 등 — 소문자/숫자/`.`/`_` 만)이면 code/message 를 분리하고,
 * 그렇지 않으면(FFI 수준 에러: `"json decode failed: ..."`, `"payload exceeds size limit"`
 * 등) `invoke.failed` 코드에 전체 문자열을 message 로 쓴다.
 */
export function parseRustraErrorString(error) {
    const raw = error ?? 'Rustra invoke failed';
    const idx = raw.indexOf(': ');
    if (idx > 0) {
        const code = raw.slice(0, idx);
        if (/^[a-z][a-z0-9_.]*$/.test(code)) {
            return new RustraCommandError(code, raw.slice(idx + 2), isRetryableCode(code));
        }
    }
    return new RustraCommandError('invoke.failed', raw);
}
/**
 * 코드 기반 retryable 추론 — Rust `RustraError` 팩토리 관례와 정합.
 * `transport.error`/`transport.timeout`은 Rust 생성 시점에 `retryable: true`로
 * 설정되는 유일한 코드군이다 (구조화 와이어에는 retryable 플래그가 없으므로
 * 코드에서 도출한다).
 */
function isRetryableCode(code) {
    return code === 'transport.error' || code === 'transport.timeout';
}
// ── Global invoke (Tauri-like) ──────────────────────────────
let _engine = null;
/**
 * 글로벌 엔진을 설정합니다. 앱 시작 시 한 번만 호출합니다.
 *
 * @param engine - 플랫폼별로 생성한 EngineClient
 *
 * @example
 * ```ts
 * // React Native
 * import { configure } from '@rustra/types';
 * import { createRkyvV2Engine } from '@rustra/react-native';
 * configure(createRkyvV2Engine(native, rkyvV2Registry));
 *
 * // Node
 * import { configure } from '@rustra/types';
 * import { createRkyvV2Engine } from '@rustra/node';
 * configure(createRkyvV2Engine(nativeAddon, rkyvV2Registry));
 *
 * // Bun
 * import { configure } from '@rustra/types';
 * import { createRkyvV2Engine } from '@rustra/bun';
 * configure(createRkyvV2Engine(ffi, rkyvV2Registry));
 * ```
 */
export function configure(engine) {
    _engine = engine;
}
/**
 * 글로벌 엔진으로 명령을 호출합니다.
 *
 * 일반적으로 직접 호출하지 않고, 코드젠이 생성한 명령 함수를 사용합니다.
 *
 * @example
 * ```ts
 * const result = await invoke<AddNumbersOutput>('addNumbers', { a: 42, b: 58 });
 * // 또는:
 * const result = await addNumbers({ a: 42, b: 58 });
 * ```
 */
export function invoke(command, args) {
    if (!_engine) {
        throw new Error('Rustra not configured. Call configure(engine) first.');
    }
    return _engine.invoke(command, args);
}
/**
 * 글로벌 엔진으로 여러 명령을 한 번에 호출합니다 (P0-2 invokeBatch).
 *
 * 정적 명령만 있으면 단일 네이티브 횡단으로 일괄 처리되어 잦은 호출의 jank 를 줄이고,
 * 동적 명령이 섞이면 항목별로 자동 라우팅됩니다.
 *
 * @example
 * ```ts
 * const [a, b] = await invokeBatch([
 *   { command: 'addNumbers', args: { a: 1, b: 2 } },
 *   { command: 'multiply', args: { a: 3, b: 4 } },
 * ]);
 * ```
 */
export function invokeBatch(entries) {
    if (!_engine) {
        throw new Error('Rustra not configured. Call configure(engine) first.');
    }
    if (!_engine.invokeBatch) {
        throw new Error('Configured engine does not support invokeBatch.');
    }
    return _engine.invokeBatch(entries);
}
// ── Runtime-safe UTF-8 helpers ─────────────────────────────
// Lynx's QuickJS runtime has no TextEncoder/TextDecoder globals, so the engine
// must not depend on them. Pure-JS UTF-8 codec (surrogate-pair correct).
function _utf8Encode(s) {
    const out = [];
    for (let i = 0; i < s.length; i++) {
        const c = s.charCodeAt(i);
        if (c < 0x80) {
            out.push(c);
        }
        else if (c < 0x800) {
            out.push(0xc0 | (c >> 6), 0x80 | (c & 0x3f));
        }
        else if (c >= 0xd800 && c <= 0xdbff) {
            const low = s.charCodeAt(++i);
            const cp = 0x10000 + ((c - 0xd800) << 10) + (low - 0xdc00);
            out.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3f), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
        }
        else {
            out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
        }
    }
    return new Uint8Array(out);
}
function _utf8Decode(bytes, start, end) {
    let s = '';
    let i = start;
    while (i < end) {
        const b = bytes[i];
        if (b < 0x80) {
            s += String.fromCharCode(b);
            i += 1;
        }
        else if ((b & 0xe0) === 0xc0) {
            s += String.fromCharCode(((b & 0x1f) << 6) | (bytes[i + 1] & 0x3f));
            i += 2;
        }
        else if ((b & 0xf0) === 0xe0) {
            s += String.fromCharCode(((b & 0x0f) << 12) | ((bytes[i + 1] & 0x3f) << 6) | (bytes[i + 2] & 0x3f));
            i += 3;
        }
        else if ((b & 0xf8) === 0xf0) {
            const cp = ((b & 0x07) << 18) |
                ((bytes[i + 1] & 0x3f) << 12) |
                ((bytes[i + 2] & 0x3f) << 6) |
                (bytes[i + 3] & 0x3f);
            const adj = cp - 0x10000;
            s += String.fromCharCode(0xd800 + (adj >> 10), 0xdc00 + (adj & 0x3ff));
            i += 4;
        }
        else {
            i += 1;
        }
    }
    return s;
}
/**
 * 네이티브 getSchema() 로부터 현재 명령 스키마를 조회한다 (정적 + 동적 명령 포함).
 * 동적 명령의 commandId/타입을 알아내 rkyvV2 Tier 3 fallback 에 사용된다.
 */
export function getLiveSchema(native) {
    if (!native.getSchema) {
        return new Map();
    }
    const bytes = native.getSchema();
    const u = new Uint8Array(bytes);
    const json = _utf8Decode(u, 0, u.length);
    const parsed = JSON.parse(json);
    const map = new Map();
    for (const c of parsed.commands ?? []) {
        map.set(c.name, {
            commandId: c.commandId,
            inputSchema: c.inputSchema,
            outputSchema: c.outputSchema,
        });
    }
    return map;
}
// ── Tier 3 (JSON-in-binary) wire helpers ────────────────────
// request:  [command_id: u16 LE @0][json @2]
// success:  [ok:1 @0][pad 3B][json_len: u32 LE @4][json @8]
// error:    [ok:0 @0][pad to @8][err_len: u16 LE @8][postcard({code,message}) @10]
function encodeTier3Request(commandId, args) {
    const json = _utf8Encode(JSON.stringify(args ?? {}, _jsonSetReplacer));
    const buf = new Uint8Array(2 + json.length);
    new DataView(buf.buffer).setUint16(0, commandId, true);
    buf.set(json, 2);
    return buf.buffer;
}
/**
 * JSON 경로에서 `Set`을 배열로 직렬화한다 — Rust `BTreeSet`/`HashSet`은
 * serde JSON 에서 배열로 직렬화되므로 와이어 호환을 맞춘다
 * (`Map`은 rustra 계약에 없으므로 다루지 않는다).
 */
function _jsonSetReplacer(_key, value) {
    if (value instanceof Set)
        return [...value];
    return value;
}
// postcard varint + length-prefixed string decode, local to the Tier 3 path so
// this file has no dependency on the generated codec helpers.
function _tier3DecodeString(u, offset) {
    let shift = 0;
    let bytesRead = 0;
    let len = 0;
    while (true) {
        const b = u[offset + bytesRead];
        len |= (b & 0x7f) << shift;
        bytesRead++;
        if ((b & 0x80) === 0)
            break;
        shift += 7;
        if (bytesRead > 5)
            throw new Error('varint too long');
    }
    len = len >>> 0;
    const start = offset + bytesRead;
    return {
        value: _utf8Decode(u, start, start + len),
        bytesRead: bytesRead + len,
    };
}
function decodeTier3Response(bytes) {
    const u = new Uint8Array(bytes);
    if (u[0] === 1) {
        const len = new DataView(bytes).getUint32(4, true);
        const json = _utf8Decode(u, 8, 8 + len);
        return { ok: true, result: JSON.parse(json) };
    }
    const errLen = new DataView(bytes).getUint16(8, true);
    let error = { code: 'invoke.failed', message: 'invoke failed' };
    if (errLen > 0) {
        // postcard({ code: String, message: String })
        const { value: code, bytesRead: b1 } = _tier3DecodeString(u, 10);
        const { value: message } = _tier3DecodeString(u, 10 + b1);
        error = { code, message };
    }
    return { ok: false, error };
}
export function createRkyvV2Engine(native, registry, options) {
    // F5 (opt-in): 계약 해시 검증. 빌드 시점 hash 와 네이티브 실시간 hash 가 다르면
    // 엔진을 만들지 않고 즉시 실패(fail-fast)한다.
    if (options?.contractHash !== undefined) {
        if (typeof native.getContractHash !== 'function') {
            throw new RustraCommandError('contract.unenforceable', 'contractHash option was set but the native module does not expose ' +
                'getContractHash(); cannot verify schema drift');
        }
        const hashBytes = new Uint8Array(native.getContractHash());
        const nativeHash = _utf8Decode(hashBytes, 0, hashBytes.length).trim();
        if (nativeHash !== options.contractHash) {
            throw new RustraCommandError('contract.mismatch', `contract hash mismatch: native="${nativeHash.slice(0, 16)}…" vs ` +
                `expected="${options.contractHash.slice(0, 16)}…" — generated client ` +
                `and native binary are out of sync; regenerate the client`);
        }
    }
    // B1 fast path: 네이티브가 C++ typed 코덱(invokeTyped + hasStaticCodec)을 노출하면
    // 정적 명령을 C++에서 postcard 인코딩/디코딩한다 (JS codec 왕복 ~3.4µs 제거).
    const hasTypedPath = !!(native.invokeTyped && native.hasStaticCodec);
    // P0-2: 단일 횡단 배치가 가능하려면 invokeTypedBatch 도 필요.
    const hasBatchPath = hasTypedPath && !!native.invokeTypedBatch;
    return {
        invoke(command, args) {
            // 1순위: C++ fast path (RN JSI). 정적 명령만.
            if (hasTypedPath && native.hasStaticCodec(command)) {
                return Promise.resolve(native.invokeTyped(command, args));
            }
            // 2순위: JS codec (Node/Bun/Tauri 또는 typed 누락 시). 정적 명령.
            const codec = registry.get(command);
            if (codec) {
                const resultBytes = native.invokeRkyvV2(codec.encode(args));
                const response = codec.decode(resultBytes);
                if (!response.ok) {
                    const e = response.error ?? { code: 'invoke.failed', message: 'RkyvV2 invoke failed' };
                    // Reject (do not throw) so the declared Promise<T> contract holds and
                    // callers can use .catch() / await-try-consistently for command errors.
                    return Promise.reject(new RustraCommandError(e.code, e.message, e.retryable ?? isRetryableCode(e.code)));
                }
                return Promise.resolve(response.result);
            }
            // 3순위: 동적 명령 → Tier 3 fallback (live schema 의 commandId 사용)
            const entry = getLiveSchema(native).get(command);
            if (!entry) {
                return Promise.reject(new RustraCommandError('command.not_found', `RkyvV2: no codec and not in live schema for "${command}"`));
            }
            const resp = decodeTier3Response(native.invokeRkyvV2(encodeTier3Request(entry.commandId, args)));
            if (!resp.ok) {
                const e = resp.error ?? { code: 'invoke.failed', message: 'RkyvV2 (tier3) invoke failed' };
                return Promise.reject(new RustraCommandError(e.code, e.message, e.retryable ?? isRetryableCode(e.code)));
            }
            return Promise.resolve(resp.result);
        },
        invokeBatch(entries) {
            // 모든 항목이 정적 코덱이면 단일 JSI 횡단(invokeTypedBatch)으로 일괄 처리.
            if (hasBatchPath &&
                entries.length > 0 &&
                entries.every((e) => native.hasStaticCodec(e.command))) {
                const names = entries.map((e) => e.command);
                const args = entries.map((e) => e.args);
                const results = native.invokeTypedBatch(names, args);
                return Promise.resolve(results);
            }
            // 동적 명령이 섞였거나 배치 미지원 → 항목별 라우팅(typed/Tier3 자동 분기).
            return Promise.all(entries.map((e) => this.invoke(e.command, e.args)));
        },
    };
}
//# sourceMappingURL=index.js.map