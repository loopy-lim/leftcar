English | [한국어](./README.ko.md)

# @rustra/types

The core types package of rustra-bridge. Provides the `EngineClient` interface shared by
all platform adapters (Node, Bun, Tauri, React Native), error types, the rkyv V2 codec,
and Tauri-like global invoke.

## Public API overview

```ts
// Configure the platform engine once
import { configure } from '@rustra/types';
import { createRkyvV2Engine } from '@rustra/react-native';
configure(createRkyvV2Engine(native, registry));

// Type-safe invocations from anywhere (used inside generated clients)
import { addNumbers } from './generated/commands.js';
const result = await addNumbers({ a: 42, b: 58 });
```

Key exports:

- `EngineClient` — the common interface: `invoke<T>()` (+ optional `invokeBatch`)
- `configure()` / `invoke()` — global invoke (Tauri-like single entry point)
- `InvokeOptions.signal` — AbortSignal — on abort, the promise rejects immediately and the
  cancellation propagates to the native side (when `invokeAsync`/`invokeCancel` are
  exposed), with error code `cancelled`
- `RustraCommandError` — serializable error + `parseRustraErrorString`
- rkyv V2 codec — pure-JS encoder/decoder for the Rust `invoke_rkyv_v2` round trip
- `contractHash` verification — checks that the build-time contract matches the runtime
  contract
- `RkyvV2EngineOptions` — engine options: `onContractMismatch` (opt-in degraded mode on
  hash mismatch), `schemaVersion`/`onSchemaStale` (warning when JS is staler than native),
  `maxPayloadBytes` (size pre-check of the payload right after encoding)

## Related docs

- [rustra-bridge](https://github.com/loopy-lim/rustra#readme)
- `docs/architecture.md`, `docs/compatibility-contract.md`
