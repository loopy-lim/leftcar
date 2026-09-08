import { interpolate, type TranslationSchema } from "@leftcar/ui-tokens";
import { currentTranslation } from "./language-store";

export type ViewerTextKey = keyof TranslationSchema["viewer"];

/**
 * 표시 시점에 현재 언어로 포맷되는 오류. 발생 지점(세션/페어링/런처 등
 * React 바깥 모듈)은 i18n 키만 들고, 화면의 formatErrorMessage 경계에서
 * 번역한다 — 오류가 오래 잡혀 있다가 뒤늦게 표시돼도 언어 전환이 반영된다.
 * "{detail}" 플레이스홀더를 가진 템플릿은 params.detail로 원인을 넘긴다.
 */
export class LocalizedError extends Error {
  constructor(
    readonly key: ViewerTextKey,
    readonly params: Record<string, string | number> = {},
  ) {
    super(`leftcar:${key}`);
    this.name = "LocalizedError";
  }

  format(): string {
    return interpolate(currentTranslation().viewer[this.key], this.params);
  }
}
