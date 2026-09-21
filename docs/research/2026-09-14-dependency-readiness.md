# 2026-09-14 의존성 후속 검증

이번 변경은 **Android Maven 취약점 8건 중 6건을 제거하고, 선언 라이선스 미확인 22건의 출처를 확인**했다. Bun 취약점 4건과 Commons IO 취약점 2건은 남는다. 전체 APK 빌드, Android 기기 실행, 공개 배포 승인과는 별도 결과다. 기계 판독 기록과 모든 입력·출력 SHA-256은 [검사 요약](../superpowers/evidence/dependencies-2026-09-14/summary.json)에 있다.

## 실제 변경과 결과

| 대상 | 변경 | 실제 확인 |
| --- | --- | --- |
| Gson | 2.8.6 → **2.8.9** | Gradle `releaseRuntimeClasspath`의 선택 버전, OSV 재조회, JSON 왕복 검증 |
| Bouncy Castle | bcprov/bcpkix/bcutil **jdk15to18 1.78.1 → 1.86** | 세 모듈의 실제 선택 버전 일치, OSV 재조회, tcp-socket이 사용하는 PEM API 검증 |
| Gradle 라이선스 수집 | 정확한 부모 POM 상속과 Expo 로컬 Maven 발행 메타데이터 지원 | 현재 248개 모두 선언 확인: 직접 POM 227, 부모 POM 7, Expo 발행 정보 14 |
| 도구 XML 파서 | 기존 lock에 있던 `@xmldom/xmldom 0.9.12`를 root devDependency로 명시 | frozen install 통과. Expo plist의 요구 버전 0.8.15도 유지 |

