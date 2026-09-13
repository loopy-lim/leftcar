/**
 * 낙관적 토글의 응답 반영 판정. React 렌더러 없이는 훅의 비동기 흐름을
 * 검증하기 어렵기 때문에 판정 로직만 순수 함수로 분리했다(Privacy.tsx의
 * 토글 훅이 쓴다):
 * - 더 새 토글이 발행된(superseded) 요청의 응답·실패는 현재 값을 덮지 않는다.
 * - 토글이 한 번이라도 발행됐으면 초기 로드 응답은 무시된다.
 */
export function createToggleGate() {
  let latest = 0;
  let toggledSinceLoad = false;
  return {
    /** 토글 요청을 발행하고 순번을 돌려준다. */
    issue(): number {
      toggledSinceLoad = true;
      return ++latest;
    },
    /** 이 순번의 요청이 아직 최신인지 — 아니면 응답·실패를 무시한다. */
    isCurrent(seq: number): boolean {
      return seq === latest;
    },
    /** 초기 로드 응답을 반영해도 되는지. */
    allowsInitialLoad(): boolean {
      return !toggledSinceLoad;
    },
  };
}

export type ToggleGate = ReturnType<typeof createToggleGate>;
