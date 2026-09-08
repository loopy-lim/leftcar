// ── rustra generated ────────────────────────────────────────
// File:   src/index.ts
// Source: schema.json (single source of truth for this file)
// Regen:  rustra codegen --config rustra.json
// Stage:  rust-probe schema → ts renderer
// DO NOT EDIT — changes will be overwritten and fail codegen --check.
// ────────────────────────────────────────────────────────────

import { NativeModules } from 'react-native';
import { getRustraNative as getInstalledNative } from '@rustra/react-native';

type Installer = { install(): Promise<boolean | void> };

function nativeInstaller(): Installer {
  const current = NativeModules.RustraBridge as Installer | undefined;
  if (!current) {
    throw new Error(
      '[rustra:autolink] RustraBridge was not linked. Run ' +
        '`bunx --bun react-native config` to inspect bare RN autolinking, then ' +
        '`cd ios && pod install` or rebuild Android. Expo Go cannot load JSI; Expo apps need a development build.',
    );
  }
  return current;
}

export async function installRustraJSI(): Promise<void> {
  await nativeInstaller().install();
  getInstalledNative();
}

export function getRustraNative(): ReturnType<typeof getInstalledNative> {
  return getInstalledNative();
}
