import { describe, expect, it } from "vitest";
import {
  FILE_CHUNK_SIZE,
  MAX_FILE_SIZE,
  base64ToBytes,
  listShareQueue,
  mapFileTransferError,
  receiveFile,
  sanitizeFileName,
  sendFile,
  type FileTransferClient,
  type ShareQueueEntry,
} from "./file-transfer";

/**
 * 호스트(src-tauri/file_transfer.rs)의 제약을 흉내 내는 스크립트 클라이언트.
 * 실제 와이어 없이 명령 순서·오프셋 강제·게이트 오류를 검증한다.
 */

/** 표준 base64 인코더(패딩 포함). v2 전송은 인코딩 없이 base64 범위 읽기를
 *  그대로 실으므로 프로덕션에는 인코더가 없고, 페이크 호스트만 쓴다. */
function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const step = 0x8000;
  for (let i = 0; i < bytes.length; i += step) {
    binary += String.fromCharCode(...bytes.subarray(i, i + step));
  }
  return btoa(binary);
}

interface RecordedRequest {
  command: string;
  args: Record<string, unknown>;
}

class FakeHost {
  readonly requests: RecordedRequest[] = [];
  gateEnabled = true;
  bytes = new Uint8Array(0);
  private written = 0;
  private token = "token-1";
  queue: ShareQueueEntry[] = [
    { queueId: "queue-1", name: "notes.txt", size: 5 },
  ];

  client: FileTransferClient = {
    request: async <T,>(command: string, args?: unknown): Promise<T> => {
      this.requests.push({
        command,
        args: (args ?? {}) as Record<string, unknown>,
      });
      return this.handle(command, (args ?? {}) as Record<string, unknown>) as T;
    },
  };

  failNext(error: string) {
    this.nextError = error;
  }

  private nextError: string | null = null;

  private fail(error?: string): never {
    throw new Error(error ?? this.nextError ?? "unknown");
  }

  private handle(command: string, args: Record<string, unknown>): unknown {
    if (this.nextError) {
      const error = this.nextError;
      this.nextError = null;
      this.fail(error);
    }
    if (command === "sendFileBegin") {
      if (!this.gateEnabled) this.fail("file share disabled");
      const name = String(args.name);
      const size = Number(args.size);
      if (!name || name.includes("/") || name.includes("..") || name.startsWith(".")) {
        this.fail(`invalid file name: ${name}`);
      }
      if (size > 20 * 1024 * 1024) this.fail("file is too large");
      this.written = 0;
      this.bytes = new Uint8Array(size);
      return { fileToken: this.token };
    }
    if (command === "sendFileChunk") {
      if (!this.gateEnabled) this.fail("file share disabled");
      const offset = Number(args.offset);
      const data = base64ToBytes(String(args.dataBase64));
      if (offset !== this.written) this.fail("chunk offset is out of order");
      this.bytes.set(data, offset);
      this.written += data.length;
      return { written: this.written };
    }
    if (command === "sendFileEnd") {
      return {
        path: `Downloads/leftcar/test-viewer/${"out.bin"}`,
      };
    }
    if (command === "listShareQueue") {
      if (!this.gateEnabled) this.fail("file share disabled");
      return { queue: this.queue };
    }
    if (command === "fetchFileBegin") {
      if (!this.gateEnabled) this.fail("file share disabled");
      const entry = this.queue.find((candidate) => candidate.queueId === args.queueId);
      if (!entry) this.fail("unknown queue entry");
      return { fileToken: this.token, name: entry!.name, size: entry!.size };
    }
    if (command === "fetchFileChunk") {
      if (!this.gateEnabled) this.fail("file share disabled");
      const offset = Number(args.offset);
      const length = Number(args.length);
      const slice = this.source.subarray(offset, Math.min(offset + length, this.source.length));
      return { data: bytesToBase64(slice), size: this.source.length };
    }
    if (command === "fetchFileEnd") {
      return {};
    }
    if (command === "sendFileCancel" || command === "fetchFileCancel") {
      return {};
    }
    this.fail(`unexpected command: ${command}`);
  }

  /** fetchFileChunk가 제공할 호스트 측 원본 바이트. */
  source = new Uint8Array(0);
}


/** 테스트용 업로드 원본 — Uint8Array를 readBase64 범위 읽기로 노출한다. */
function sourceFromBytes(name: string, bytes: Uint8Array) {
  return {
    name,
    size: bytes.length,
    readBase64: async (position: number, length: number) =>
      bytesToBase64(bytes.subarray(position, Math.min(position + length, bytes.length))),
  };
}

/** 테스트용 수신 싱크 — append된 base64를 바이트로 모은다. */
class MemorySink {
  readonly path = "/test/received.bin";
  readonly parts: Uint8Array[] = [];
  discarded = false;
  finalized = false;

  async appendBase64(chunk: string): Promise<void> {
    this.parts.push(base64ToBytes(chunk));
  }

  async finalize(): Promise<void> {
    this.finalized = true;
  }

