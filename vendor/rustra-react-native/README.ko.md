# @rustra/react-native

Rustra의 React Native JSI 어댑터와 생성 모듈용 공유 네이티브 소스입니다. Expo와
bare React Native에서 같은 generated entry와 표준 autolinking 경로를 사용합니다.

## 권장 사용

Rust crate는 정적 라이브러리와 mobile entry를 노출합니다.

```toml
[lib]
crate-type = ["rlib", "staticlib"]
```

```rust
pub fn package() -> rustra::Package {
    // commands...
}

rustra::native_entry!(package);
```

앱의 `rustra.json`에서 React Native 생성을 켭니다.

```json
{
  "schema": "./generated/schema.json",
  "output": "./generated",
  "codegen": {
    "rustManifest": "./Cargo.toml",
    "rustBinary": "generate"
  },
  "reactNative": {}
}
```

```bash
bun add @rustra/react-native @rustra/types
bun add -d @rustra/cli
bunx --bun @rustra/cli doctor --config rustra.json
bunx --bun @rustra/cli codegen --config rustra.json
bun install
```

생성기는 다음을 자동으로 처리합니다.

- `modules/rustra-bridge`에 앱 전용 `@rustra/generated-react-native` 패키지 생성
- 앱 `package.json`에 충돌을 확인한 뒤 Bun workspace dependency 연결
- iOS Podspec, Android Gradle/CMake/JNI, 공유 C++ JSI bridge 생성
- Cargo metadata에서 package와 static library 이름 추론
- TypeScript/C++ codec과 lazy bootstrap 생성

앱에서는 생성 명령만 import합니다.

```ts
import { addNumbers } from './generated/react-native';

const result = await addNumbers({ a: 20, b: 22 });
```

첫 호출이 JSI 설치, contract hash/schema version 검증, fast engine 설정을
concurrency-safe하게 한 번만 수행합니다. 별도의 `installRustraJSI()`,
`createFastEngine()`, `configure()` 호출은 필요하지 않습니다.

## Expo와 bare React Native

- bare RN: `bunx --bun react-native config`로 양 플랫폼 autolinking을 확인하고,
  iOS는 `cd ios && pod install` 후 앱을 다시 빌드합니다.
- Expo: development build 또는 `expo run:*`을 사용합니다. Expo Go는 JSI 네이티브
  코드를 로드할 수 없습니다.
- 두 환경 모두 Podfile, `settings.gradle`, `MainApplication`, CMake를 직접 수정하지
  않습니다.

`reactNative: {}`는 가장 가까운 `Cargo.toml`이 단일 staticlib crate를 가리킬 때
완전 자동입니다. monorepo workspace가 모호하면 app crate를 지정합니다.

```json
{
  "reactNative": {
    "rustManifest": "../native/Cargo.toml"
  }
}
```

그래도 여러 staticlib가 남을 때만 `rustPackage`가 필요합니다. `moduleDir`,
`rustLibrary`, `cppOutput`은 특별한 레이아웃에서만 override합니다.

## 충돌 방지

생성 모듈은 의도적으로 고정된 전용 이름을 사용합니다.

- JavaScript package: `@rustra/generated-react-native`
- React Native module: `RustraBridge`
- Android namespace: `dev.rustra.bridge`
- native shared library: `rustra_bridge`

생성기는 같은 dependency가 다른 위치를 가리키면 덮어쓰지 않고 중단합니다. 앱마다
로컬 생성 패키지를 하나만 두어 다른 Expo/Nitro/Turbo 모듈과 이름과 build target이
겹치지 않게 합니다.

## dev 핫코어 (dylib 핫스왑)

dev에서는 앱 재빌드·리마운트 없이 Rust 코어 dylib를 스왑할 수 있습니다. 공용 C++
브릿지의 모든 FFI 호출은 함수 포인터 테이블 경유라 스왑은 atomic 테이블 교체 한
번이고 JS 재바인딩은 없습니다.

- 발행: `rustra dev`의 `dev.target: "dylib"`가 cdylib를 빌드하고, parity 게이트를
  통과한 뒤에만 `<stem>-hot-live<ext>`를 원자적(tmp + rename)으로 발행합니다. 게이트가
  거절하면 기존 live 아티팩트가 그대로 남아 앱은 구 코어를 유지합니다(fail-closed).
- iOS(시뮬레이터만): `RUSTRA_HOT_CORE_DIR` 환경변수에 디렉터를 지정하면 install 때
  어댑터가 그 디렉터를 300ms 간격으로 폴링합니다. 이름 계약에 주의하세요 —
  `RUSTRA_HOT_CORE_DIR`(RN)은 디렉터이고, Tauri 쪽 `RUSTRA_HOT_CORE`는 파일 경로로
  서로 다른 변수입니다.
- Android: 폴링 디렉터는 `<filesDir>/rustra/hot`이며 생성된 JNI glue가 같은 폴링을
  켭니다.
- 전달: dylib를 제자리 덮어쓰지 마세요. 임시 파일로 push/copy한 뒤 rename으로
  제자리에 넣어야 합니다(CLI 발행도 그렇게 합니다). 프로세스가 매핑한 dylib를 같은
  경로로 덮어쓰면 프로세스가 죽습니다 — iOS/macOS는 SIGKILL(코드 서명 변조),
  Android는 SIGSEGV(수정된 파일 페이지 재로딩).

스왑마다 코어 내부 상태(채널, 이벤트 컨텍스트)는 버려지고 새 코어에 이벤트 싱크가
재등록됩니다. 구 코어는 의도적으로 unload하지 않습니다. dev에서
`getRustraNative().hotCoreStatus()`가 마지막 스왑의 구/신 계약 해시(또는 마지막 오류)를
돌려주고 기본 정적 빌드에서는 `null`입니다.

## 저수준 API

직접 transport를 제어해야 할 때만 `createReactNativeEngine`, `createFastEngine`,
`getRustraNative`를 사용합니다. JSON 경로는 Hermes의 `TextEncoder`/
`TextDecoder` 유무와 관계없이 exact `ArrayBuffer`로 동작하고, fast path는 generated
postcard codec과 caller-buffer FFI를 사용합니다.
