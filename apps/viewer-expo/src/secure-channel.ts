/**
 * Leftcar 세션 암호화 — crates/secure-channel의 TS 대응 구현.
 *
 * 제어 평면 핸드셰이크(뷰어 측): 임시 X25519로 PFS 세션 키를 합의하고, QR로
 * 핀한 호스트 Ed25519 공개키로 ServerHello 서명을 검증한다. 이후 모든 줄은
 * ChaCha20-Poly1305 봉인 프레임 `{"e":"<b64url(counter‖tag‖ct)>"}`로 오간다.
 *
 * 상호 운용은 고정 벡터로 잠긴다 — 키·프레임 값은 Rust 테스트
 * (cargo test -p secure-channel print_vector)의 출력과 정확히 일치해야 한다.
 */
import { chacha20poly1305 } from "@noble/ciphers/chacha";
import { ed25519, x25519 } from "@noble/curves/ed25519";
import { hkdf } from "@noble/hashes/hkdf";
import { sha256 } from "@noble/hashes/sha256";

export const SIG_CONTEXT = "leftcar-ctrl-v1";
export const HANDSHAKE_INFO = "leftcar/control/v1";

export const COUNTER_LEN = 8;
export const TAG_LEN = 16;
export const MAX_PLAINTEXT = 16 * 1024 * 1024;

// -- 랜덤 소스 ---------------------------------------------------------------

type RandomSource = (length: number) => Uint8Array;

let randomSource: RandomSource | null = null;

/** 테스트·임베딩에서 결정적 랜덤을 주입할 때 쓴다(운영 경로 아님). */
export function setRandomSource(source: RandomSource | null): void {
  randomSource = source;
}

function defaultRandomSource(length: number): Uint8Array {
  // 1) 표준 WebCrypto(노드/테스트)
  const globalCrypto = (globalThis as { crypto?: { getRandomValues(b: Uint8Array): Uint8Array } })
    .crypto;
  if (globalCrypto?.getRandomValues) {
    return globalCrypto.getRandomValues(new Uint8Array(length));
  }
  // 2) 기기: expo-crypto의 getRandomValues는 동기 호출이다(Hermes).
  //    네이티브 브릿지 모듈은 비동기라 핸드셰이크의 동기 경로에 못 쓴다.
  try {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const ExpoCrypto = require("expo-crypto");
    if (typeof ExpoCrypto.getRandomValues === "function") {
      return ExpoCrypto.getRandomValues(new Uint8Array(length)) as Uint8Array;
    }
  } catch {
    // expo-crypto 미탑재 환경 — 아래 오류가 정확한 원인을 알린다.
  }
  throw new Error("no CSPRNG available for secure channel");
}

export function randomBytes(length: number): Uint8Array {
  const source = randomSource ?? defaultRandomSource;
  return source(length);
}

// -- base64url ----------------------------------------------------------------

