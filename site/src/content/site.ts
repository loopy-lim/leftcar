export interface Feature {
  id: string;
  title: string;
  description: string;
}

export interface SecurityItem {
  id: string;
  title: string;
  description: string;
}

export interface StatusItem {
  id: string;
  title: string;
  status: string;
  highlight: boolean;
}

export interface FooterLink {
  id: string;
  label: string;
  url: string;
}

export const SITE_CONTENT = {
  hero: {
    badge: 'MIT 라이선스 · 오픈 소스',
    versionBadge: 'v0.1.4 (2026-09-13)',
    headline: '승인한 PC 화면을, 손안의 여러 창으로',
    subcopy: 'Leftcar는 Mac 또는 Windows PC의 화면을 Android 휴대폰과 태블릿에서 빠르게 보고, 필요할 때 키보드와 포인터로 조작하는 다중 화면 뷰어입니다. 사용자가 승인한 디스플레이만 신뢰하는 로컬 네트워크로 전송됩니다.',
    cta1: {
      label: 'v0.1.4 Viewer APK 받기',
      url: 'https://github.com/loopy-lim/leftcar/releases/tag/v0.1.4',
    },
    cta2: {
      label: 'GitHub에서 보기',
      url: 'https://github.com/loopy-lim/leftcar',
    },
  },
  features: [
    {
      id: 'f1',
      title: '여러 화면, 여러 창',
      description: '컴퓨터마다 Leftcar를 실행하고, 원하는 디스플레이를 Android에서 독립된 창으로 여세요. 특정 기기 전용 기능이 아니라 Android 표준 다중 창을 사용합니다.',
    },
    {
      id: 'f2',
      title: '릴레이 없는 직접 연결',
      description: '같은 로컬 네트워크 안에서 PC와 직접 연결됩니다. 압축 영상과 고주파 입력은 JavaScript를 거치지 않는 별도 네이티브 데이터 경로로 전송됩니다.',
    },
    {
      id: 'f3',
      title: '키보드·포인터 조작',
      description: '세션별로 입력을 허용하면 Android 화면에서 PC를 조작할 수 있습니다. 포인터 전송률은 영상 FPS의 2배로 제한됩니다.',
    },
    {
      id: 'f4',
      title: '파일 전송·클립보드 동기화',
      description: '선택 기능으로 파일을 주고받고 클립보드를 동기화합니다. 두 기능 모두 Host와 Viewer 양쪽의 동의 게이트를 유지합니다.',
    },
  ] as Feature[],
  security: {
    lead: '화면은 민감한 데이터입니다. Leftcar는 승인된 것만 전송하고, 입력은 기본적으로 꺼져 있습니다.',
    items: [
      {
        id: 's1',
        title: '기기별 화면 승인',
        description: 'Host 사용자가 각 기기의 화면 접근을 명시적으로 승인해야 합니다.',
      },
      {
        id: 's2',
        title: '입력 기본 OFF',
        description: '새 세션은 원격 입력이 꺼진 상태로 시작하며, 세션별로 별도 허용합니다.',
      },
      {
        id: 's3',
        title: '암호화된 제어·미디어',
        description: 'QR로 핀된 호스트 Ed25519 키 핸드셰이크 뒤 ChaCha20-Poly1305 AEAD로 봉인합니다.',
      },
      {
        id: 's4',
        title: '로컬 네트워크 전용',
        description: '릴레이 없는 직접 연결만 전제하며, 공개 인터넷 노출은 범위에 넣지 않습니다.',
      },
    ] as SecurityItem[],
  },
  stack: {
    badges: ['Rust workspace', 'Tauri 2 (macOS/Windows Host)', 'Expo/React Native (Android Viewer)', '네이티브 캡처/디코더', 'CI'],
    description: 'Rustra는 Rust와 TypeScript 사이의 명령·상태·오류 계약에만 사용하고, 제품 로직은 TypeScript와 Rust로 작성합니다.',
  },
  status: {
    lead: '상태 기준일 2026-09-13. 최신 공개 릴리스는 v0.1.4이며 Leftcar-Viewer-0.1.4.apk를 제공합니다.',
    items: [
      { id: 'st1', title: 'macOS Host', status: '우선 대상', highlight: false },
      { id: 'st2', title: 'Windows Host', status: '코드·교차 컴파일 완료, 물리 기기 검증 진행 중', highlight: true },
      { id: 'st3', title: 'Linux Host', status: '선택적 후속 과제 (개발 예정)', highlight: true },
      { id: 'st4', title: '장시간(10·30분) 영상 수용 검증', status: '개발 중', highlight: true },
      { id: 'st5', title: '앱 창 단독 캡처', status: '후속 목표 (개발 예정)', highlight: true },
    ] as StatusItem[],
    note: '공개 APK와 개발 소스의 검증 결과는 구분해 안내합니다.',
  },
  footer: {
    links: [
      { id: 'fl1', label: 'GitHub 저장소', url: 'https://github.com/loopy-lim/leftcar' },
      { id: 'fl2', label: '릴리스', url: 'https://github.com/loopy-lim/leftcar/releases' },
      { id: 'fl3', label: '문서', url: 'https://github.com/loopy-lim/leftcar/blob/main/docs/README.md' },
      { id: 'fl4', label: 'MIT License', url: 'https://github.com/loopy-lim/leftcar/blob/main/LICENSE' },
    ] as FooterLink[],
    copyright: 'Leftcar — 로컬 네트워크 기반 PC→Android 다중 화면 뷰어',
  },
};
