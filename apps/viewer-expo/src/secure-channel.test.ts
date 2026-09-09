import { afterEach, describe, expect, it } from "vitest";
import {
  StreamSealer,
  bytesToBase64Url,
  clientFinish,
  createClientHello,
  deriveKeys,
  hexToBytes,
  parseSealedLine,
  parseServerHello,
  sealedLine,
  setRandomSource,
  toHex,
} from "./secure-channel";

// 상호 운용 고정 벡터: Rust `cargo test -p secure-channel -- print_vector
// --ignored --nocapture` 출력과 정확히 일치해야 한다. 한 바이트라도 어긋나면
// 뷰어와 호스트가 서로를 못 알아듣는다.
const VEC = {
  spk: "03a107bff3ce10be1d70dd18e74bc09967e4d6309ba50d5f1ddc8664125531b8",
  sig: "b8d0ff5fadcb9f47824759df315d852be7ee5483c7f16dc9d3853604bb6134ffc545adf1fd57b517bc084589c0e62df4c8637dad9ee084b374bae31f6d365e0f",
  clientXk: "358072d6365880d1aeea329adf9121383851ed21a28e3b75e965d0d2cd166254",
  serverXk: "79a631eede1bf9c98f12032cdeadd0e7a079398fc786b88cc846ec89af85a51a",
  kC2s: "dadcc9dd549eace6797cf9f0e9a6716ed8b9d789dcc5c4cbfd058c0634aaa69c",
  kS2c: "b5fd58db06e436bb0b3070e44b52d8a733e97e615b251763216871aab95ceb41",
  c2sFrame1:
    "00000000000000018b799614f1525fccb77713ef3b7deb426d95ee162f9ea66bd70fbab35b6550577ee7fbbe2a6f786d2a8269ec94b872ed699c93dea0ca0d66",
  s2cFrame1:
    "00000000000000011a4e46222f43ad16b566b2b078c137a28264d7818f8452227040f6f1c350ab87ee63f09d49ccb8f8c6755b3d3c190d04b997",
};

/** 벡터의 결정적 입력(Rust 테스트와 동일한 바이트 시퀀스). */
function fixed(start: number): Uint8Array {
  const bytes = new Uint8Array(32);
  for (let i = 0; i < 32; i += 1) bytes[i] = start + i;
  return bytes;
}

function helloFromVector() {
  // 벡터의 x25519 비밀은 32..64(클라이언트), 64..96(서버) 바이트 시퀀스다.
  const clientSecret = fixed(32);
  const nc = fixed(160);
  const ns = fixed(192);
  const hello = {
    nc,
    ns,
    xk: hexToBytes(VEC.serverXk),
    spk: hexToBytes(VEC.spk),
    sig: hexToBytes(VEC.sig),
  };
  return { clientSecret, nc, ns, hello };
}

afterEach(() => {
  setRandomSource(null);
});

describe("secure-channel cross-language vectors", () => {
  it("derives the exact session keys the Rust implementation derives", () => {
    const { clientSecret, nc, ns, hello } = helloFromVector();
    const { keys } = clientFinish(clientSecret, { nc, xk: hexToBytes(VEC.clientXk) }, hello, null);
    expect(toHex(keys.c2s)).toBe(VEC.kC2s);
    expect(toHex(keys.s2c)).toBe(VEC.kS2c);
  });

  it("seals frame #1 byte-for-byte like the Rust implementation", () => {
    const { keys } = (() => {
      const { clientSecret, nc, ns, hello } = helloFromVector();
      return clientFinish(clientSecret, { nc, xk: hexToBytes(VEC.clientXk) }, hello, null);
    })();
    const sealer = new StreamSealer(keys.c2s);
    const frame = sealer.seal(
      new TextEncoder().encode('{"command":"pair","args":{},"token":"a"}'),
    );
    expect(toHex(frame)).toBe(VEC.c2sFrame1);

    const echo = new StreamSealer(keys.s2c);
    const frame2 = echo.seal(new TextEncoder().encode('{"ok":true,"result":{"token":"b"}}'));
    expect(toHex(frame2)).toBe(VEC.s2cFrame1);
  });
});

