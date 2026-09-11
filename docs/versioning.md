# 버전 문자열 현황

릴리스 판단 때 참조하는 현재 버전 문자열 기록이다. 아래는 사실상(de-facto)
관행을 서술한 것이며 새 규칙이나 승격 약속이 아니다.

| 위치 | 현재 값 | 역할 |
| --- | --- | --- |
| `apps/viewer-expo/android/app/build.gradle` `versionName` | `0.1.5` | Viewer Android 릴리스 기준 |
| `apps/viewer-expo/android/app/build.gradle` `versionCode` | `4` | Viewer Android 설치 판별 값 |
| `apps/viewer-expo/app.config.ts` `version` | `0.1.2` | Expo 앱 표시 버전 |
| `apps/viewer-expo/package.json` `version` | `0.1.0` | 개발 트래킹용 |
| `apps/host-desktop/src-tauri/Cargo.toml` `version` | `0.1.2` | macOS/Windows Host 빌드 버전 |

- 사실상 정책: Viewer Android의 `versionName`/`versionCode`가 릴리스 판별의
  기준이고, 나머지 버전 문자열은 개발 진행을 따라간다.
- 미해결(공개 배포 전 항목): Android APK는 현재 debug 키로 서명되어 있다.
  release 서명 체계는 별도 릴리스 결정 사항으로 이 문서에서 정하지 않는다.
