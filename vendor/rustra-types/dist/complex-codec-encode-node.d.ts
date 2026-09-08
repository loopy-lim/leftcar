import { Writer } from './complex-codec-wire.js';
import type { CompiledNode } from './complex-codec-compiled.js';
/**
 * 컴파일된 IR 을 순회하는 인코더 — 스키마 해석(`resolvedSchema`/`optionInner`/
 * `variants`)을 호출마다 재계산하지 않고 노드 결정만 소비한다. 와이어는 기존
 * `&ComplexSchema` 해석 버전과 바이트 단위로 동일하다(원본 분기 순서·에러
 * 문자열 유지).
 */
export declare function encodeNode(writer: Writer, node: CompiledNode, value: unknown, maxDepth: number, depth: number, maxCollectionLength: number): void;
//# sourceMappingURL=complex-codec-encode-node.d.ts.map