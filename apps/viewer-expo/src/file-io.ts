/**
 * 파일 전송의 기기 파일 API 브리지(usb-runtime 패턴).
 *
 * - `file-io.ts`(이 파일): 웹·테스트(SSR)에서 로드되는 기본 구현.
 * - `file-io.native.ts`: 네이티브 빌드에서 metro가 대신 로드하며
 *   expo-document-picker / expo-file-system을 연결한다.
 *
 * UI는 getFileIo()만 호출한다. 이 모듈은 react-native를 import하지 않는다.
 */

export interface PickedFile {
  name: string;
  /** 표준 base64로 읽은 파일 내용(호스트 dataBase64와 같은 인코딩). */
  base64: string;
  size: number;
}

export interface FileIo {
  /** 문서 선택기로 파일 하나를 고른다. 취소는 null. */
  pickSendFile(): Promise<PickedFile | null>;
  /** 받은 파일(base64)을 앱 Documents에 저장하고 경로를 돌려준다. */
  saveReceivedFile(name: string, base64: string): Promise<string>;
}

declare global {
  var __leftcarFileIo: FileIo | undefined;
}

/** 네이티브 런타임이 없는 환경의 기본 구현(웹·테스트). */
const fallbackFileIo: FileIo = {
  async pickSendFile() {
    return null;
  },
  async saveReceivedFile() {
    throw new Error("saving files requires the native runtime");
  },
};

export function getFileIo(): FileIo {
  return globalThis.__leftcarFileIo ?? fallbackFileIo;
}
