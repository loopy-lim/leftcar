import { RustraCommandError } from './errors.js';
import type { RkyvV2Codec } from './public.js';
/**
 * tier 2(JS 코덱) 응답 프레임을 결과/에러로 환산한다 — `dispatch` 와 전파
 * 경로 콜백이 공유하는 유일 경로 (T1 리뷰). `codec.decode` 가 잘못된 프레임으로
 * throw 하면 그 예외를 reject 값으로 돌린다(비-Error 는 `invoke.failed` 로
 * 래핑): 전파 경로의 콜백은 네이티브 트램펄린 안에서 실행되므로 예외가
 * 새어나가면 프라미스가 영원히 정착하지 않는다. 이 함수 자체는 throw 하지 않는다.
 */
export declare function tier2Outcome<T>(codec: RkyvV2Codec<unknown, unknown>, frame: ArrayBuffer | ArrayBufferView): {
    ok: true;
    value: T;
} | {
    ok: false;
    error: Error;
};
/**
 * (T3) 인코딩된 페이로드의 크기 사전 검사 — JS 코덱(tier 2)/tier 3 경로가
 * 네이티브를 호출하기 직전에 공유한다. `limit` 이 undefined 면 검사하지 않는다
 * (네이티브의 동적 한도가 최종 게이트). 초과 시 `payload.too_large`
 * (non-retryable — 결정론적 클라이언트 조건) 를 반환하고 호출자는 네이티브
 * 왕복 없이 즉시 reject 한다.
 */
export declare function payloadTooLargeError(encodedBytes: number, limit: number | undefined): RustraCommandError | undefined;
import type { RkyvV2SchemaNative } from './live-schema.js';
import type { RkyvSchemaRuntime } from './rkyv-engine-context.js';
import type { RkyvV2EngineOptions } from './rkyv-engine-options.js';
export declare function validateRkyvEngineOptions(native: RkyvV2SchemaNative, options: RkyvV2EngineOptions | undefined, schema: RkyvSchemaRuntime): void;
//# sourceMappingURL=rkyv-engine-contract.d.ts.map