Gson 2.8.9는 해당 역직렬화 취약점의 수정 버전이며 기존 Java 6 범위를 유지한다. 사용하는 상위 패키지는 `@expo/log-box`다. Bouncy Castle은 `react-native-tcp-socket`이 사용하는 Java 5–8 계열을 유지했고, 상위 코드에서 쓰는 `PemReader`/`PemObject` 호출을 검증했다. Maven Central에서 현재 발행된 1.86과 공식 배포 링크를 대조했다. [Gson 변경 기록](https://google.github.io/gson/CHANGELOG.html), [Gson advisory](https://github.com/advisories/GHSA-4jrv-ppp4-jm57), [Bouncy Castle 공식 배포](https://www.bouncycastle.org/download/bouncy-castle-java/), [bcprov 발행 metadata](https://repo.maven.apache.org/maven2/org/bouncycastle/bcprov-jdk15to18/maven-metadata.xml)

의존성은 앱의 Gradle `constraints`로 최소 버전을 정했다. 패키지를 새로 추가하거나 Expo/React Native를 일괄 갱신하지 않았다. `jdk18on`으로 전환하지 않았다. 선택된 버전과 실제 API 호출 검증은 확인했지만 Android 24/25 기기에서의 실행이나 전체 APK 수락까지 확인한 것은 아니다.

## Maven 감사: 이전 후보 8건 → 변경 후 2건

이전 D49wkS 내부 후보 manifest의 248개 좌표 중 **공개 Maven 234개**를 공식 OSV API로 조회했다. npm 패키지 내부의 `local-maven-repo`에 발행된 Expo 14개는 공개 Maven 조회에서 제외했으며 별도 네이티브 취약점 검증을 완료한 것으로 세지 않았다. 요청에는 공개 좌표와 버전만 전송했다. 소스, 앱 상태, 인증 정보는 전송하지 않았다. [OSV 조회 계약](https://google.github.io/osv.dev/post-v1-querybatch/)

| 패키지 | 이전 OSV 결과 | 현재 결과 |
| --- | --- | --- |
| Gson 2.8.6 | high 1 | 2.8.9: 0 |
| Bouncy Castle bcpkix 1.78.1 | moderate 2 | 1.86: 0 |
| Bouncy Castle bcprov 1.78.1 | critical 1, high 1, moderate 1 | 1.86: 0 |
| Commons IO 2.6 | high 1, moderate 1 | **2건 유지** |

[이전 요청·응답과 advisory 상세](../superpowers/evidence/dependencies-2026-09-14/gradle-osv.json), [수정 후 실제 Gradle 보고서](../superpowers/evidence/dependencies-2026-09-14/gradle-runtime-after.txt), [수정 후 전체 inventory와 라이선스 출처](../superpowers/evidence/dependencies-2026-09-14/gradle-runtime-after.json), [수정 후 OSV 요청·응답](../superpowers/evidence/dependencies-2026-09-14/gradle-osv-after.json)

Commons IO 2.14.0 이상이 두 advisory를 해소하지만 현재 앱의 최소 Android 버전은 **24**다. 현재 Expo FileSystem은 26 미만의 `File.toPath()` 미지원 때문에 별도의 삭제 구현을 남겨 두었다. 2.14.0의 `FileUtils`는 NIO 경로를 사용하므로 버전 제약 하나로 교체하면 API 24/25의 파일 복사·삭제 경로에 위험이 생긴다. 최소 Android 버전을 조용히 올리거나 NIO desugaring을 검증 없이 추가하지 않았다. 후속 해결에는 지원 범위를 유지하는 NIO desugaring/상위 패키지 이관과 API 24/25 파일 작업 검증이 필요하다. [Commons IO 보안 공지](https://commons.apache.org/proper/commons-io/security.html), [2.14.0 FileUtils 소스](https://raw.githubusercontent.com/apache/commons-io/rel/commons-io-2.14.0/src/main/java/org/apache/commons/io/FileUtils.java), [Android File.toPath](https://developer.android.com/reference/java/io/File#toPath())

현재 관찰한 Expo 호출은 DocumentPicker의 `FilenameUtils.getExtension`, FileSystem의 `FileUtils` 복사·삭제와 `IOUtils.copy`다. advisory의 `FilenameUtils.normalize`와 `XmlStreamReader`를 직접 호출하는 앱 코드는 찾지 못했다. 이는 취약 버전 제거나 모든 경로의 비도달성 증명이 아니다. [경로 정규화 advisory](https://github.com/advisories/GHSA-gwrp-pvrq-jmwv), [XML reader advisory](https://github.com/advisories/GHSA-78wr-2p64-hpwj)

이 OSV 조회는 **별도 감사 기록**이다. 기존 `release:preflight`의 Gradle scanner 상태는 여전히 `unavailable`이고, 그 스키마를 이 기록으로 대체하거나 성공 처리하지 않았다. Gradle dependency lock과 검증 신뢰 정책도 아직 없다. 자동으로 현재 캐시 전체를 신뢰하는 verification metadata는 생성하지 않았다.

## Bun 4건의 경계

전체 lock에 대한 최신 `bun audit --json`은 종료 코드 **1**, high 2/moderate 2/critical 0이다. [원본 감사 결과](../superpowers/evidence/dependencies-2026-09-14/bun-audit-after.json), [공식 npm 최신 metadata](../superpowers/evidence/dependencies-2026-09-14/npm-upstream-metadata.json)

- **image-size 1.2.1: high 2.** 현재 최신 2.0.2도 두 공지의 영향 범위에 있으며 수정 릴리스가 없다. Metro의 이미지 처리에서 호출한다. `image-size`는 확장자 대신 바이트를 탐지하므로 PNG 확장자만 허용해도 조작된 ICNS/JXL/HEIF 입력을 차단했다고 볼 수 없다. `disableTypes`는 프로세스 전역 상태이므로 Metro worker까지 적용되는지 확인 없이 한 곳에 호출하는 완화를 추가하지 않았다. 빌드에는 출처를 검토한 이미지 자산을 사용하고 신뢰하지 않는 이미지 입력을 제외해야 한다. 패키지 취약 상태 자체는 열린다. [ICNS 공지](https://github.com/advisories/GHSA-w3rx-r6r6-pgpr), [JXL/HEIF 공지](https://github.com/advisories/GHSA-5p2g-fcmc-qvqq)
- **decode-uri-component 0.2.2: moderate 1.** 현재 최신 Expo Router 57.0.21도 query-string ^7.1.3을 요구하며 그 범위의 최신 7.1.3은 디코더 ^0.2.2를 사용한다. 수정 0.5.0과 query-string 9.5.1은 ESM이며 기존 허용 범위 밖이다. 설치된 Router의 기본 링크 파서는 URL `searchParams`를 사용하고, 다른 내장 React Navigation 파서는 `queryString.parse`를 유지한다. 이 차이만으로 전체 앱의 비도달성을 판정하지 않았다. 강제 override는 추가하지 않았다. [디코더 수정 릴리스](https://github.com/SamVerschueren/decode-uri-component/releases/tag/v0.5.0), [advisory](https://github.com/advisories/GHSA-vcc3-ghjq-m6fr)
- **uuid 7.0.3: moderate 1.** xcode 최신 3.0.1은 여전히 ^7.0.3을 요구한다. 설치된 xcode는 `uuid.v4()`에 출력 버퍼 없이 호출하며 공지가 지적한 v3/v5/v6 버퍼 쓰기를 호출하지 않는다. xcode의 관찰 경로에서는 영향 API를 사용하지 않지만 패키지는 여전히 감사 대상이며 수정 상태로 표시하지 않는다. [uuid 공지](https://github.com/advisories/GHSA-w5hq-g745-h8pq), [xcode 공식 패키지](https://registry.npmjs.org/xcode/3.0.1)

## 라이선스와 Rust

라이선스 수집기는 XML의 루트 `licenses`만 읽고 정확한 group/artifact/version을 확인한다. 부모 좌표의 순환·미해결 변수·잘못된 identity·복구가 필요한 XML은 미확인으로 남긴다. Expo 로컬 발행은 `expo-module.config.json`의 publication 좌표·버전과 package.json 버전이 일치해야 하며 두 파일의 SHA를 남긴다. 캐시 파일이나 node_modules 생성물을 편집하지 않았다. 라이선스 선언을 찾았다는 것은 사용 정책 승인이나 배포 NOTICE 작성 완료를 의미하지 않는다. [Maven의 부모 POM 상속 계약](https://maven.apache.org/pom.html#inheritance)

기본 로컬 RustSec 캐시가 이전 조사와 다른 DB를 사용함을 확인해, 공식 저장소를 임시 디렉터리에 새로 읽고 명시적 `--db`로 두 lock을 다시 검사했다. 이 DB는 1,243개 advisory이며 root/Host vulnerability는 **0/0**, Host 정보성 경고는 **7개**다. DB commit과 각각의 원본 결과는 요약 JSON에 남긴다. 기존 7개 경고의 target별 분석은 [이전 보고서](2026-09-13-dependency-readiness.md)를 따르며 이번 검사에서 재분석한 것으로 세지 않는다. [공식 RustSec DB](https://github.com/RustSec/advisory-db)

## 실행한 검증과 남은 수락

- 라이선스 수집기: 수정 전 신규 3개 테스트 실패를 확인한 뒤 관련 **21개 테스트 통과**.
- `bun install --frozen-lockfile --ignore-scripts`: 통과. 수정 후 Bun audit의 4건은 그대로 보존.
- 실제 `:app:dependencies --configuration releaseRuntimeClasspath`: 통과. 현재 248개 좌표와 선택 버전을 다시 읽고 공개 234개를 OSV에 재조회.
- Java 8 대상으로 컴파일한 JVM probe: Gson JSON 왕복, PEM 내용 읽기, 잘린 PEM 거부 **3개 통과**. 사용 클래스의 bytecode는 Gson Java 6, PEM reader Java 5 수준이다. [probe와 다운로드 artifact SHA 기록](../superpowers/evidence/dependencies-2026-09-14/jvm-receipt.json)

새 APK 패키징과 해당 APK의 실기기 실행, API 24/25 호환성, Commons IO의 두 공지, Bun 네 공지, Gradle lock/신뢰 정책/상시 scanner 연결, NOTICE·정책 승인, 기존 Rust 정보성 경고는 각각 별도의 남은 범위다. 이 보고서는 물리 디스플레이 조작이나 가상 디스플레이 생성 실험을 수행하지 않았다.