  async discard(): Promise<void> {
    this.discarded = true;
    this.parts.length = 0;
  }

  bytes(): Uint8Array {
    const total = this.parts.reduce((sum, part) => sum + part.length, 0);
    const out = new Uint8Array(total);
    let offset = 0;
    for (const part of this.parts) {
      out.set(part, offset);
      offset += part.length;
    }
    return out;
  }
}

describe("sanitizeFileName", () => {
  it("keeps plain names intact", () => {
    expect(sanitizeFileName("report.pdf")).toBe("report.pdf");
    expect(sanitizeFileName("파일 이름.txt")).toBe("파일 이름.txt");
  });

  it("strips path components", () => {
    expect(sanitizeFileName("../etc/passwd")).toBe("passwd");
    expect(sanitizeFileName("a/b/c.txt")).toBe("c.txt");
    expect(sanitizeFileName("a\\b.txt")).toBe("b.txt");
  });

  it("removes leading dots and control characters", () => {
    expect(sanitizeFileName(".hidden")).toBe("hidden");
    expect(sanitizeFileName("..x")).toBe("x");
    expect(sanitizeFileName("bad\u0000name.txt")).toBe("badname.txt");
    expect(sanitizeFileName("  spaced.txt  ")).toBe("spaced.txt");
  });

  it("falls back to file for empty names", () => {
    expect(sanitizeFileName("")).toBe("file");
    expect(sanitizeFileName("///")).toBe("file");
    expect(sanitizeFileName("...")).toBe("file");
  });

  it("truncates names longer than 255 bytes on a code point boundary", () => {
    const name = `${"가".repeat(200)}.txt`; // 603바이트 + 4
    const sanitized = sanitizeFileName(name);
    expect(new TextEncoder().encode(sanitized).length).toBeLessThanOrEqual(255);
    // 잘린 결과도 여전히 유효한 문자열(문자 코드점 경계)이다.
    expect(new TextDecoder().decode(new TextEncoder().encode(sanitized))).toBe(sanitized);
  });
});

describe("base64", () => {
  it("round-trips bytes through standard base64", () => {
    const bytes = new Uint8Array([104, 101, 108, 108, 111]); // "hello"
    expect(bytesToBase64(bytes)).toBe("aGVsbG8=");
    expect(base64ToBytes("aGVsbG8=")).toEqual(bytes);
    // url-safe 알파벳은 표준 base64가 아니다 — +, /를 쓴다.
    const binary = new Uint8Array([251, 255, 190]);
    const encoded = bytesToBase64(binary);
    expect(encoded).not.toContain("-");
    expect(encoded).not.toContain("_");
    expect(base64ToBytes(encoded)).toEqual(binary);
  });
});

describe("sendFile", () => {
  it("streams sequential chunks with progress and finishes with the host path", async () => {
    const host = new FakeHost();
    const fileBytes = new Uint8Array(FILE_CHUNK_SIZE + 3);
    for (let i = 0; i < fileBytes.length; i += 1) fileBytes[i] = i % 251;

    const progress: number[] = [];
    const result = await sendFile(host.client, sourceFromBytes("data.bin", fileBytes), (p) =>
      progress.push(p.transferredBytes),
    );

    expect(result.path).toBe("Downloads/leftcar/test-viewer/out.bin");
    const commands = host.requests.map((request) => request.command);
    expect(commands[0]).toBe("sendFileBegin");
    expect(commands).toEqual([
      "sendFileBegin",
      "sendFileChunk",
      "sendFileChunk",
      "sendFileEnd",
    ]);
    expect(host.requests[0].args).toEqual({ name: "data.bin", size: fileBytes.length });
    expect(host.requests[1].args.offset).toBe(0);
    expect(host.requests[2].args.offset).toBe(FILE_CHUNK_SIZE);
    // 청크 내용이 그대로 도착한다.
    expect(host.bytes).toEqual(fileBytes);
    // 진행은 0에서 시작해 끝까지 간다.
    expect(progress[0]).toBe(0);
    expect(progress[progress.length - 1]).toBe(fileBytes.length);
  });

  it("rejects files larger than the cap before any command", async () => {
    const host = new FakeHost();
    const big = new Uint8Array(8);
    await expect(
      sendFile(host.client, { name: "big.bin", size: MAX_FILE_SIZE + 1, readBase64: async () => bytesToBase64(big) }),
    ).rejects.toMatchObject({ key: "fileTooLarge" });
    expect(host.requests).toHaveLength(0);
  });

  it("rejects an empty source name", async () => {
    const host = new FakeHost();
    await expect(
      sendFile(host.client, sourceFromBytes("   ", new Uint8Array(1))),
    ).rejects.toMatchObject({ key: "fileNameError" });
    expect(host.requests).toHaveLength(0);
  });

  it("propagates host gate errors", async () => {
    const host = new FakeHost();
    host.gateEnabled = false;
    await expect(
      sendFile(host.client, sourceFromBytes("a.txt", new Uint8Array(1))),
    ).rejects.toThrow("file share disabled");
  });

  it("cancels the upload token when a chunk fails mid-transfer", async () => {
    const host = new FakeHost();
    const source = sourceFromBytes("a.bin", new Uint8Array(FILE_CHUNK_SIZE + 4));
    const client: FileTransferClient = {
      request: async <T,>(command: string, args?: unknown): Promise<T> => {
        const response = await host.client.request<T>(command, args);
        // begin이 성공한 직후 다음 요청(첫 청크)이 실패하게 만든다.
        if (command === "sendFileBegin") host.failNext("socket reset");
        return response;
      },
    };
    await expect(sendFile(client, source)).rejects.toThrow("socket reset");
    const commands = host.requests.map((request) => request.command);
    expect(commands).toContain("sendFileBegin");
    expect(commands).toContain("sendFileCancel");
    expect(commands).not.toContain("sendFileEnd");
  });
});

