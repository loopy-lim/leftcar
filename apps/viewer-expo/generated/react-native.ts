// ── rustra generated ────────────────────────────────────────
// File:   react-native.ts
// Source: schema.json (single source of truth for this file)
// Regen:  rustra codegen --config rustra.json
// Stage:  schema → host entry
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────

// ── 계약 검증 정책(호스트 엔트리 공통) ───────────────────────────
// 공통 기본값: contractVerification = 'strict' — 스키마/바이너리 불일치는
// contract.mismatch 로 즉시 실패한다. OTA 롤백 등 의도적 드리프트에만
// 'warn'/'off' 로 바꿔 쓴다(생성 파일의 이 한 줄이 공식 탈출구).
// 이 엔트리(react-native): contractHash + contractVerification: 'strict' + schemaVersion 전달
// 이 엔트리(react-native): (프레임 엔진 전체 옵션 집합 — bun 엔트리와 동일).
// Changelog(M9 옵션 정합): node/bun/tauri/react-native 4개 엔트리가 동일한
//   계약 검증 기본값('strict')과 옵션명을 명시한다. 옵션명 변경/제거는
//   없다 — 기존 생성 엔트리 코드는 그대로 동작한다(하위 호환).
import { createRustraBootstrap } from '@rustra/react-native';
import { installRustraJSI, getRustraNative } from "@rustra/generated-react-native";
import { GENERATED_CONTRACT_HASH, SCHEMA_VERSION } from './contract.js';
import { frameRegistry } from './frame-registry.js';

export * from './commands.js';
export { subscribeEvent } from '@rustra/react-native';

export const rustra = createRustraBootstrap({
  install: installRustraJSI,
  getNative: getRustraNative,
  frameCodecs: frameRegistry,
  contractHash: GENERATED_CONTRACT_HASH,
  contractVerification: 'strict',
  schemaVersion: SCHEMA_VERSION,
});
