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

export const SITE_VERSION = 'v0.1.4';

export const SITE_CONTENT = {
  hero: {
    badge: `MIT 라이선스, 오픈 소스, ${SITE_VERSION}`,
    headline: '승인한 PC 화면을, 손안의 여러 창으로',
    subcopy: 'Leftcar는 승인한 기기와만 로컬 네트워크로 직접 연결되어, PC 화면을 Android의 여러 독립된 창으로 보여 줍니다. 키보드와 포인터 조작도 폰 화면 위에서 그대로입니다.',
    cta1: {
      label: 'Viewer APK 받기',
      url: `https://github.com/loopy-lim/leftcar/releases/tag/${SITE_VERSION}`,
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
      description: '같은 로컬 네트워크 안에서 PC와 직접 연결되고, 압축 영상과 고주파 입력은 JavaScript를 거치지 않는 네이티브 데이터 경로로 흐릅니다.',
    },
    {
      id: 'f3',
      title: '키보드·포인터 조작',
      description: '세션별로 입력을 허용하면 Android 화면에서 PC를 조작할 수 있습니다. 포인터 전송률 상한은 영상 FPS의 2배.',
    },
    {
      id: 'f4',
      title: '파일 전송·클립보드 동기화',
      description: '파일 전송과 클립보드 동기화는 선택 기능입니다. 두 기능 모두 Host와 Viewer 양쪽의 동의 게이트를 거칩니다.',
    },
  ] as Feature[],
  featuresLead: 'PC 화면을 폰에서, 여러 개로.',
  security: {
    lead: '화면은 민감합니다. 승인한 기기에만 보여 주고, 입력은 기본으로 잠가 둡니다.',
    items: [
      {
        id: 's1',
        title: '기기별 화면 승인',
        description: 'Host 사용자가 각 기기의 화면 접근을 명시적으로 승인해야 합니다.',
      },
      {
        id: 's2',
        title: '입력 기본 OFF',
        description: '새 세션은 원격 입력이 꺼진 상태로 시작하고, 허용 여부는 세션별로 따로 정합니다.',
      },
      {
        id: 's3',
        title: '암호화된 제어·미디어',
        description: '첫 연결은 QR 코드로 서로 확인하고, 그 뒤의 화면·입력 데이터는 모두 암호화해 주고받습니다. 사용하는 암호 방식은 문서에서 설명합니다.',
      },
      {
        id: 's4',
        title: '로컬 네트워크 전용',
        description: '릴레이 없는 직접 연결만 전제합니다. 공개 인터넷 노출은 범위 밖입니다.',
      },
    ] as SecurityItem[],
  },
  stack: {
    badges: ['Rust workspace', 'Tauri 2 (macOS/Windows Host)', 'Expo/React Native (Android Viewer)', '네이티브 캡처/디코더', 'CI'],
    description: 'Rust와 TypeScript 사이의 명령·상태·오류 경계는 Rustra가 관리하는 계약으로 정의하고, 제품 로직은 TypeScript와 Rust로 작성합니다.',
  },
  status: {
    lead: `상태 기준일 2026-09-13, 최신 릴리스 ${SITE_VERSION}`,
    items: [
      { id: 'st1', title: 'macOS Host', status: '우선 대상', highlight: true },
      { id: 'st2', title: 'Windows Host', status: '코드·교차 컴파일 완료, 물리 기기 검증 진행 중', highlight: false },
      { id: 'st3', title: 'Linux Host', status: '선택적 후속 과제 (개발 예정)', highlight: false },
      { id: 'st4', title: '장시간(10·30분) 영상 수용 검증', status: '개발 중', highlight: false },
      { id: 'st5', title: '앱 창 단독 캡처', status: '후속 목표 (개발 예정)', highlight: false },
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