describe("receiveFile", () => {
  it("pulls the queue, fetches chunks in range, and ends the transfer", async () => {
    const host = new FakeHost();
    host.source = new Uint8Array([1, 2, 3, 4, 5]);
    // FILE_CHUNK_SIZE가 소스보다 크므로 청크 하나로 끝난다.

    const queue = await listShareQueue(host.client);
    expect(queue).toEqual([{ queueId: "queue-1", name: "notes.txt", size: 5 }]);

    const progress: number[] = [];
    const sink = new MemorySink();
    const received = await receiveFile(host.client, { queueId: "queue-1" }, {
      createSink: async () => sink,
      onProgress: (p) => progress.push(p.percent),
    });
    expect(received.name).toBe("notes.txt");
    expect(received.path).toBe(sink.path);
    expect(Array.from(sink.bytes())).toEqual([1, 2, 3, 4, 5]);
    // .part 스테이징 규약: 전송이 온전히 끝난 뒤에만 완성(finalize)한다.
    expect(sink.finalized).toBe(true);
    const commands = host.requests.map((request) => request.command);
    expect(commands).toEqual([
      "listShareQueue",
      "fetchFileBegin",
      "fetchFileChunk",
      "fetchFileEnd",
    ]);
    expect(host.requests[2].args).toMatchObject({ offset: 0, length: 5 });
    expect(progress[progress.length - 1]).toBe(100);
  });

  it("rejects an oversized fetch before allocating and cancels the token", async () => {
    const host = new FakeHost();
    host.queue = [{ queueId: "queue-big", name: "huge.iso", size: MAX_FILE_SIZE + 1 }];
    await expect(
      receiveFile(host.client, { queueId: "queue-big" }, { createSink: async () => new MemorySink() }),
    ).rejects.toThrow();
    const commands = host.requests.map((request) => request.command);
    // 상한 초과 응답에는 청크를 요청하지 않는다(전체 할당 전에 거부).
    expect(commands).not.toContain("fetchFileChunk");
    expect(commands).toContain("fetchFileCancel");
  });

  it("cancels the fetch token when a chunk request fails mid-transfer", async () => {
    const host = new FakeHost();
    host.queue = [
      { queueId: "queue-2", name: "video.bin", size: FILE_CHUNK_SIZE + 10 },
    ];
    host.source = new Uint8Array(FILE_CHUNK_SIZE + 10);
    const client: FileTransferClient = {
      request: async <T,>(command: string, args?: unknown): Promise<T> => {
        const response = await host.client.request<T>(command, args);
        // 첫 청크가 성공한 직후 다음 요청이 실패하게 만든다.
        if (command === "fetchFileChunk") host.failNext("link died");
        return response;
      },
    };
    const sink = new MemorySink();
    await expect(
      receiveFile(client, { queueId: "queue-2" }, { createSink: async () => sink }),
    ).rejects.toThrow("link died");
    expect(sink.discarded, "failed receive must discard the partial file").toBe(true);
    // 실패한 전송은 완성(finalize)되지 않는다 — .part만 치워진다.
    expect(sink.finalized).toBe(false);
    const commands = host.requests.map((request) => request.command);
    expect(commands).toContain("fetchFileCancel");
    expect(commands).not.toContain("fetchFileEnd");
  });

  it("propagates the disabled-gate error from listShareQueue", async () => {
    const host = new FakeHost();
    host.gateEnabled = false;
    await expect(listShareQueue(host.client)).rejects.toThrow("file share disabled");
  });
});

describe("mapFileTransferError", () => {
  it("maps known host errors to viewer i18n keys", () => {
    expect(mapFileTransferError(new Error("file share disabled"))).toBe("fileShareDisabled");
    expect(mapFileTransferError(new Error("file is too large"))).toBe("fileTooLarge");
    expect(mapFileTransferError(new Error("file name must not be empty"))).toBe("fileNameError");
    expect(mapFileTransferError(new Error("shared file is missing"))).toBe("fileShareEmpty");
  });

  it("returns null for unrelated errors", () => {
    expect(mapFileTransferError(new Error("control request timeout"))).toBeNull();
    expect(mapFileTransferError(undefined)).toBeNull();
  });
});
