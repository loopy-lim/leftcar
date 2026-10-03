import { sha256 } from "@noble/hashes/sha256";
import { bytesToHex } from "@noble/hashes/utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DocumentPickerResult } from "expo-document-picker";
import {
  FILE_CHUNK_SIZE,
  receiveFile,
  type FileTransferClient,
} from "./file-transfer";
import * as nativeFileIo from "./file-io.native";

interface TestDevice {
  selection: DocumentPickerResult;
  files: Map<string, Uint8Array>;
  readSizes: number[];
  writeSizes: number[];
  pickerOptions: { multiple: boolean; copyToCacheDirectory: boolean } | null;
}

const device = vi.hoisted<TestDevice>(() => ({
  selection: { canceled: true, assets: null },
  files: new Map<string, Uint8Array>(),
  readSizes: [],
  writeSizes: [],
  pickerOptions: null,
}));

vi.mock("expo-document-picker", () => ({
  getDocumentAsync: async (options: { multiple: boolean; copyToCacheDirectory: boolean }) => {
    device.pickerOptions = options;
    return device.selection;
  },
}));

vi.mock("expo-file-system/legacy", () => ({
  documentDirectory: "file:///documents/",
  EncodingType: { Base64: "base64" },
  getInfoAsync: async (uri: string) => ({ exists: device.files.has(uri) }),
  readAsStringAsync: async (uri: string, options: {
    encoding: string;
    position: number;
    length: number;
  }) => {
    if (options.encoding !== "base64") throw new Error("binary range must use base64");
    const source = device.files.get(uri);
    if (!source) throw new Error("file does not exist");
    device.readSizes.push(options.length);
    return encodeBase64(source.subarray(options.position, options.position + options.length));
  },
  writeAsStringAsync: async (uri: string, data: string, options: {
    encoding: string;
    append?: boolean;
  }) => {
    if (options.encoding !== "base64") throw new Error("binary write must use base64");
    const bytes = decodeBase64(data);
    const previous = options.append ? device.files.get(uri) ?? new Uint8Array() : new Uint8Array();
    const next = new Uint8Array(previous.length + bytes.length);
    next.set(previous);
    next.set(bytes, previous.length);
    device.files.set(uri, next);
    device.writeSizes.push(bytes.length);
  },
  moveAsync: async ({ from, to }: { from: string; to: string }) => {
    const bytes = device.files.get(from);
    if (!bytes) throw new Error("staging file does not exist");
    // Expo's file:// move uses renameTo, which can replace an existing file.
    device.files.set(to, bytes);
    device.files.delete(from);
  },
  deleteAsync: async (uri: string) => { device.files.delete(uri); },
}));

beforeEach(() => {
  device.selection = { canceled: true, assets: null };
  device.files.clear();
  device.readSizes.length = 0;
  device.writeSizes.length = 0;
  device.pickerOptions = null;
});

function publicFixture(): Uint8Array {
  return Uint8Array.from({ length: 2 * 1024 * 1024 + 174 }, (_, index) => index % 251);
}

function encodeBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

function decodeBase64(encoded: string): Uint8Array {
  const binary = atob(encoded);
  return Uint8Array.from(binary, character => character.charCodeAt(0));
}

function downloadClient(source: Uint8Array, failSecondChunk = false): FileTransferClient {
  return {
    async request<T>(command: string, args?: unknown): Promise<T> {
      if (command === "fetchFileBegin") return { fileToken: "public-fixture", name: "public.bin", size: source.length } as T;
      if (command === "fetchFileEnd" || command === "fetchFileCancel") return {} as T;
      if (command !== "fetchFileChunk") throw new Error(`unexpected command: ${command}`);
      const request = args as { offset: number; length: number };
      if (failSecondChunk && request.offset > 0) throw new Error("public fixture connection failed");
      return {
        data: encodeBase64(source.subarray(request.offset, request.offset + request.length)),
        size: source.length,
      } as T;
    },
  };
}

describe("Android file IO entrypoint", () => {
  it("returns null when the native document picker is canceled", async () => {
    const selected = await nativeFileIo.getFileIo().pickSendFile();
    expect(selected).toBeNull();
    expect(device.pickerOptions).toEqual({ multiple: false, copyToCacheDirectory: true });
  });

  it("returns the selected public document with bounded range reads", async () => {
    const source = publicFixture();
    device.files.set("file:///cache/public.bin", source);
    device.selection = {
      canceled: false,
      assets: [{ uri: "file:///cache/public.bin", name: "public.bin", size: source.length, mimeType: "application/octet-stream", lastModified: 0 }],
    };
    const selected = await nativeFileIo.getFileIo().pickSendFile();
    if (!selected) throw new Error("document selection was canceled");
    expect(selected.name).toBe("public.bin");
    expect(selected.size).toBe(2_097_326);
    const data = await selected.readBase64(786_432, 174);
    expect(decodeBase64(data)).toEqual(source.subarray(786_432, 786_606));
    expect(device.readSizes).toEqual([174]);
  });

  it("appends all three download chunks before finalizing without overwriting an existing file", async () => {
    const source = publicFixture();
    device.files.set("file:///documents/public.bin", new Uint8Array([9, 8, 7]));
    const result = await receiveFile(downloadClient(source), { queueId: "public-only" }, {
      createSink: name => nativeFileIo.getFileIo().createReceivedSink(name),
    });
    expect(result.path).toBe("file:///documents/public (2).bin");
    const saved = device.files.get(result.path);
    if (!saved) throw new Error("completed download was not saved");
    expect(saved.length).toBe(2_097_326);
    // Independently generated for byte[index] = index % 251, length 2 MiB + 174.
    expect(bytesToHex(sha256(saved))).toBe("baa89489d29db6b1abe78f9cbdccf790afc876e56f45da923b0ff66a1d60fcd3");
    expect(device.files.get("file:///documents/public.bin")).toEqual(new Uint8Array([9, 8, 7]));
    expect(device.files.has(`${result.path}.part`)).toBe(false);
    expect(device.writeSizes).toEqual([0, 786_432, 786_432, 524_462]);
    expect(Math.max(...device.writeSizes)).toBeLessThanOrEqual(FILE_CHUNK_SIZE);
  });

  it("discards a partial native download when a later chunk fails", async () => {
    await expect(receiveFile(downloadClient(publicFixture(), true), { queueId: "public-only" }, {
      createSink: name => nativeFileIo.getFileIo().createReceivedSink(name),
    })).rejects.toThrow("public fixture connection failed");
    expect(device.files.has("file:///documents/public.bin")).toBe(false);
    expect(device.files.has("file:///documents/public.bin.part")).toBe(false);
    expect(device.writeSizes).toEqual([0, 786_432]);
  });

  it("preserves every existing file when all 101 destination names are occupied", async () => {
    const originalFiles = new Map<string, Uint8Array>();
    for (let index = 1; index <= 101; index += 1) {
      const name = index === 1 ? "public.bin" : `public (${index}).bin`;
      const path = `file:///documents/${name}`;
      originalFiles.set(path, new Uint8Array([index, 42, 17]));
    }
    for (const [path, bytes] of originalFiles) device.files.set(path, bytes.slice());
    await expect(receiveFile(downloadClient(new Uint8Array([1, 2, 3])), { queueId: "public-only" }, {
      createSink: name => nativeFileIo.getFileIo().createReceivedSink(name),
    })).rejects.toThrow("file name candidates exhausted");
    expect(device.files).toEqual(originalFiles);
    expect(device.writeSizes).toEqual([]);
  });
});
