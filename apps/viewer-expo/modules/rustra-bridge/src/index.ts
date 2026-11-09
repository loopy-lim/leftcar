// ── unikit generated ────────────────────────────────────────
// File:   src/index.ts
// Source: schema.json (single source of truth for this file)
// Regen:  unikit codegen --config unikit.json
// Stage:  rust-probe schema → ts renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────

import { NativeModules } from 'react-native';
import { getUnikitNative as getInstalledNative } from '@unikit-bridge/react-native';

type Installer = { install(): boolean | Promise<boolean | void> };

function nativeInstaller(): Installer {
  const current = NativeModules.UnikitBridge as Installer | undefined;
  if (!current) {
    throw new Error(
      '[unikit:autolink] UnikitBridge was not linked. Run ' +
        '`npx react-native config` (or `bunx --bun react-native config`) to inspect bare RN ' +
        'autolinking, then `cd ios && pod install` or rebuild Android. Expo Go cannot load JSI; ' +
        'Expo apps need a development build.',
    );
  }
  return current;
}

export async function installUnikitJSI(): Promise<void> {
  const installed = await nativeInstaller().install();
  if (installed === false) {
    throw new Error(
      '[unikit:install] The native JSI installer reported failure. Rebuild the native app and retry.',
    );
  }
  getInstalledNative();
}

export function getUnikitNative(): ReturnType<typeof getInstalledNative> {
  return getInstalledNative();
}
