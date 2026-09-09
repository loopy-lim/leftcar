/**
 * 파일 전송 v1 — 암호화된 제어 채널 위의 청크 전송 순수 모듈.
 *
 * react-native 의존이 없다(SSR vitest 대상). 파일 시스템·문서 선택기 접근은
 * file-io 브리지(`file-io.ts` / `file-io.native.ts`)와 UI 컴포넌트가 담당하고,
 * 이 모듈은 명령 클라이언트(`FileTransferClient`)만 안다.
 *
 * 와이어 규약(호스트 src-tauri/file_transfer.rs와 대응):
 * - 업로드: sendFileBegin{name,size} → sendFileChunk{fileToken,dataBase64,offset}
 *   (오프셋은 순차 강제) → sendFileEnd{fileToken} → {path}
 * - 다운로드: listShareQueue → fetchFileBegin{queueId} → fetchFileChunk
 *   {fileToken,offset,length} → fetchFileEnd{fileToken}
 * - dataBase64는 표준 base64(btoa 출력, url-safe 아님)다.
 */
import { LocalizedError } from "./localized-error";

/** 청크 크기(디코딩 후). 768 KiB → 표준 base64로 정확히 1 MiB. */
export const FILE_CHUNK_SIZE = 768 * 1024;
/** 업로드 상한(호스트 MAX_FILE_SIZE와 동일). */
export const MAX_FILE_SIZE = 20 * 1024 * 1024;

export interface FileTransferClient {
  request<T>(command: string, args?: unknown): Promise<T>;
}

export interface ShareQueueEntry {
  queueId: string;
  name: string;
  size: number;
}

export interface TransferProgress {
  transferredBytes: number;
  totalBytes: number;
  percent: number;
}

export type TransferProgressListener = (progress: TransferProgress) => void;

// -- 표준 base64 -------------------------------------------------------------

/** 표준 base64 인코딩(패딩 포함). 호스트는 STANDARD 엔진으로만 디코딩한다. */
export function bytesToBase64(bytes: Uint8Array): string {
  // fromCharCode를 32K씩 묶어 호출한다 — 바이트 단위 문자열 연결은 큰 파일에서
  // 수 초가 걸린다(Hermes).
  let binary = "";
  const step = 0x8000;
  for (let i = 0; i < bytes.length; i += step) {
    binary += String.fromCharCode(...bytes.subarray(i, i + step));
  }
  return btoa(binary);
}

/** 표준 base64 디코딩(패딩 유무 모두 허용). */
export function base64ToBytes(encoded: string): Uint8Array {
  const normalized = encoded.replace(/\s/g, "");
  const padded = normalized + "=".repeat((4 - (normalized.length % 4)) % 4);
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

// -- 이름 정리와 청크 분할 ----------------------------------------------------

/**
 * 파일명을 호스트가 허용하는 단일 이름으로 만든다. 경로 구분자·`..`·선행
 * 점·제어 문자를 제거하고 255바이트로 잘라 낸다. 빈 결과는 "file".
 */
export function sanitizeFileName(name: string): string {
  const base = name.split(/[/\\]/).pop() ?? "";
  const cleaned = Array.from(base)
    .filter((c) => !/\p{C}/u.test(c))
    .join("")
    .trim();
  if (!cleaned || cleaned.startsWith(".")) {
    // 선행 점만 문제인 경우 점을 떼고, 그래도 비면 기본 이름.
    const stripped = cleaned.replace(/^\.+/, "").trim();
    if (stripped) return truncateToBytes(stripped, 255);
    return "file";
  }
  return truncateToBytes(cleaned, 255);
}

function truncateToBytes(value: string, limit: number): string {
  const encoded = new TextEncoder().encode(value);
  if (encoded.length <= limit) return value;
  let cut = limit;
  while (cut > 0 && (encoded[cut] & 0xc0) === 0x80) cut -= 1;
  return new TextDecoder().decode(encoded.subarray(0, cut));
}

/** 파일 바이트를 전송 청크로 분할한다(마지막 청크는 남는 바이트). */
export function chunkBytes(bytes: Uint8Array, chunkSize = FILE_CHUNK_SIZE): Uint8Array[] {
  const chunks: Uint8Array[] = [];
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    chunks.push(bytes.subarray(offset, Math.min(offset + chunkSize, bytes.length)));
  }
  return chunks;
}

function progressOf(transferredBytes: number, totalBytes: number): TransferProgress {
  return {
    transferredBytes,
    totalBytes,
    percent: totalBytes > 0 ? Math.min(100, Math.round((transferredBytes / totalBytes) * 100)) : 100,
  };
}

// -- 업로드(뷰어 → 호스트) ----------------------------------------------------

export interface SendFileResult {
  name: string;
  /** 호스트가 알려 준 표시용 저장 위치(예: Downloads/leftcar/<기기>/<이름>). */
  path: string;
}

