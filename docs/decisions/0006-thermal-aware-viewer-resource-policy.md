# ADR-0006: thermal 인지 뷰어 자원 정책은 뷰어 TS 정책 계층이 판단한다

- 상태: 제안 (실기 데이터 확정 후 수용 전환)
- 날짜: 2026-10-20

## 상황

R-011(4창 Galaxy XR thermal/battery 급악화)의 완화책으로 "dynamic FPS/resolution, charger mode, 상한"이 리스크 레지스터에 지정돼 있으나, 어느 계층이 판단하는지는 정해져 있지 않았다. 진단 캡처(docs/plans/2026-10-19-viewer-diagnostics.md)가 배터리·thermal을 15초 샘플러로 NDJSON에 기록하게 되면서 런타임 입력은 존재하지만 JS로 노출되지 않는 상태였다.

계층별 제약:

- **Kotlin**: docs/09 §5가 Kotlin MediaCodec quality policy와 판단 로직을 금지한다. 진단 샘플러도 "판단 없는" 설계를 계약으로 가진다.
- **호스트(macOS/Windows)**: 뷰어가 제안한 width/height/fps를 경계 검사만 하고 수용하는 것이 현재 계약이다(control.rs `validate_stream_shape`). 호스트는 기기 열·배터리를 알 수 없다.
- **Rust(viewer-decoder/android-viewer)**: 디코더 사실(capability hint, 릴리스 경로)은 다루지만 세션 자원 정책의 자리는 아니다.

## 결정

1. **판단은 뷰어 TS 정책 계층**(`thermal-policy.ts` 순수 함수 + `thermal-state.ts` 캐시)이 한다. 진입/해제 임계, capacity 상한, 시작 단변 상한, 저배터리 규칙을 전부 상수 표로 고정한다.
2. **Kotlin은 판단 없는 전달자**다. DiagnosticsSampler 틱이 최신 샘플을 `@Volatile`로 보관하고 `getLatestSample`로 전달한다. 새 폴링 루프를 만들지 않는다(전력·JNI 비용이 R-011을 악화한다).
3. **적용은 최소 개입**: 시작 앵커 캡(`resolveInitialStreamTarget`의 min 체인), 디코더 capacity overlay(`capacity()=min(hint, overlay)`), 적응 upshift 클램프. 라이브 스트림 강제 강등·창 퇴거는 하지 않는다.
4. **임계는 보수적으로**: 진입 SEVERE, 해제 LIGHT(히스테리시스), MODERATE는 관측만. 실기 thermal 분포 데이터가 없는 상태에서 오탈 비용이 큰 완화를 걸지 않는다. 값은 실기 데이터 후 조정 지점으로 문서에 남긴다.
5. **charger mode의 해석**: 충전은 발열을 가중하므로 thermal 상한을 완화하지 않는다. 배터리 축으로만 해석한다 — `batteryPercent ≤ 15 && !charging`일 때 상한을 추가로 낮춘다.
6. **호스트는 무변경**: 뷰어가 낮춘 수치는 기존 수용 계약으로 전파된다. thermal 시 split 승격(정확 4K60 요구)은 뷰어 overlay가 사전 차단한다.

## 결과

- thermal 판단 로직이 vitest로 검증 가능한 순수 함수로 고정된다.
- 진단 기록과 판단이 같은 원천(샘플러 값)을 쓰므로 "기록된 값으로 판단했다"가 성립한다.
- 기본 동작 보존: 첫 샘플 없음·네이티브 부재 시 완화 미적용(현행과 동일).
- 미검증: 임계값의 실기 타당성은 수집(docs/plans/2026-10-20-thermal-cadence-rendezvous-plan.md T5.2) 전까지 열려 있다.
