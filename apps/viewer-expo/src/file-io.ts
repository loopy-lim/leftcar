/**
 * 파일 전송의 기기 파일 API 브리지(usb-runtime 패턴).
 *
 * - `file-io.ts`(이 파일): 웹·테스트(SSR)에서 로드되는 기본 구현.
 * - `file-io.native.ts`: 네이티브 빌드에서 metro가 대신 로드하며
 *   expo-document-picker / expo-file-system을 연결한다.
 *
 * UI는 getFileIo()만 호출한다. 이 모듈은 react-native를 import하지 않는다.
 * v2부터 업로드는 범위 읽기(readBase64), 다운로드는 디스크 append 싱크로
 * 스트리밍한다 — 파일 전체가 메모리에 올라오는 경로가 없다.
 */

export interface PickedFile {
  name: string;
  size: number;
  /** `position`부터 `length`바이트를 표준 base64로 읽는다. */
  readBase64(position: number, length: number): Promise<string>;
}

export interface ReceivedFileSink {
  /** 최종 저장 경로(완료 안내에 쓴다). */
  readonly path: string;
  /** base64 청크를 파일 끝에 순서대로 붙인다. */
  appendBase64(chunk: string): Promise<void>;
  /**
   * 전송이 끝나면 스테이징 `.part`를 최종 경로로 이름 바꿔 완성한다.
   * 중간에 죽은 전송은 `.part`로만 남아 완성 파일과 구별된다(호스트의
   * file_transfer.rs와 같은 규약).
   */
  finalize(): Promise<void>;
  /** 전송 실패 시 지금까지 받은 파일을 지운다. */
  discard(): Promise<void>;
}

export interface FileIo {
  /** 문서 선택기로 파일 하나를 고른다. 취소는 null. */
  pickSendFile(): Promise<PickedFile | null>;
  /** 중복 없는 빈 파일을 만들고 append 싱크를 돌려준다. */
  createReceivedSink(name: string): Promise<ReceivedFileSink>;
}

declare global {
  var __leftcarFileIo: FileIo | undefined;
}

/** 네이티브 런타임이 없는 환경의 기본 구현(웹·테스트). */
const fallbackFileIo: FileIo = {
  async pickSendFile() {
    return null;
  },
  async createReceivedSink() {
    throw new Error("saving files requires the native runtime");
  },
};

export function getFileIo(): FileIo {
  return globalThis.__leftcarFileIo ?? fallbackFileIo;
}