/**
 * 파일 하나를 호스트로 보낸다. 게이트 꺼짐·이름 거부·크기 초과는
 * LocalizedError로, 전송 경로 오류는 그대로 전파한다(호스트 오류 문자열은
 * mapFileTransferError에서 번역 키로 바꿀 수 있다).
 */
export async function sendFile(
  client: FileTransferClient,
  file: { name: string; bytes: Uint8Array },
  onProgress?: TransferProgressListener,
): Promise<SendFileResult> {
  const totalBytes = file.bytes.length;
  if (totalBytes > MAX_FILE_SIZE) {
    throw new LocalizedError("fileTooLarge");
  }
  const name = sanitizeFileName(file.name);
  // 원래 이름의 마지막 경로 구분자 뒤가 비었으면(빈 문자열·"/"만) 거부.
  // sanitizeFileName은 항상 유효한 단일 이름을 돌려주므로("file" 폴백) 이
  // 검사는 원본 이름이 실질적으로 없던 경우만 걸러 낸다.
  const originalBase = file.name.split(/[/\\]/).pop()?.trim() ?? "";
  if (!originalBase) {
    throw new LocalizedError("fileNameError");
  }
  const begin = await client.request<{ fileToken: string }>("sendFileBegin", {
    name,
    size: totalBytes,
  });
  const fileToken = begin.fileToken;
  onProgress?.(progressOf(0, totalBytes));
  let offset = 0;
  while (offset < totalBytes) {
    const end = Math.min(offset + FILE_CHUNK_SIZE, totalBytes);
    const chunk = bytesToBase64(file.bytes.subarray(offset, end));
    const response = await client.request<{ written: number }>("sendFileChunk", {
      fileToken,
      dataBase64: chunk,
      offset,
    });
    // 호스트가 순차 오프셋을 강제하므로 written은 end와 일치해야 한다.
    if (response.written !== end) {
      throw new LocalizedError("fileTransferFailed", {
        detail: `written ${response.written} != ${end}`,
      });
    }
    offset = end;
    onProgress?.(progressOf(offset, totalBytes));
  }
  const end = await client.request<{ path: string }>("sendFileEnd", { fileToken });
  return { name, path: end.path };
}

// -- 다운로드(호스트 → 뷰어) --------------------------------------------------

export async function listShareQueue(client: FileTransferClient): Promise<ShareQueueEntry[]> {
  const result = await client.request<{ queue: ShareQueueEntry[] }>("listShareQueue", {});
  return Array.isArray(result.queue) ? result.queue : [];
}

export interface ReceiveFileResult {
  name: string;
  bytes: Uint8Array;
}

/** 공유 대기열 항목 하나를 내려받아 바이트로 돌려준다(저장은 file-io 브리지). */
export async function receiveFile(
  client: FileTransferClient,
  entry: Pick<ShareQueueEntry, "queueId">,
  onProgress?: TransferProgressListener,
): Promise<ReceiveFileResult> {
  const begin = await client.request<{ fileToken: string; name: string; size: number }>(
    "fetchFileBegin",
    { queueId: entry.queueId },
  );
  const totalBytes = begin.size;
  const bytes = new Uint8Array(totalBytes);
  onProgress?.(progressOf(0, totalBytes));
  let offset = 0;
  while (offset < totalBytes) {
    const length = Math.min(FILE_CHUNK_SIZE, totalBytes - offset);
    const response = await client.request<{ data: string; size: number }>("fetchFileChunk", {
      fileToken: begin.fileToken,
      offset,
      length,
    });
    const chunk = base64ToBytes(response.data);
    if (chunk.length !== length) {
      throw new LocalizedError("fileTransferFailed", {
        detail: `chunk ${chunk.length} != ${length}`,
      });
    }
    bytes.set(chunk, offset);
    offset += chunk.length;
    onProgress?.(progressOf(offset, totalBytes));
  }
  await client.request("fetchFileEnd", { fileToken: begin.fileToken });
  return { name: begin.name, bytes };
}

// -- 오류 매핑 ---------------------------------------------------------------

/**
 * 호스트 제어 오류 문자열을 뷰어 i18n 키(viewer 섹션)로 바꾼다. 매핑이 없으면
 * null — 호출자가 formatErrorMessage의 일반 경로를 쓴다.
 */
export function mapFileTransferError(error: unknown): string | null {
  const raw =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : "";
  const normalized = raw.toLowerCase();
  if (normalized.includes("file share disabled")) return "fileShareDisabled";
  if (normalized.includes("file is too large")) return "fileTooLarge";
  if (normalized.includes("file name")) return "fileNameError";
  if (normalized.includes("shared file is missing")) return "fileShareEmpty";
  if (normalized.includes("file transfer is incomplete")) return "fileTransferFailed";
  return null;
}