export function bytesToBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 1) binary += String.fromCharCode(bytes[i]);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function base64UrlToBytes(encoded: string): Uint8Array {
  const normalized = encoded.replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalized + "=".repeat((4 - (normalized.length % 4)) % 4);
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

export function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i += 1) {
    bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

// -- 핸드셰이크 ---------------------------------------------------------------

export interface ClientHello {
  nc: Uint8Array;
  xk: Uint8Array;
}

export interface ServerHello {
  ns: Uint8Array;
  xk: Uint8Array;
  spk: Uint8Array;
  sig: Uint8Array;
}

export interface SessionKeys {
  c2s: Uint8Array;
  s2c: Uint8Array;
}

export interface HandshakeResult {
  keys: SessionKeys;
  /** 검증에 성공한 호스트 공개키 — TOFU 핀의 원천. */
  spk: Uint8Array;
}

export function createClientHello(): { hello: ClientHello; secret: Uint8Array } {
  const secret = randomBytes(32);
  return {
    secret,
    hello: { nc: randomBytes(32), xk: x25519.getPublicKey(secret) },
  };
}

/** ClientHello JSON 줄. 평문으로 전송된다(공개 값만 담는다). */
export function encodeClientHello(hello: ClientHello): string {
  return JSON.stringify({
    v: 1,
    hello: "c",
    nc: bytesToBase64Url(hello.nc),
    xk: bytesToBase64Url(hello.xk),
  });
}

export function parseServerHello(line: string): ServerHello {
  const parsed = JSON.parse(line) as {
    hello?: unknown;
    ns?: unknown;
    xk?: unknown;
    spk?: unknown;
    sig?: unknown;
  };
  if (parsed.hello !== "s") throw new Error("not a ServerHello");
  const ns = base64UrlToBytes(parsed.ns as string);
  const xk = base64UrlToBytes(parsed.xk as string);
  const spk = base64UrlToBytes(parsed.spk as string);
  const sig = base64UrlToBytes(parsed.sig as string);
  if (ns.length !== 32 || xk.length !== 32 || spk.length !== 32 || sig.length !== 64) {
    throw new Error("malformed ServerHello");
  }
  return { ns, xk, spk, sig };
}

/** 서명이 덮는 전사. Rust signed_transcript와 바이트 단위로 일치해야 한다. */
export function signedTranscript(
  nc: Uint8Array,
  ns: Uint8Array,
  xkC: Uint8Array,
  xkS: Uint8Array,
  spk: Uint8Array,
): Uint8Array {
  const context = new TextEncoder().encode(SIG_CONTEXT);
  const out = new Uint8Array(context.length + 32 * 5);
  let offset = 0;
  for (const part of [context, nc, ns, xkC, xkS, spk]) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

/**
 * ServerHello 검증 + 세션키 도출. `pinnedSpk`가 있으면 반드시 일치해야 하고
 * (QR 핀), 없으면 TOFU로 검증된 키를 그대로 돌려준다.
 */
export function clientFinish(
  clientSecret: Uint8Array,
  client: ClientHello,
  hello: ServerHello,
  pinnedSpk: Uint8Array | null,
): HandshakeResult {
  if (pinnedSpk) {
    if (!bytesEqual(pinnedSpk, hello.spk)) {
      throw new Error("host key does not match pinned key");
    }
  }
  const message = signedTranscript(client.nc, hello.ns, client.xk, hello.xk, hello.spk);
  const ok = ed25519.verify(hello.sig, message, hello.spk);
  if (!ok) throw new Error("host signature invalid");
  const shared = x25519.getSharedSecret(clientSecret, hello.xk);
  return { keys: deriveKeys(shared, client.nc, hello.ns), spk: hello.spk };
}

export function deriveKeys(shared: Uint8Array, nc: Uint8Array, ns: Uint8Array): SessionKeys {
  const salt = new Uint8Array(64);
  salt.set(nc, 0);
  salt.set(ns, 32);
  const info = new TextEncoder().encode(HANDSHAKE_INFO);
  const okm = hkdf(sha256, shared, salt, info, 64);
  return { c2s: okm.slice(0, 32), s2c: okm.slice(32, 64) };
}

function bytesEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i += 1) diff |= a[i] ^ b[i];
  return diff === 0;
}

// -- 봉인 프레임 --------------------------------------------------------------

/** TCP(제어)용 봉인기 — 엄격 단조 카운터. 방향마다 하나씩 만든다. */
export class StreamSealer {
  private readonly cipherKey: Uint8Array;
  private next = 1n;
  private lastSeen = 0n;

  constructor(key: Uint8Array) {
    if (key.length !== 32) throw new Error("sealer key must be 32 bytes");
    this.cipherKey = key;
  }

  seal(plaintext: Uint8Array): Uint8Array {
    if (plaintext.length > MAX_PLAINTEXT) throw new Error("plaintext too large");
    const counter = this.next;
    this.next += 1n;
    const nonce = nonceFor(counter);
    const sealed = chacha20poly1305(this.cipherKey, nonce).encrypt(plaintext);
    const frame = new Uint8Array(COUNTER_LEN + sealed.length);
    new DataView(frame.buffer).setBigUint64(0, counter, false);
    frame.set(sealed, COUNTER_LEN);
    return frame;
  }

  open(frame: Uint8Array): Uint8Array {
    if (frame.length < COUNTER_LEN + TAG_LEN) throw new Error("frame too short");
    const counter = new DataView(
      frame.buffer,
      frame.byteOffset,
      frame.byteLength,
    ).getBigUint64(0, false);
    if (counter === 0n || counter <= this.lastSeen) throw new Error("counter replayed or stale");
    const plaintext = chacha20poly1305(this.cipherKey, nonceFor(counter)).decrypt(
      frame.subarray(COUNTER_LEN),
    );
    this.lastSeen = counter;
    return plaintext;
  }
}

function nonceFor(counter: bigint): Uint8Array {
  const nonce = new Uint8Array(12);
  new DataView(nonce.buffer).setBigUint64(4, counter, false);
  return nonce;
}

/** 봉인 프레임을 전송 줄(`{"e":"<b64url>"}`)로 감싼다. */
export function sealedLine(frame: Uint8Array): string {
  return JSON.stringify({ e: bytesToBase64Url(frame) });
}

/** 전송 줄에서 봉인 프레임을 꺼낸다. 봉인 줄이 아니면 null. */
export function parseSealedLine(line: string): Uint8Array | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(line);
  } catch {
    return null;
  }
  const encoded = (parsed as { e?: unknown }).e;
  if (typeof encoded !== "string") return null;
  return base64UrlToBytes(encoded);
}
