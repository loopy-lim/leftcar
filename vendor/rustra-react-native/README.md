English | [한국어](./README.ko.md)

# @rustra/react-native

Rustra's React Native JSI adapter and shared native sources for generated modules. Expo
and bare React Native use the same generated entry and the standard autolinking path.

## Recommended usage

The Rust crate exposes a static library and a mobile entry.

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

Enable React Native generation in the app's `rustra.json`.

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

The generator handles the following automatically:

- Generates an app-specific `@rustra/generated-react-native` package under
  `modules/rustra-bridge`
- Links a Bun workspace dependency into the app `package.json` after checking for
  conflicts
- Generates the iOS Podspec, Android Gradle/CMake/JNI, and the shared C++ JSI bridge
- Infers the package and static library names from Cargo metadata
- Generates the TypeScript/C++ codec and lazy bootstrap

The app only imports generated commands.

```ts
import { addNumbers } from './generated/react-native';

const result = await addNumbers({ a: 20, b: 22 });
```

The first call performs JSI installation, contract hash/schema version verification, and
fast engine setup exactly once, concurrency-safe. Separate `installRustraJSI()`,
`createFastEngine()`, and `configure()` calls are not needed.

## Expo and bare React Native

- bare RN: verify autolinking on both platforms with `bunx --bun react-native config`,
  and on iOS run `cd ios && pod install` before rebuilding the app.
- Expo: use a development build or `expo run:*`. Expo Go cannot load JSI native code.
- Neither environment requires editing the Podfile, `settings.gradle`,
  `MainApplication`, or CMake by hand.

`reactNative: {}` is fully automatic when the nearest `Cargo.toml` points at a single
staticlib crate. If the monorepo workspace is ambiguous, specify the app crate.

```json
{
  "reactNative": {
    "rustManifest": "../native/Cargo.toml"
  }
}
```

Only when multiple staticlibs remain even then is `rustPackage` needed. Override
`moduleDir`, `rustLibrary`, and `cppOutput` only for unusual layouts.

## Collision avoidance

Generated modules deliberately use fixed, dedicated names.

- JavaScript package: `@rustra/generated-react-native`
- React Native module: `RustraBridge`
- Android namespace: `dev.rustra.bridge`
- native shared library: `rustra_bridge`

If the same dependency would point at a different location, the generator stops instead
of overwriting. Keep exactly one locally generated package per app so that names and
build targets do not collide with other Expo/Nitro/Turbo modules.

## Low-level API

Use `createReactNativeEngine`, `createFastEngine`, and `getRustraNative` only when you
need direct control over the transport. The JSON path works with an exact `ArrayBuffer`
regardless of whether Hermes provides `TextEncoder`/`TextDecoder`, and the fast path uses
the generated postcard codec and caller-buffer FFI.
