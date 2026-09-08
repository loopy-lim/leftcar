import { Reader } from './complex-codec-reader.js';
import type { CompiledNode } from './complex-codec-compiled.js';
/**
 * 컴파일된 IR 을 순회하는 디코더 — 런타임 스키마 재해석 없음. 원본 분기
 * 순서(옵션 태그, oneOf 인덱스, enum 인덱스, 타입 디스패치)와 에러 문자열을
 * 유지한다.
 */
export declare function decodeNode(reader: Reader, node: CompiledNode, maxDepth: number, depth: number, maxCollectionLength: number): unknown;
//# sourceMappingURL=complex-codec-decode-node.d.ts.map