describe("secure-channel client handshake", () => {
  it("accepts a pinned matching key and rejects a mismatching pin", () => {
    const { clientSecret, nc, hello } = helloFromVector();
    const pinned = hexToBytes(VEC.spk);
    const ok = clientFinish(clientSecret, { nc, xk: hexToBytes(VEC.clientXk) }, hello, pinned);
    expect(toHex(ok.spk)).toBe(VEC.spk);
    expect(() =>
      clientFinish(
        clientSecret,
        { nc, xk: hexToBytes(VEC.clientXk) },
        hello,
        new Uint8Array(32).fill(9),
      ),
    ).toThrow("pinned key");
  });

  it("rejects a tampered signature", () => {
    const { clientSecret, nc, hello } = helloFromVector();
    const tampered = { ...hello, sig: hexToBytes(VEC.sig) };
    tampered.sig[0] ^= 1;
    expect(() =>
      clientFinish(clientSecret, { nc, xk: hexToBytes(VEC.clientXk) }, tampered, null),
    ).toThrow("signature");
  });

  it("round-trips createClientHello → parseServerHello with a scripted server", () => {
    // 스크립트 서버: 같은 라이브러리로 ServerHello를 만들어 준다(자기 일치).
    setRandomSource((n) => new Uint8Array(n).fill(7));
    const { hello: clientHello, secret } = createClientHello();
    const line = JSON.stringify({
      v: 1,
      hello: "s",
      ns: bytesToBase64Url(new Uint8Array(32).fill(3)),
      xk: bytesToBase64Url(new Uint8Array(32).fill(4)),
      spk: bytesToBase64Url(new Uint8Array(32).fill(5)),
      sig: bytesToBase64Url(new Uint8Array(64).fill(6)),
    });
    const parsed = parseServerHello(line);
    expect(parsed.ns.length).toBe(32);
    // 서명은 가짜이므로 실패해야 한다 — 구조 파싱만 확인.
    expect(() => clientFinish(secret, clientHello, parsed, null)).toThrow("signature");
  });
});

describe("StreamSealer", () => {
  it("round-trips and enforces strict counters", () => {
    const key = new Uint8Array(32).fill(1);
    const tx = new StreamSealer(key);
    const rx = new StreamSealer(key);
    const f1 = tx.seal(new TextEncoder().encode("one"));
    const f2 = tx.seal(new TextEncoder().encode("two"));
    expect(new TextDecoder().decode(rx.open(f1))).toBe("one");
    expect(new TextDecoder().decode(rx.open(f2))).toBe("two");
    expect(() => rx.open(f1)).toThrow("replayed");
    // 다른 키는 인증 실패
    expect(() => new StreamSealer(new Uint8Array(32).fill(2)).open(f1)).toThrow();
    // 잘림
    expect(() => rx.open(f1.subarray(0, 8))).toThrow("short");
  });

  it("sealed line framing round-trips", () => {
    const frame = new Uint8Array([1, 2, 3, 4, 5]);
    const parsed = parseSealedLine(sealedLine(frame));
    expect(parsed).not.toBeNull();
    expect(Array.from(parsed as Uint8Array)).toEqual([1, 2, 3, 4, 5]);
    expect(parseSealedLine('{"command":"x"}')).toBeNull();
    expect(parseSealedLine("not json")).toBeNull();
  });
});

describe("deriveKeys salt order", () => {
  it("is salted nc‖ns (informational — pinned by the vector test)", () => {
    const keys = deriveKeys(new Uint8Array(32).fill(1), new Uint8Array(32).fill(2), new Uint8Array(32).fill(3));
    expect(keys.c2s.length).toBe(32);
    expect(keys.s2c.length).toBe(32);
  });
});
