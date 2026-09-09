/**
 * 파일 전송의 네이티브 파일 API 구현(expo-document-picker + expo-file-system).
 * 이 파일은 네이티브 빌드에서만 로드된다(metro의 .native.ts 해석) — SSR
 * vitest는 file-io.ts의 폴백을 본다.
 */
import * as DocumentPicker from "expo-document-picker";
// SDK 54+ expo-file-system은 새 File/Paths API가 기본이고 문자열 base64 IO는
// legacy 하위 경로에 있다. v1은 base64 문자열 경로가 단순하므로 legacy를 쓴다.
import * as FileSystem from "expo-file-system/legacy";
import type { FileIo, PickedFile } from "./file-io";

const fileIo: FileIo = {
  async pickSendFile(): Promise<PickedFile | null> {
    const result = await DocumentPicker.getDocumentAsync({
      multiple: false,
      copyToCacheDirectory: true,
    });
    if (result.canceled || !result.assets?.length) return null;
    const asset = result.assets[0];
    // v1은 메모리 버퍼 경로(20 MiB 상한)라 base64 한 번 읽기로 충분하다.
    const base64 = await FileSystem.readAsStringAsync(asset.uri, {
      encoding: FileSystem.EncodingType.Base64,
    });
    return {
      name: sanitizeFileName(asset.name ?? "file"),
      base64,
      size: asset.size ?? Math.floor((base64.length * 3) / 4),
    };
  },

  async saveReceivedFile(name: string, base64: string): Promise<string> {
    const target = `${FileSystem.documentDirectory ?? ""}${sanitizeFileName(name)}`;
    await FileSystem.writeAsStringAsync(target, base64, {
      encoding: FileSystem.EncodingType.Base64,
    });
    return target;
  },
};

function sanitizeFileName(name: string): string {
  const base = name.split(/[/\\]/).pop() ?? "";
  const cleaned = base.replace(/[\0\p{C}]/gu, "").trim();
  return cleaned.startsWith(".") ? cleaned.replace(/^\.+/, "").trim() || "file" : cleaned || "file";
}

globalThis.__leftcarFileIo = fileIo;
