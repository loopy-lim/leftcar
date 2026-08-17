import type { ExpoConfig } from "expo/config";

// Leftcar viewer — Expo dev build (네이티브 rustra 모듈 때문에 Expo Go 불가).
const config: ExpoConfig = {
  name: "Leftcar",
  slug: "leftcar-viewer",
  scheme: "leftcar",
  version: "0.1.0",
  orientation: "default",
  userInterfaceStyle: "automatic",
  android: {
    package: "dev.leftcar.viewer",
  },
  plugins: ["expo-router"],
  experiments: { typedRoutes: false },
};

export default config;
