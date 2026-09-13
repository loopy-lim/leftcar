# 2026-09-14 라이선스 수집기 리뷰 수정

[앞선 의존성 조사](2026-09-14-dependency-readiness.md)의 수집기에서 발견한 메타데이터 판정 오류 두 건을 수정했다. 기존 조사·감사 원본은 날짜별 증거로 보존하고, 이번 수정 결과는 [별도 기록](../superpowers/evidence/dependencies-2026-09-14/review-fix/summary.json)에 남겼다.

1. **Maven 단일 필드의 중복을 거부한다.** project의 identity·parent·licenses, parent의 좌표, 개별 license의 name·URL처럼 하나여야 하는 필드를 중복 선언하면 첫 항목을 선택하지 않고 미확인으로 남긴다. `licenses` 내부의 합법적인 여러 `license` 항목은 유지한다.
2. **자식의 명시적 라이선스 목록을 부모 이름으로 대체하지 않는다.** 자식 목록에 URL만 있거나 이름이 비어 있으면 부모의 MIT 이름을 가져오지 않는다. 여러 자식 라이선스 중 하나의 이름이 없는 경우도 일부 선언만으로 확정하지 않는다. 부모 상속은 자식 목록이 비었을 때만 허용한다. 이는 Maven의 `mergeModel_Licenses`가 자식 목록의 비어 있음으로 상속을 판단하는 동작을 따른다. [Maven 3.9.11 원본 merger](https://maven.apache.org/ref/3.9.11/xref/org/apache/maven/model/merge/MavenModelMerger.html)

독립 리뷰가 제공한 두 재현 입력을 확인한 뒤, 중복 필드 10개와 이름 없는 자식 목록 3개로 **13개 테스트가 수정 전 실패**함을 확인했다. 수정 후 수집기와 release-inputs 관련 **35개 테스트가 통과**했다. 빈 자식 목록의 부모 상속과 이름 있는 복수 라이선스 목록의 보존도 통과했다. [실패 기록](../superpowers/evidence/dependencies-2026-09-14/review-fix/red.log), [수정 후 기록](../superpowers/evidence/dependencies-2026-09-14/review-fix/green.log)

기존 실제 Gradle 출력과 현재 캐시를 새 수집기로 다시 읽은 결과는 **248개 선언 확인, 미확인 0개**이며, 직접 POM 227·부모 POM 7·Expo 로컬 발행 정보 14개다. 전체 기록은 이전 결과와 바이트 단위로 동일하다. 이번 재검증에서는 Gradle이나 빌드를 실행하지 않았고, OSV 취약점 감사를 새로 실행한 것으로 세지 않는다. [재수집 기록과 입력 SHA](../superpowers/evidence/dependencies-2026-09-14/review-fix/inventory-replay.json)

코드 변경은 `tools/release-gradle-licenses.mjs`와 해당 `release-gradle.test.mjs`로 한정했다. 최소 Android 버전·의존성 버전·앱 동작·scanner 상태는 변경하지 않았다. 앞선 Bun 4건, Commons IO 2건, Rust Host 정보성 경고 7건 및 전체 APK·실기기 수락의 구분은 유지된다. 라이선스 선언 출처 확인은 법적 사용 승인이나 NOTICE 완료를 의미하지 않는다.
