/**
 * 파일 전송의 네이티브 파일 API 구현(expo-document-picker + expo-file-system).
 * 이 파일은 네이티브 빌드에서만 로드된다(metro의 .native.ts 해석) — SSR
 * vitest는 file-io.ts의 폴백을 본다.
 *
 * v2 스트리밍: 업로드는 readAsStringAsync의 position/length 범위 읽기,
 * 다운로드는 writeAsStringAsync(append)로 청크를 디스크에 붙인다. 어떤
 * 경로도 파일 전체를 메모리에 올리지 않는다.
 */
import * as DocumentPicker from "expo-document-picker";
// SDK 54+ expo-file-system은 새 File/Paths API가 기본이고 문자열 base64 IO는
// legacy 하위 경로에 있다. v2도 base64 문자열 경로를 쓴다(청크 단위).
import * as FileSystem from "expo-file-system/legacy";
import type { FileIo, PickedFile, ReceivedFileSink } from "./file-io";
import { MAX_FILE_SIZE } from "./file-transfer";

/** `name (2).ext` 형태의 중복 회피 이름. */
function dedupedName(name: string, index: number): string {
  const dot = name.lastIndexOf(".");
  const suffix = ` (${index})`;
  return dot > 0 ? `${name.slice(0, dot)}${suffix}${name.slice(dot)}` : `${name}${suffix}`;
}

/**
 * 같은 이름의 기존 파일을 덮어쓰지 않게 첫 빈 경로를 찾는다. 호스트가
 * `name (2).ext`로 중복을 피하는 것과 같은 규칙을 기기 쪽에도 적용한다.
 */
async function firstFreeTarget(name: string): Promise<string> {
  const directory = FileSystem.documentDirectory ?? "";
  for (let index = 1; index <= 100; index += 1) {
    const candidate = index === 1 ? name : dedupedName(name, index);
    const target = `${directory}${candidate}`;
    const info = await FileSystem.getInfoAsync(target);
    if (!info.exists) return target;
  }
  return `${directory}${dedupedName(name, 101)}`;
}

const fileIo: FileIo = {
  async pickSendFile(): Promise<PickedFile | null> {
    const result = await DocumentPicker.getDocumentAsync({
      multiple: false,
      copyToCacheDirectory: true,
    });
    if (result.canceled || !result.assets?.length) return null;
    const asset = result.assets[0];
    // 상한 초과 파일은 첫 읽기 전에 거부한다.
    if (typeof asset.size === "number" && asset.size > MAX_FILE_SIZE) {
      throw new Error("file is too large");
    }
    const uri = asset.uri;
    return {
      name: sanitizeFileName(asset.name ?? "file"),
      size: asset.size ?? 0,
      readBase64: (position: number, length: number) =>
        FileSystem.readAsStringAsync(uri, {
          encoding: FileSystem.EncodingType.Base64,
          position,
          length,
        }),
    };
  },

  async createReceivedSink(name: string): Promise<ReceivedFileSink> {
    const target = await firstFreeTarget(sanitizeFileName(name));
    // 호스트(file_transfer.rs)와 같은 .part 스테이징 — 최종 이름은 여기서
    // 선점하되 실제 쓰기는 `.part`로 한다. 프로세스가 중간에 죽으면 잘린
    // 파일이 완성 파일로 보이지 않고, 다음 수신이 남은 `.part`를 빈 상태로
    // 덮어 써 재사용한다(append 없는 빈 쓰기는 잘라 내므로).
    const staging = `${target}.part`;
    await FileSystem.writeAsStringAsync(staging, "", {
      encoding: FileSystem.EncodingType.Base64,
    });
    return {
      path: target,
      appendBase64: async (chunk: string) => {
        await FileSystem.writeAsStringAsync(staging, chunk, {
          encoding: FileSystem.EncodingType.Base64,
          append: true,
        });
      },
      finalize: async () => {
        await FileSystem.moveAsync({ from: staging, to: target });
      },
      discard: async () => {
        await FileSystem.deleteAsync(staging, { idempotent: true });
      },
    };
  },
};

function sanitizeFileName(name: string): string {
  const base = name.split(/[/\\]/).pop() ?? "";
  const cleaned = base.replace(/[\0\p{C}]/gu, "").trim();
  return cleaned.startsWith(".") ? cleaned.replace(/^\.+/, "").trim() || "file" : cleaned || "file";
}

globalThis.__leftcarFileIo = fileIo;
