import { saveRecentHostStrict, getRecentHosts } from "./recent-hosts";

/**
 * 호스트 공개키 핀 저장소(프로세스 메모리). QR 스캔·recent hosts 로드 시
 * 채워지고 connect()의 핀 대조에 쓰인다. 값은 QR v2의 `k` 필드(b64url 32B).
 */

const pinned = new Map<string, string>();

function keyOf(host: string, port: number): string {
  return `${host.trim().toLowerCase()}:${port}`;
}

/** 메모리에만 등록한다(앱 시작 시 recent hosts에서 복원할 때). */
export function registerPinnedHostKey(host: string, port: number, hostKey: string): void {
  pinned.set(keyOf(host, port), hostKey);
}

/**
 * 핀을 등록하고 recent hosts 엔트리에도 반영한다(QR 페어링·TOFU 첫 연결 후).
 * 이름은 기존 엔트리의 것을 보존한다.
 */
export async function rememberPinnedHostKey(
  host: string,
  port: number,
  hostKey: string,
  signal?: AbortSignal,
): Promise<void> {
  const storageKey = keyOf(host, port);
  const previousPin = pinned.get(storageKey);
  registerPinnedHostKey(host, port, hostKey);
  try {
    const hosts = await getRecentHosts();
    const previous = hosts.find(
      (item) =>
        item.host.trim().toLowerCase() === host.trim().toLowerCase() && item.port === port,
    );
    await saveRecentHostStrict(host, port, previous?.name, hostKey, signal);
  } catch (error) {
    if (previousPin === undefined) pinned.delete(storageKey);
    else pinned.set(storageKey, previousPin);
    throw error;
  }
}

export function getPinnedHostKey(host: string, port: number): string | null {
  return pinned.get(keyOf(host, port)) ?? null;
}

/** Restore the in-memory endpoint alias during a failed credential transaction. */
export function restorePinnedHostKey(
  host: string,
  port: number,
  hostKey: string | null,
): void {
  if (hostKey === null) pinned.delete(keyOf(host, port));
  else pinned.set(keyOf(host, port), hostKey);
}

export function resetPinnedHostKeysForTests(): void {
  pinned.clear();
}
