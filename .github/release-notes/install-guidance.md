## 설치 참고 (무료 CI 빌드, 미공증)

이 릴리스 파일들은 공개 저장소의 GitHub Actions 러너에서 만들었고 상용
코드 서명 인증서가 없다. 그래서 운영체제가 처음 실행을 막을 수 있다.
파일이 변조된 게 아니라 서명·공증이 없어서 나오는 경고다. 무결성은
`SHA256SUMS.txt`로 확인할 수 있다.

### macOS (`Leftcar-Host-*-macOS-arm64.zip`)

- Apple Silicon 전용 ad-hoc 서명 빌드다. 공증(notarization)이 없어서
  첫 실행이 차단된다.
- zip 해제 후 `Leftcar Host.app`을 응용 프로그램 폴더로 이동한다.
  (`아카이브 유틸리티` 더블클릭 대신 터미널이면
  `ditto -x -k Leftcar-Host-*.zip .` 권장)
- 차단 알림이 뜨면: 시스템 설정 → 개인정보 보호 및 보안 → 아래쪽
  "그래도 열기". 또는 터미널에서
  `xattr -dr com.apple.quarantine /Applications/Leftcar\ Host.app`.
- 서명 identity가 이전 릴리스(0.1.10, Apple Development 인증서)와 달라
  화면 기록·손쉬운 사용 권한을 시스템 설정에서 다시 승인해야 할 수 있다.
- 공증 경고 없이 배포하려면 Apple Developer Program($99/년) 등록 후
  CI에 서명·공증 비밀을 넣으면 된다.

### Windows (`Leftcar.Host_*_x64-setup.exe`)

- 서명되지 않은 설치 파일이다. SmartScreen의 "Windows가 PC를 보호했습니다"
  화면이 뜨면 추가 정보 → 실행을 선택한다.
- 브라우저가 `.exe`를 위험 파일로 표시할 수 있다. 마찬가지로 서명
  경로(코드 서명 인증서 구매)를 열면 사라지는 경고다.

### Android 뷰어

이 워크플로는 Host만 만든다. 뷰어 APK는 기존 설치(내부 디버그 키)와의
서명 호환을 위해 로컬 `bun run build android-internal`로 유지한다.
