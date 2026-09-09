import { describe, expect, it } from "vitest";
import {
  FILE_CHUNK_SIZE,
  MAX_FILE_SIZE,
  base64ToBytes,
  bytesToBase64,
  chunkBytes,
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
  private queue: ShareQueueEntry[] = [
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
    this.fail(`unexpected command: ${command}`);
  }

  /** fetchFileChunk가 제공할 호스트 측 원본 바이트. */
  source = new Uint8Array(0);
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

describe("chunkBytes and base64", () => {
  it("splits into 768 KiB chunks with a short tail", () => {
    const bytes = new Uint8Array(FILE_CHUNK_SIZE * 2 + 10);
    const chunks = chunkBytes(bytes);
    expect(chunks).toHaveLength(3);
    expect(chunks[0].length).toBe(FILE_CHUNK_SIZE);
    expect(chunks[1].length).toBe(FILE_CHUNK_SIZE);
    expect(chunks[2].length).toBe(10);
  });

  it("returns no chunks for an empty file", () => {
    expect(chunkBytes(new Uint8Array(0))).toHaveLength(0);
  });

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
    const result = await sendFile(host.client, { name: "data.bin", bytes: fileBytes }, (p) =>
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

  it("rejects files larger than 20 MiB before any command", async () => {
    const host = new FakeHost();
    const bytes = new Uint8Array(MAX_FILE_SIZE + 1);
    await expect(
      sendFile(host.client, { name: "big.bin", bytes }),
    ).rejects.toMatchObject({ key: "fileTooLarge" });
    expect(host.requests).toHaveLength(0);
  });

  it("rejects an empty source name", async () => {
    const host = new FakeHost();
    await expect(
      sendFile(host.client, { name: "   ", bytes: new Uint8Array(1) }),
    ).rejects.toMatchObject({ key: "fileNameError" });
    expect(host.requests).toHaveLength(0);
  });

  it("propagates host gate errors", async () => {
    const host = new FakeHost();
    host.gateEnabled = false;
    await expect(
      sendFile(host.client, { name: "a.txt", bytes: new Uint8Array(1) }),
    ).rejects.toThrow("file share disabled");
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
    const received = await receiveFile(host.client, { queueId: "queue-1" }, (p) =>
      progress.push(p.percent),
    );
    expect(received.name).toBe("notes.txt");
    expect(Array.from(received.bytes)).toEqual([1, 2, 3, 4, 5]);
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
