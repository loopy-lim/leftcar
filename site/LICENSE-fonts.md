# 사이트 폰트 라이선스 고지

이 사이트는 다음 폰트를 self-host로 배포합니다. 라이선스 원문은 [`public/fonts/OFL.txt`](public/fonts/OFL.txt)에 함께 배포됩니다.

## Wanted Sans Bold (서브셋)

| 항목 | 내용 |
|---|---|
| 폰트명 | Wanted Sans |
| 버전 | 1.0.3 |
| 웨이트 | Bold 700 (1웨이트만 사용) |
| 라이선스 | SIL Open Font License 1.1 (OFL.txt 참조) |
| 저작권 | Copyright 2024 The Wanted Sans Project Authors |
| 출처 | https://github.com/wanteddev/wanted-sans/releases/tag/v1.0.3 |
| 배포 파일 | `public/fonts/WantedSans-Bold.subset.woff2` |

### 서브셋 안내

배포 파일은 SIL OFL 1.1이 허용하는 범위에서 사이트 카피 사용 문자(387자) + 버퍼 문자집합으로 축소한 **커스텀 서브셋**입니다. 서브셋 재생성 절차는 `scripts/subset-font.sh` 스크립트 주석을 참조하세요. OFL 1.1에 따라 원본 폰트의 판매 행위를 제외한 사용·수정·재배포가 허가되며, 폰트 저작물로서의 라이선스 원문을 함께 배포했습니다.

### 적용 범위

히어로 헤드라인(`.hero-headline`)과 섹션 제목(`.section-title`)에만 적용되며, 본문·리드·카드 등은 시스템 UI 폰트 스택(라이선스 고지 불필요)을 사용합니다.
