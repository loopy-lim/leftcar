import assert from 'node:assert/strict';
import test from 'node:test';
import { CancelledError, createReactNativeEngine, createRustraBootstrap, createBytesChannel, createChannel, invokeTypedSync, getRustraNative, RustraCommandError, } from './index.js';
import { decodeUtf8, encodeUtf8, exactArrayBuffer } from '@rustra/types';
const encoder = new TextEncoder();
test('Hermes fallback encodes and decodes Korean and emoji without WHATWG globals', async () => {
    const encoderDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'TextEncoder');
    const decoderDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'TextDecoder');
    Object.defineProperty(globalThis, 'TextEncoder', { configurable: true, value: undefined });
    Object.defineProperty(globalThis, 'TextDecoder', { configurable: true, value: undefined });
    try {
        const input = '안녕하세요 Rustra 🚀';
        const bytes = encodeUtf8(input);
        assert.equal(decodeUtf8(bytes), input);
        assert.deepEqual([...bytes], [...encoder.encode(input)]);
        assert.equal(exactArrayBuffer(bytes).byteLength, bytes.byteLength);
    }
    finally {
        if (encoderDescriptor)
            Object.defineProperty(globalThis, 'TextEncoder', encoderDescriptor);
        else
            Reflect.deleteProperty(globalThis, 'TextEncoder');
        if (decoderDescriptor)
            Object.defineProperty(globalThis, 'TextDecoder', decoderDescriptor);
        else
            Reflect.deleteProperty(globalThis, 'TextDecoder');
    }
});
function createMockNative(returnValue) {
    return {
        invoke(_payload) {
            return encoder.encode(JSON.stringify(returnValue)).buffer;
        },
    };
}
test('missing JSI module error points through native linking to the Rust ABI', () => {
    const globalRecord = globalThis;
    const previous = Object.getOwnPropertyDescriptor(globalRecord, '__rustraNative');
    Reflect.deleteProperty(globalRecord, '__rustraNative');
    try {
        assert.throws(() => getRustraNative(), (error) => error instanceof Error &&
            /Expo Go/.test(error.message) &&
            /Rust static archive/.test(error.message) &&
            /extern "C" FFI symbols/.test(error.message));
    }
    finally {
        if (previous)
            Object.defineProperty(globalRecord, '__rustraNative', previous);
    }
});
test('React Native bootstrap installs and configures once across concurrent readiness', async () => {
    let installs = 0;
    const native = {};
    const bootstrap = createRustraBootstrap({
        install: async () => {
            installs++;
            await Promise.resolve();
        },
        getNative: () => native,
        rkyvV2Codecs: new Map(),
    });
    const [left, right] = await Promise.all([bootstrap.ready(), bootstrap.ready()]);
    assert.equal(installs, 1);
    assert.equal(left, right);
});
test('React Native bootstrap adds native-to-Rust remediation to install failures', async () => {
    const bootstrap = createRustraBootstrap({
        install: async () => {
            throw new Error('ERR_NO_BRIDGE');
        },
        getNative: () => ({}),
        rkyvV2Codecs: new Map(),
    });
    await assert.rejects(bootstrap.ready(), (error) => error instanceof Error &&
        /ERR_NO_BRIDGE/.test(error.message) &&
        /autolinking/.test(error.message) &&
        /Rust FFI symbols/.test(error.message));
});
test('routes invoke through JSI native module', async () => {
    const native = createMockNative({ ok: true, result: { value: 42 } });
    const engine = createReactNativeEngine(native);
    const result = await engine.invoke('addNumbers', { a: 20, b: 22 });
    assert.deepEqual(result, { value: 42 });
});
test('throws on error response', async () => {
    const native = createMockNative({ ok: false, error: 'command not found' });
    const engine = createReactNativeEngine(native);
    await assert.rejects(async () => engine.invoke('missing'), (err) => {
        assert.ok(err instanceof Error);
        assert.equal(err.message, 'command not found');
        return true;
    });
});
test('includes default error message when error is missing', async () => {
    const native = createMockNative({ ok: false });
    const engine = createReactNativeEngine(native);
    await assert.rejects(async () => engine.invoke('cmd'), (err) => {
        assert.ok(err instanceof Error);
        assert.equal(err.message, 'Rustra invoke failed');
        return true;
    });
});
test('JSON adapter rejects a pre-aborted call without crossing native', async () => {
    let calls = 0;
    const engine = createReactNativeEngine({
        invoke() {
            calls++;
            return encoder.encode('{"ok":true,"result":1}').buffer;
        },
    });
    const ac = new AbortController();
    ac.abort();
    await assert.rejects(engine.invoke('cancelled', undefined, { signal: ac.signal }), (error) => error instanceof RustraCommandError && error.code === 'cancelled');
    assert.equal(calls, 0);
});
test('JSON adapter honors timeoutMs through the common timeout contract', async () => {
    const engine = createReactNativeEngine(createMockNative({ ok: true, result: 42 }));
    assert.equal(await engine.invoke('fast', undefined, { timeoutMs: 100 }), 42);
});
test('JSON adapter exposes Promise-based invokeBatch with stable order', async () => {
    const engine = createReactNativeEngine({
        invoke(payload) {
            const request = JSON.parse(decodeUtf8(payload));
            return exactArrayBuffer(encodeUtf8(JSON.stringify({ ok: true, result: request.command === 'first' ? 1 : 2 })));
        },
    });
    const out = await engine.invokeBatch([{ command: 'first' }, { command: 'second' }]);
    assert.deepEqual(out, [1, 2]);
});
// ── Trust-test baselines (Phase 0) ──────────────────────────
// 현재 결함을 "현재 동작"으로 고정한다. Phase 1 수정 후 각 단언이
// 실패하며, 그때 새 동작(Promise.reject / RustraCommandError)으로 전환한다.
test('F3: native.invoke throw is caught by .catch() (Promise<T> honored)', async () => {
    // EngineClient 계약: invoke()는 Promise<T>를 반환해야 하므로,
    // 네이티브 실패는 반드시 rejected Promise여야 한다. async 전환 후
    // 동기 throw 는 rejected Promise 로 정규화되어 .catch() 에서 잡힌다.
    const engine = createReactNativeEngine({
        invoke() {
            throw new Error('native boom');
        },
    });
    let caughtByCatch = false;
    // async invoke 는 동기 throw 를 rejected Promise 로 정규화한다.
    // .catch() 체인을 await 해야 rejection handler(microtask) 가 실행된다.
    await engine.invoke('cmd', {}).catch(() => {
        caughtByCatch = true;
    });
    assert.equal(caughtByCatch, true, 'F3: async invoke converts sync throw to rejected Promise — .catch() must catch it');
});
test('F4: error response rejects with RustraCommandError carrying code', async () => {
    const engine = createReactNativeEngine(createMockNative({ ok: false, error: 'command.not_found: unknown' }));
    await assert.rejects(async () => engine.invoke('cmd', {}), (err) => {
        assert.ok(err instanceof RustraCommandError, 'must be RustraCommandError (code preserved)');
        assert.equal(err.code, 'command.not_found', 'RustaError "code: message" → code parsed');
        assert.match(err.message, /unknown/);
        return true;
    });
});
// ── subscribeEvent (Rust → JS push) ─────────────────────────
import { subscribeEvent } from './index.js';
function createEventNative() {
    const listeners = new Map();
    const calls = [];
    const native = {
        onEvent(name, callback) {
            calls.push({ name, callback });
            listeners.set(name, callback);
        },
        offEvent(name) {
            listeners.delete(name);
        },
    };
    return {
        native,
        listeners,
        calls,
        emit(name, payloadJson) {
            listeners.get(name)?.(payloadJson);
        },
    };
}
test('subscribeEvent registers a native listener and parses the JSON payload once', () => {
    const h = createEventNative();
    const root = globalThis;
    const previous = root.__rustraNative;
    root.__rustraNative = h.native;
    const received = [];
    const payloadJson = JSON.stringify({ step: 1, total: 5 });
    try {
        subscribeEvent('progress.tick', (payload) => {
            received.push(payload);
        });
    }
    finally {
        root.__rustraNative = previous;
    }
    assert.equal(h.calls.length, 1, 'native.onEvent must be called once');
    assert.equal(h.calls[0].name, 'progress.tick');
    h.emit('progress.tick', payloadJson);
    assert.deepEqual(received, [{ step: 1, total: 5 }], 'callback receives the parsed object');
});
test('subscribeEvent supports the canonical name-first shape used by generated events', () => {
    const h = createEventNative();
    const root = globalThis;
    const previous = root.__rustraNative;
    root.__rustraNative = h.native;
    const received = [];
    try {
        subscribeEvent('canonical.tick', (payload) => received.push(payload));
        h.emit('canonical.tick', JSON.stringify({ ok: true }));
        assert.deepEqual(received, [{ ok: true }]);
    }
    finally {
        root.__rustraNative = previous;
    }
});
test('subscribeEvent unsubscribe removes the native listener', () => {
    const h = createEventNative();
    const root = globalThis;
    const previous = root.__rustraNative;
    root.__rustraNative = h.native;
    let unsubscribe;
    try {
        unsubscribe = subscribeEvent('demo.done', () => { });
    }
    finally {
        root.__rustraNative = previous;
    }
    assert.equal(h.listeners.size, 1);
    unsubscribe();
    assert.equal(h.listeners.size, 0, 'offEvent must remove the listener');
});
test('subscribeEvent normalizes unparseable payloads to null', () => {
    const h = createEventNative();
    const root = globalThis;
    const previous = root.__rustraNative;
    root.__rustraNative = h.native;
    const received = [];
    try {
        subscribeEvent('bad.json', (payload) => {
            received.push(payload);
        });
    }
    finally {
        root.__rustraNative = previous;
    }
    h.emit('bad.json', 'not-json{');
    assert.deepEqual(received, [null], 'broken JSON must arrive as null, not throw');
});
test('subscribeEvent fails loudly when native has no event capability', () => {
    const legacy = {};
    const root = globalThis;
    const previous = root.__rustraNative;
    root.__rustraNative = legacy;
    try {
        assert.throws(() => subscribeEvent('any.event', () => { }), (error) => error instanceof RustraCommandError && error.code === 'event.unavailable');
    }
    finally {
        root.__rustraNative = previous;
    }
});
test('subscribeEvent allows an explicit no-op fallback when native lacks events', () => {
    const legacy = {};
    const root = globalThis;
    const previous = root.__rustraNative;
    root.__rustraNative = legacy;
    let unsubscribe;
    try {
        unsubscribe = subscribeEvent('any.event', () => { }, {
            allowMissingNative: true,
        });
    }
    finally {
        root.__rustraNative = previous;
    }
    unsubscribe(); // throw 하지 않아야 한다
});
test('subscribeEvent coexists with multiple event names', () => {
    const h = createEventNative();
    const root = globalThis;
    const previous = root.__rustraNative;
    root.__rustraNative = h.native;
    const ticks = [];
    const dones = [];
    try {
        subscribeEvent('progress.tick', (p) => ticks.push(p));
        subscribeEvent('demo.done', (p) => dones.push(p));
    }
    finally {
        root.__rustraNative = previous;
    }
    h.emit('progress.tick', JSON.stringify({ step: 2 }));
    h.emit('demo.done', JSON.stringify({ emitted: 6 }));
    assert.deepEqual(ticks, [{ step: 2 }]);
    assert.deepEqual(dones, [{ emitted: 6 }]);
});
test('createChannel exposes a typed handle and idempotent close', () => {
    let callback;
    const dropped = [];
    const channel = createChannel((payload) => assert.deepEqual(payload, { chunk: 1 }), {
        createChannel(next) {
            callback = next;
            return 42;
        },
        dropChannel(handle) {
            dropped.push(handle);
            return true;
        },
    });
    assert.equal(channel.handle, 42);
    callback(JSON.stringify({ chunk: 1 }));
    assert.equal(channel.close(), true);
    assert.equal(channel.close(), false);
    assert.deepEqual(dropped, [42]);
});
test('createBytesChannel round-trips binary frames without JSON parsing', () => {
    let callback;
    const dropped = [];
    const frames = [];
    const channel = createBytesChannel((payload) => frames.push(Array.from(payload)), {
        createChannelBytes(next) {
            callback = next;
            return 7;
        },
        dropChannel(handle) {
            dropped.push(handle);
            return true;
        },
    });
    assert.equal(channel.handle, 7);
    callback(new Uint8Array([0xff, 0xfc, 0x00, 0xde]).buffer);
    assert.deepEqual(frames, [[0xff, 0xfc, 0x00, 0xde]], 'bytes must arrive untouched');
    assert.equal(channel.close(), true);
    assert.equal(channel.close(), false);
    assert.deepEqual(dropped, [7]);
});
test('invokeTypedSync returns the decoded value without a Promise hop', () => {
    const native = {
        invokeTyped(name, args) {
            assert.equal(name, 'addNumbers');
            assert.deepEqual(args, { a: 1, b: 2 });
            return { value: 3 };
        },
    };
    const result = invokeTypedSync('addNumbers', { a: 1, b: 2 }, native);
    assert.deepEqual(result, { value: 3 }, 'sync path returns the decoded output directly');
});
test('invokeTypedSync normalizes C++ "code: message" JSError into RustraCommandError', () => {
    const native = {
        invokeTyped() {
            throw new Error("platform.unavailable: command 'x' is declared for platforms [macos]");
        },
    };
    assert.throws(() => invokeTypedSync('x', {}, native), (err) => {
        assert.ok(err instanceof RustraCommandError);
        assert.equal(err.code, 'platform.unavailable');
        return true;
    });
});
test('invokeTypedSync loud-fails on natives without the typed fast path', () => {
    assert.throws(() => invokeTypedSync('x', {}, {}), /invokeTyped/);
});
test('createBytesChannel loud-fails on natives without the bytes path', () => {
    const native = {
        createChannel: () => 1,
        dropChannel: () => true,
    };
    assert.throws(() => createBytesChannel(() => { }, native), /createChannelBytes/);
});
test('createChannel rejects an invalid native handle instead of creating an unusable channel', () => {
    const native = {
        createChannel: () => Number.NaN,
        dropChannel: () => true,
    };
    assert.throws(() => createChannel(() => { }, native), /invalid handle/);
});
// ── createAsyncEngine (P0-3 + T1 얕은 취소) ─────────────────
import { createAsyncEngine, createFastEngine } from './index.js';
// ── FastEngineOptions → core 옵션 전달 (follow-up 2) ───────
// 어댑터는 "전달됐는지"만 검증 — core 동작 상세는 @rustra/types 에서 이미 검증됨.
test('createFastEngine forwards maxPayloadBytes to the core pre-check', async () => {
    // maxPayloadBytes: 8 → 인코딩 후 8B 초과면 payload.too_large 로 네이티브 호출 없이 reject.
    const native = {
        invoke: () => new ArrayBuffer(0),
        invokeRkyvV2: () => {
            throw new Error('native must not be called for an over-limit payload');
        },
    };
    const codec = {
        commandId: 1,
        encode: () => new ArrayBuffer(16), // 16B > 8B limit
        decode: () => ({ ok: true, result: {} }),
    };
    const engine = createFastEngine(native, {
        rkyvV2Codecs: new Map([['big', codec]]),
        maxPayloadBytes: 8,
    });
    await assert.rejects(engine.invoke('big', {}), (err) => err instanceof RustraCommandError && err.code === 'payload.too_large');
});
test('createFastEngine forwards schemaVersion/onSchemaStale (stale warning path)', () => {
    const stale = [];
    const native = {
        invoke: () => new ArrayBuffer(0),
        invokeRkyvV2: () => new ArrayBuffer(8),
        getSchema: () => encoder.encode(JSON.stringify({ schemaVersion: 1, commands: [] })).buffer,
    };
    const engine = createFastEngine(native, {
        rkyvV2Codecs: new Map(),
        schemaVersion: 4,
        onSchemaStale: (info) => stale.push(info),
    });
    // 엔진 생성 시점에 staleness 검사가 돈다 — 옵션이 core 에 닿았으면 기록돼 있다.
    assert.ok(engine, 'engine is created');
    assert.deepEqual(stale, [{ nativeVersion: 1, jsVersion: 4 }]);
});
test('createFastEngine forwards onContractMismatch (degraded mode entry)', () => {
    const mismatches = [];
    const native = {
        invoke: () => new ArrayBuffer(0),
        invokeRkyvV2: () => new ArrayBuffer(0),
        getContractHash: () => encoder.encode('native-hash-AAAA').buffer,
    };
    const engine = createFastEngine(native, {
        rkyvV2Codecs: new Map(),
        contractHash: 'different-hash-BBBB',
        onContractMismatch: (info) => mismatches.push(info),
    });
    assert.ok(engine, 'degraded mode — engine is created instead of throwing');
    assert.deepEqual(mismatches, [
        { nativeHash: 'native-hash-AAAA', expectedHash: 'different-hash-BBBB' },
    ]);
});
/**
 * invokeTypedAsync mock 네이티브 — 성공 콜백을 보류(defer)했다가 수동 전달한다.
 * calls 로 네이티브 호출 수를, resolveNow 로 늦은 resolve 를 흉내낸다.
 */
function makeAsyncNative() {
    const state = {
        calls: 0,
        delivered: false,
        resolveNow: () => { },
        rejectNow: (_msg) => { },
    };
    const native = {
        invoke(_payload) {
            return new ArrayBuffer(0);
        },
        invokeRkyvV2(_payload) {
            return new ArrayBuffer(0);
        },
        invokeTypedAsync(_name, _args, onSuccess, onError) {
            state.calls++;
            state.resolveNow = () => {
                state.delivered = true;
                onSuccess({ value: 42 });
            };
            state.rejectNow = (msg) => {
                state.delivered = true;
                onError(msg);
            };
        },
    };
    return { native, state };
}
test('async engine without signal resolves via invokeTypedAsync (T1 baseline)', async () => {
    const h = makeAsyncNative();
    const engine = createAsyncEngine(h.native, { rkyvV2Codecs: new Map() });
    const p = engine.invoke('heavy', { n: 1 });
    h.state.resolveNow(); // 네이티브 콜백 도착
    const out = await p;
    assert.equal(out.value, 42);
    assert.equal(h.state.calls, 1, 'invokeTypedAsync must be called exactly once');
});
test('async engine exposes Promise-based invokeBatch with stable order', async () => {
    const native = {
        invoke: () => new ArrayBuffer(0),
        invokeRkyvV2: () => new ArrayBuffer(0),
        invokeTypedAsync(name, _args, onSuccess) {
            onSuccess(name === 'first' ? 1 : 2);
            return 0;
        },
    };
    const engine = createAsyncEngine(native, { rkyvV2Codecs: new Map() });
    assert.deepEqual(await engine.invokeBatch([{ command: 'first' }, { command: 'second' }]), [1, 2]);
});
test('createAsyncEngine reports when it falls back to the synchronous engine', () => {
    const native = {
        invoke: () => new ArrayBuffer(0),
        invokeRkyvV2: () => new ArrayBuffer(0),
    };
    const warnings = [];
    const originalWarn = console.warn;
    console.warn = (...args) => warnings.push(args);
    try {
        createAsyncEngine(native, { rkyvV2Codecs: new Map() });
    }
    finally {
        console.warn = originalWarn;
    }
    assert.ok(warnings.some((args) => String(args[0]).includes('synchronous')), 'sync fallback must be visible to the developer');
});
test('async engine without signal rejects via invokeTypedAsync error callback (T1 baseline)', async () => {
    const h = makeAsyncNative();
    const engine = createAsyncEngine(h.native, { rkyvV2Codecs: new Map() });
    const p = engine.invoke('heavy', { n: 1 });
    h.state.rejectNow('math.divide_by_zero: nope');
    await assert.rejects(p, (err) => {
        assert.ok(err instanceof RustraCommandError);
        assert.equal(err.code, 'math.divide_by_zero');
        return true;
    });
});
test('pre-aborted signal rejects cancelled without calling invokeTypedAsync (T1)', async () => {
    const h = makeAsyncNative();
    const engine = createAsyncEngine(h.native, { rkyvV2Codecs: new Map() });
    const ac = new AbortController();
    ac.abort();
    await assert.rejects(engine.invoke('heavy', { n: 1 }, { signal: ac.signal }), (err) => {
        assert.ok(err instanceof RustraCommandError, 'must be RustraCommandError');
        assert.equal(err.code, 'cancelled');
        assert.equal(err.retryable, true, 'cancelled is retryable');
        assert.match(err.message, /heavy/);
        return true;
    });
    assert.equal(h.state.calls, 0, 'native must never be called for a pre-aborted signal');
});
test('abort mid-flight rejects cancelled; late native resolve is ignored (T1)', async () => {
    const h = makeAsyncNative();
    const engine = createAsyncEngine(h.native, { rkyvV2Codecs: new Map() });
    const ac = new AbortController();
    const p = engine.invoke('heavy', { n: 1 }, { signal: ac.signal });
    assert.equal(h.state.calls, 1, 'native must have been dispatched before abort');
    ac.abort(); // 진행 중 중단
    await assert.rejects(p, (err) => {
        assert.ok(err instanceof RustraCommandError);
        assert.equal(err.code, 'cancelled');
        return true;
    });
    // 네이티브 성공 콜백이 abort 이후 늦게 도착 — 이미 정착된 프라미스는 그대로.
    h.state.resolveNow();
    await new Promise((r) => queueMicrotask(() => r()));
    await assert.rejects(p, (err) => err instanceof RustraCommandError && err.code === 'cancelled', 'promise must stay rejected (late resolve is a no-op)');
    assert.equal(h.state.delivered, true, 'native callback did fire — it was just ignored');
});
// ── follow-up 3: invokeTypedAsync id 노출 + 전파형 취소 ────
function makePropagatingAsyncNative() {
    const state = {
        lastId: -1,
        cancels: [],
        resolveNow: (_result) => { },
        rejectNow: (_msg) => { },
    };
    const native = {
        invoke(_payload) {
            return new ArrayBuffer(0);
        },
        invokeRkyvV2(_payload) {
            return new ArrayBuffer(0);
        },
        invokeTypedAsync(_name, _args, onSuccess, onError) {
            state.lastId = 7; // 신형 네이티브 — id 반환
            state.resolveNow = (result) => onSuccess(result);
            state.rejectNow = (msg) => onError(msg);
            return state.lastId;
        },
        invokeCancel(invocationId) {
            state.cancels.push(invocationId);
            return true;
        },
    };
    return { native, state };
}
test('abort mid-flight propagates: invokeCancel(id) is called (follow-up 3)', async () => {
    const h = makePropagatingAsyncNative();
    const engine = createAsyncEngine(h.native, { rkyvV2Codecs: new Map() });
    const ac = new AbortController();
    const p = engine.invoke('heavy', { n: 1 }, { signal: ac.signal });
    assert.equal(h.state.lastId, 7, 'native issued an invocation id');
    ac.abort();
    await assert.rejects(p, (err) => {
        assert.ok(err instanceof RustraCommandError);
        assert.equal(err.code, 'cancelled');
        return true;
    });
    assert.deepEqual(h.state.cancels, [7], 'abort must propagate the invocation id to native.invokeCancel');
    // 늦은 네이티브 성공 콜백 — 이미 정착된 프라미스는 그대로.
    h.state.resolveNow({ value: 42 });
    await new Promise((r) => queueMicrotask(() => r()));
    await assert.rejects(p, (err) => err instanceof RustraCommandError && err.code === 'cancelled', 'promise must stay rejected (late resolve is a no-op)');
});
test('abort mid-flight propagates and late native error is ignored (follow-up 3)', async () => {
    const h = makePropagatingAsyncNative();
    const engine = createAsyncEngine(h.native, { rkyvV2Codecs: new Map() });
    const ac = new AbortController();
    const p = engine.invoke('heavy', { n: 1 }, { signal: ac.signal });
    ac.abort();
    h.state.rejectNow('internal: late failure');
    await assert.rejects(p, (err) => {
        assert.ok(err instanceof RustraCommandError);
        assert.equal(err.code, 'cancelled', 'abort wins over the late error');
        return true;
    });
});
test('signal path without invokeCancel falls back to shallow cancel (follow-up 3)', async () => {
    // 구형 네이티브 — invokeCancel 미노출 (void 반환). 얕은 취소로 폴백해야 한다.
    const h = makeAsyncNative();
    let cancelCalls = 0;
    const native = {
        ...h.native,
        invokeCancel: (_id) => {
            cancelCalls++;
            return false;
        },
    };
    delete native.invokeCancel; // 미노출 시뮬레이션
    const engine = createAsyncEngine(native, { rkyvV2Codecs: new Map() });
    const ac = new AbortController();
    const p = engine.invoke('heavy', { n: 1 }, { signal: ac.signal });
    ac.abort();
    await assert.rejects(p, (err) => {
        assert.ok(err instanceof RustraCommandError);
        assert.equal(err.code, 'cancelled');
        return true;
    });
    assert.equal(cancelCalls, 0, 'no invokeCancel call in the shallow fallback');
});
test('new native without abort resolves normally through the id path (follow-up 3)', async () => {
    const h = makePropagatingAsyncNative();
    const engine = createAsyncEngine(h.native, { rkyvV2Codecs: new Map() });
    const p = engine.invoke('heavy', { n: 1 });
    h.state.resolveNow({ value: 42 });
    const out = await p;
    assert.equal(out.value, 42);
    assert.deepEqual(h.state.cancels, [], 'no cancel without an abort');
});
test('async engine applies timeoutMs and ignores a late native callback', async () => {
    const h = makeAsyncNative();
    const engine = createAsyncEngine(h.native, { rkyvV2Codecs: new Map() });
    const promise = engine.invoke('slow', undefined, { timeoutMs: 10 });
    await assert.rejects(promise, (error) => error instanceof RustraCommandError && error.code === 'transport.timeout');
    h.state.resolveNow();
    await new Promise((resolve) => queueMicrotask(resolve));
    await assert.rejects(promise, (error) => error instanceof RustraCommandError && error.code === 'transport.timeout');
});
// ── G2: async byId 진입 — 이름 마샬링 제거 ───────────────────
function makeByIdAsyncNative() {
    const state = {
        byIdCalls: [],
        nameCalls: [],
        resolveNow: () => { },
        rejectNow: (_msg) => { },
    };
    const native = {
        invoke(_payload) {
            return new ArrayBuffer(0);
        },
        invokeRkyvV2(_payload) {
            return new ArrayBuffer(0);
        },
        invokeTypedAsyncById(cmdId, _args, onSuccess, onError) {
            state.byIdCalls.push({ cmdId, args: _args });
            state.resolveNow = () => onSuccess({ value: 42 });
            state.rejectNow = (msg) => onError(msg);
            return 9;
        },
        invokeTypedAsync(name, _args, onSuccess, onError) {
            state.nameCalls.push(name);
            state.resolveNow = () => onSuccess({ value: 42 });
            state.rejectNow = (msg) => onError(msg);
            return 8;
        },
    };
    return { native, state };
}
test('async engine routes static commands through invokeTypedAsyncById (G2)', async () => {
    const h = makeByIdAsyncNative();
    const registry = new Map([['heavy', { commandId: 5 }]]);
    const engine = createAsyncEngine(h.native, { rkyvV2Codecs: registry });
    const p = engine.invoke('heavy', { n: 1 });
    h.state.resolveNow();
    const out = await p;
    assert.equal(out.value, 42);
    assert.deepEqual(h.state.byIdCalls, [{ cmdId: 5, args: { n: 1 } }], 'static command must enter by cmdId — no name marshalling');
    assert.deepEqual(h.state.nameCalls, [], 'name entry must be bypassed on the byId path');
});
test('async engine falls back to the name path for commands outside the registry (G2)', async () => {
    // registry 밖 = 동적 명령 — C++ 코덱 테이블에 없으므로 byId 로 진입하면 안 된다.
    const h = makeByIdAsyncNative();
    const engine = createAsyncEngine(h.native, {
        rkyvV2Codecs: new Map([['heavy', { commandId: 5 }]]),
    });
    const p = engine.invoke('dynamicCmd', { n: 1 });
    h.state.resolveNow();
    const out = await p;
    assert.equal(out.value, 42);
    assert.deepEqual(h.state.byIdCalls, [], 'dynamic command must not ride the byId path');
    assert.deepEqual(h.state.nameCalls, ['dynamicCmd']);
});
test('async engine without invokeTypedAsyncById keeps the name path (G2 compat)', async () => {
    // 구형 네이티브 — byId 미노출 시 기존 이름 진입 유지.
    const h = makeAsyncNative();
    const engine = createAsyncEngine(h.native, {
        rkyvV2Codecs: new Map([['heavy', { commandId: 5 }]]),
    });
    const p = engine.invoke('heavy', { n: 1 });
    h.state.resolveNow();
    const out = await p;
    assert.equal(out.value, 42);
    assert.equal(h.state.calls, 1, 'name-based invokeTypedAsync must be used');
});
// ── DX Track Task 6 후속: 호스트별 pre-abort instanceof 일치 ──
// 두 어댑터의 pre-abort 경로도 CancelledError 서브클래스로 승격 — abort
// 타이밍(pre-abort vs mid-flight)에 관계없이 같은 instanceof 답을 보장한다.
test('JSON adapter pre-abort rejects with CancelledError instance', async () => {
    const engine = createReactNativeEngine({
        invoke() {
            throw new Error('native must not be called when already aborted');
        },
    });
    const ac = new AbortController();
    ac.abort();
    await assert.rejects(engine.invoke('cancelled', undefined, { signal: ac.signal }), (error) => {
        assert.ok(error instanceof CancelledError, 'JSON adapter pre-abort must be CancelledError');
        assert.ok(error instanceof RustraCommandError, 'CancelledError must remain a RustraCommandError');
        assert.equal(error.code, 'cancelled');
        assert.equal(error.retryable, true);
        assert.match(error.message, /aborted before dispatch/);
        return true;
    });
});
test('async engine pre-abort rejects with CancelledError instance (matches mid-flight)', async () => {
    const h = makeAsyncNative();
    const engine = createAsyncEngine(h.native, { rkyvV2Codecs: new Map() });
    const ac = new AbortController();
    ac.abort();
    await assert.rejects(engine.invoke('heavy', { n: 1 }, { signal: ac.signal }), (err) => {
        assert.ok(err instanceof CancelledError, 'async pre-abort must be CancelledError');
        assert.equal(err.code, 'cancelled');
        assert.equal(err.retryable, true);
        assert.match(err.message, /heavy/);
        return true;
    });
    assert.equal(h.state.calls, 0, 'native must never be called for a pre-aborted signal');
});
// ── A02: EngineSupports 표면 — 매트릭스 셀의 기계 판독 가능한 이행 ─────────
// RN JSON 어댑터 매핑: signal(진행 중 취소) ⚠️ 얕은 취소(JS 프라미스만 거부)
// → 'shallow' / invokeBatch ✅ per-entry Promise fallback → 'per-entry' /
// 이벤트 ❌ JSON adapter → 'none' / 채널 ✅ JSI handle + close() → true /
// timeoutMs ⚠️ 동기 native 호출은 호출 중 선점 불가 → false.
// RN rkyv V2 매핑: 취소 ⚠️ 조건부 전파(invokeAsync/invokeCancel 확인 시 Rust
// 체크포인트까지) → 'cooperative' / 배치 ✅ 정적 명령 단일 횡단 →
// 'single-crossing' / 이벤트 ✅ → 'push' / 채널 ✅ → true / timeoutMs ✅ → true.
test('A02: createReactNativeEngine exposes supports matching the compatibility matrix', async () => {
    const engine = createReactNativeEngine(createMockNative({ ok: true, result: { value: 42 } }));
    assert.deepEqual(engine.supports, {
        cancellation: 'shallow',
        batch: 'per-entry',
        events: 'none',
        channels: true,
        timeoutPreemption: false,
    });
});
test('A02: createAsyncEngine exposes cooperative cancellation supports', async () => {
    // 리뷰 정정 — async invokeBatch 는 항목별 Promise.all 폴백이므로 sync 엔진의
    // `single-crossing` 셀을 상속하지 않는다: batch 는 `per-entry` 재정의.
    const h = makeAsyncNative();
    const engine = createAsyncEngine(h.native, { rkyvV2Codecs: new Map() });
    assert.deepEqual(engine.supports, {
        cancellation: 'cooperative',
        batch: 'per-entry',
        events: 'push',
        channels: true,
        timeoutPreemption: true,
    });
});
// ── A05: bootstrap 수명 상태 모델 (RN createRustraBootstrap) ────────────────
// 각 테스트는 글로벌 슬롯을 configure 로 리셋한 뒤 등록한다(R08 가드 — 소비 전
// 경쟁 등록 loud-fail 은 여기서 재검증 대상이 아니다).
const A05_SLOT_ENGINE = {
    invoke: async () => 'slot',
};
test('A05: createRustraBootstrap exposes the lifecycle state surface', async () => {
    const { configure } = await import('@rustra/types');
    const native = {};
    configure(A05_SLOT_ENGINE);
    try {
        const bootstrap = createRustraBootstrap({
            install: async () => {
                await Promise.resolve();
            },
            getNative: () => native,
            rkyvV2Codecs: new Map(),
        });
        assert.equal(bootstrap.state, 'initializing');
        await bootstrap.ready();
        assert.equal(bootstrap.state, 'ready');
        bootstrap.dispose();
        assert.equal(bootstrap.state, 'disposed');
    }
    finally {
        configure(A05_SLOT_ENGINE);
    }
});
test('A05: ready after dispose rejects loudly (react-native)', async () => {
    const { configure } = await import('@rustra/types');
    configure(A05_SLOT_ENGINE);
    try {
        const bootstrap = createRustraBootstrap({
            install: async () => { },
            getNative: () => ({}),
            rkyvV2Codecs: new Map(),
        });
        bootstrap.dispose();
        await assert.rejects(bootstrap.ready(), (error) => error instanceof Error && /disposed/.test(error.message));
    }
    finally {
        configure(A05_SLOT_ENGINE);
    }
});
test('A05: dispose is idempotent — second dispose is a no-op (react-native)', async () => {
    const { configure } = await import('@rustra/types');
    configure(A05_SLOT_ENGINE);
    try {
        const bootstrap = createRustraBootstrap({
            install: async () => { },
            getNative: () => ({}),
            rkyvV2Codecs: new Map(),
        });
        bootstrap.dispose();
        bootstrap.dispose(); // no-op — must not throw
        assert.equal(bootstrap.state, 'disposed');
    }
    finally {
        configure(A05_SLOT_ENGINE);
    }
});
test('A05: concurrent ready calls share one initialization promise (react-native)', async () => {
    const { configure } = await import('@rustra/types');
    configure(A05_SLOT_ENGINE);
    try {
        let installs = 0;
        const native = {};
        const bootstrap = createRustraBootstrap({
            install: async () => {
                installs++;
                await Promise.resolve();
            },
            getNative: () => native,
            rkyvV2Codecs: new Map(),
        });
        const [a, b] = await Promise.all([bootstrap.ready(), bootstrap.ready()]);
        assert.equal(a, b, 'concurrent ready must share the same engine instance');
        assert.equal(installs, 1, 'install must run exactly once');
        bootstrap.dispose();
    }
    finally {
        configure(A05_SLOT_ENGINE);
    }
});
// ── dev 핫코어 상태 표면(hotCoreStatus) ─────────────────────
// 네이티브 HostFunction 이 구현한다 — JS 표면은 __rustraNative 슬롯 통과 계약
// (정적 모드 = null, 핫 모드 = 마지막 스왑 해시/오류)만 고정한다.
test('hotCoreStatus passes the native swap report through __rustraNative', () => {
    const root = globalThis;
    const previous = root.__rustraNative;
    try {
        // 정적 모드 — 네이티브가 null 을 돌려준다(스왑 표면 없음).
        root.__rustraNative = { hotCoreStatus: () => null };
        assert.equal(getRustraNative().hotCoreStatus?.(), null);
        // 핫 모드 — 마지막 성공 스왑의 구/신 계약 해시를 관측한다.
        root.__rustraNative = {
            hotCoreStatus: () => ({
                enabled: true,
                swapped: true,
                oldHash: 'deadbeef00000000',
                newHash: 'cafebabe11111111',
                error: '',
            }),
        };
        const status = getRustraNative().hotCoreStatus?.();
        assert.ok(status, 'hot mode must report a status object');
        assert.equal(status.enabled, true);
        assert.equal(status.swapped, true);
        assert.equal(status.oldHash, 'deadbeef00000000');
        assert.equal(status.newHash, 'cafebabe11111111');
        assert.equal(status.error, '');
    }
    finally {
        root.__rustraNative = previous;
    }
});
test('hotCoreStatus reports the last swap error through __rustraNative', () => {
    // 실패 스왑 — CLI 게이트 reject 등으로 dlopen 이 거절된 개발자 관측 경로.
    const root = globalThis;
    const previous = root.__rustraNative;
    try {
        root.__rustraNative = {
            hotCoreStatus: () => ({
                enabled: true,
                swapped: false,
                oldHash: '',
                newHash: '',
                error: 'dlopen failed: symbol missing in hot dylib',
            }),
        };
        const status = getRustraNative().hotCoreStatus?.();
        assert.ok(status, 'failed swap must stay observable');
        assert.equal(status.swapped, false);
        assert.match(status.error, /dlopen failed/);
    }
    finally {
        root.__rustraNative = previous;
    }
});
// ── subscribeEvent pollMs — CallInvoker 없는 호스트의 JS 폴링 drain ─────────
test('subscribeEvent pollMs drains queued events from a CallInvoker-less native', async () => {
    // C++ 디스패처 계약 재현: CallInvoker 없으면 emit 이 큐에만 쌓이고 JS 의
    // drainEvents() 폴링을 기다린다. onEvent 콜백은 큐 소비 시점에 호출된다.
    const received = [];
    let drainCount = 0;
    const native = {
        onEvent(name, callback) {
            // 실 C++ HostFunction 과 동일 — 리스너 등록만 하고 큐는 drain 에서 소비.
            listeners.set(name, callback);
        },
        offEvent(name) {
            listeners.delete(name);
        },
        drainEvents() {
            drainCount += 1;
            // 큐에 쌓인 이벤트를 drain 이 소비하며 등록된 콜백을 호출한다.
            const queued = queue.splice(0);
            for (const [name, json] of queued)
                listeners.get(name)?.(json);
            return queued.length;
        },
    };
    const listeners = new Map();
    const queue = [];
    // canonical (name, callback) 형태는 getRustraNative() 로 __rustraNative 를 읽는다.
    const root = globalThis;
    const previous = root.__rustraNative;
    root.__rustraNative = native;
    try {
        const unsubscribe = subscribeEvent('poll.tick', (payload) => received.push(payload), {
            pollMs: 5,
        });
        // emit — CallInvoker 없으므로 큐에만 적재.
        queue.push(['poll.tick', JSON.stringify({ step: 1 })]);
        await new Promise((resolve) => setTimeout(resolve, 30));
        assert.ok(drainCount >= 2, 'polling loop must run repeatedly');
        assert.deepEqual(received, [{ step: 1 }], 'queued event must reach the callback via drain');
        unsubscribe();
        const drainsAtStop = drainCount;
        await new Promise((resolve) => setTimeout(resolve, 25));
        assert.equal(drainCount, drainsAtStop, 'unsubscribe must stop the polling loop');
    }
    finally {
        root.__rustraNative = previous;
    }
});
test('subscribeEvent pollMs delivers directly on onEvent hosts without waiting for drain', () => {
    // CallInvoker 호스트 — onEvent 콜백이 즉시 호출되고 drain 은 비어 있어 무해.
    const received = [];
    const h = createEventNative();
    h.native.drainEvents = () => 0;
    const root = globalThis;
    const previous = root.__rustraNative;
    root.__rustraNative = h.native;
    try {
        const unsubscribe = subscribeEvent('direct.tick', (payload) => received.push(payload), {
            pollMs: 5,
        });
        h.emit('direct.tick', JSON.stringify({ ok: 1 }));
        assert.deepEqual(received, [{ ok: 1 }]);
        unsubscribe();
    }
    finally {
        root.__rustraNative = previous;
    }
});
test('subscribeEvent pollMs is ignored on natives without drainEvents', () => {
    // drainEvents 미노출 — 옵션은 조용히 무시(푸시 전용 네이티브 보호).
    const h = createEventNative();
    const received = [];
    const root = globalThis;
    const previous = root.__rustraNative;
    root.__rustraNative = h.native;
    try {
        const unsubscribe = subscribeEvent('push.only', (payload) => received.push(payload), {
            pollMs: 5,
        });
        h.emit('push.only', JSON.stringify({ v: 1 }));
        assert.deepEqual(received, [{ v: 1 }]);
        unsubscribe();
    }
    finally {
        root.__rustraNative = previous;
    }
});
// ── 채널 pollMs — CallInvoker 없는 호스트의 채널 큐 폴링 drain ─────────────
test('createChannel pollMs drains queued frames from a CallInvoker-less native', async () => {
    // C++ ChannelDispatcher 계약 재현: CallInvoker 없으면 send 가 큐에만 쌓이고 JS 의
    // drainEvents() 폴링을 기다린다(이벤트 폴링 테스트의 채널 변주).
    const received = [];
    let drainCount = 0;
    const callbacks = new Map();
    const queue = [];
    const native = {
        createChannel(callback) {
            callbacks.set(1, callback);
            return 1;
        },
        dropChannel(handle) {
            callbacks.delete(handle);
            return true;
        },
        drainEvents() {
            drainCount += 1;
            // 큐에 쌓인 프레임을 drain 이 소비하며 등록된 콜백을 호출한다.
            const queued = queue.splice(0);
            for (const [handle, json] of queued)
                callbacks.get(handle)?.(json);
            return queued.length;
        },
    };
    const channel = createChannel((payload) => received.push(payload), native, { pollMs: 5 });
    try {
        // send — CallInvoker 없으므로 큐에만 적재.
        queue.push([channel.handle, JSON.stringify({ chunk: 1 })]);
        await new Promise((resolve) => setTimeout(resolve, 30));
        assert.ok(drainCount >= 2, 'polling loop must run repeatedly');
        assert.deepEqual(received, [{ chunk: 1 }], 'queued frame must reach the callback via drain');
    }
    finally {
        channel.close();
    }
});
test('createChannel close releases polling demand — last close stops the loop', async () => {
    // 폴링 루프는 네이티브 인스턴스당 수요 집계 — 채널 하나가 close 해도 다른
    // 채널의 수요가 남으면 유지되고, 마지막 close 로 정지한다(타이머 해제).
    let drainCount = 0;
    let nextHandle = 0;
    const callbacks = new Map();
    const native = {
        createChannel(callback) {
            const handle = ++nextHandle;
            callbacks.set(handle, callback);
            return handle;
        },
        dropChannel(handle) {
            callbacks.delete(handle);
            return true;
        },
        drainEvents() {
            drainCount += 1;
            return 0;
        },
    };
    const first = createChannel(() => { }, native, { pollMs: 5 });
    const second = createChannel(() => { }, native, { pollMs: 5 });
    try {
        first.close(); // 수요 1 남음 — 루프는 계속.
        const drainsAfterFirstClose = drainCount;
        await new Promise((resolve) => setTimeout(resolve, 25));
        assert.ok(drainCount > drainsAfterFirstClose, 'loop must keep running while another channel holds polling demand');
        second.close(); // 마지막 수요 해제 — 루프 정지.
        const drainsAtStop = drainCount;
        await new Promise((resolve) => setTimeout(resolve, 25));
        assert.equal(drainCount, drainsAtStop, 'last close must stop the polling loop');
    }
    finally {
        first.close();
        second.close();
    }
});
test('createChannel pollMs is ignored on natives without drainEvents', () => {
    // drainEvents 미노출 — 옵션은 조용히 무시, 채널 생성 자체는 정상 동작한다.
    const received = [];
    const dropped = [];
    let callback;
    const native = {
        createChannel(next) {
            callback = next;
            return 3;
        },
        dropChannel(handle) {
            dropped.push(handle);
            return true;
        },
    };
    const channel = createChannel((payload) => received.push(payload), native, { pollMs: 5 });
    assert.equal(channel.handle, 3);
    callback(JSON.stringify({ ok: 1 }));
    assert.deepEqual(received, [{ ok: 1 }], 'push delivery must work without a polling loop');
    assert.equal(channel.close(), true);
    assert.deepEqual(dropped, [3]);
});
test('createBytesChannel pollMs drains queued binary frames from a CallInvoker-less native', async () => {
    const frames = [];
    let drainCount = 0;
    const callbacks = new Map();
    const queue = [];
    const native = {
        createChannelBytes(next) {
            callbacks.set(9, next);
            return 9;
        },
        dropChannel(handle) {
            callbacks.delete(handle);
            return true;
        },
        drainEvents() {
            drainCount += 1;
            const queued = queue.splice(0);
            for (const [handle, bytes] of queued)
                callbacks.get(handle)?.(bytes.buffer);
            return queued.length;
        },
    };
    const channel = createBytesChannel((payload) => frames.push(Array.from(payload)), native, {
        pollMs: 5,
    });
    try {
        queue.push([channel.handle, new Uint8Array([0x01, 0x02, 0xff])]);
        await new Promise((resolve) => setTimeout(resolve, 30));
        assert.ok(drainCount >= 2, 'polling loop must run repeatedly');
        assert.deepEqual(frames, [[1, 2, 255]], 'queued binary frame must arrive untouched via drain');
    }
    finally {
        channel.close();
    }
});
test('polling demand is shared between event subscriptions and channels on one native', async () => {
    // 이벤트 구독 해제는 채널 폴링을 끄지 않고, pollMs 없는 구독이 남아 있어도
    // 마지막 채널 close 로 루프가 정지한다(정지 조건 = pollMs 수요 합계 0).
    let drainCount = 0;
    const listeners = new Map();
    const callbacks = new Map();
    const native = {
        onEvent(name, callback) {
            listeners.set(name, callback);
        },
        offEvent(name) {
            listeners.delete(name);
        },
        createChannel(callback) {
            callbacks.set(1, callback);
            return 1;
        },
        dropChannel(handle) {
            callbacks.delete(handle);
            return true;
        },
        drainEvents() {
            drainCount += 1;
            return 0;
        },
    };
    const root = globalThis;
    const previous = root.__rustraNative;
    root.__rustraNative = native;
    const unsubscribe = subscribeEvent('poll.tick', () => { }, { pollMs: 5 });
    const passive = subscribeEvent('push.only', () => { }); // pollMs 없는 구독
    const channel = createChannel(() => { }, native, { pollMs: 5 });
    try {
        unsubscribe(); // 채널 수요 남음 — 루프 유지.
        const drainsAfterUnsubscribe = drainCount;
        await new Promise((resolve) => setTimeout(resolve, 25));
        assert.ok(drainCount > drainsAfterUnsubscribe, 'event unsubscribe must not stop the loop while a channel holds demand');
        channel.close(); // 마지막 수요 해제 — pollMs 없는 구독이 남아도 정지.
        const drainsAtStop = drainCount;
        await new Promise((resolve) => setTimeout(resolve, 25));
        assert.equal(drainCount, drainsAtStop, 'last polling demand release must stop the loop');
    }
    finally {
        unsubscribe();
        passive();
        channel.close();
        root.__rustraNative = previous;
    }
});
//# sourceMappingURL=index.test.js.map