import type { ExpoConfig } from "expo/config";

// Leftcar viewer — Expo dev build (네이티브 rustra 모듈 때문에 Expo Go 불가).
type LeftcarExpoConfig = ExpoConfig & {
  // Expo SDK 57 still consumes the native splash settings, but the
  // `expo/config` re-export omits this field from its public type.
  splash: {
    image: string;
    resizeMode: "cover" | "contain";
    backgroundColor: string;
  };
};

const config: LeftcarExpoConfig = {
  name: "Leftcar XR",
  slug: "leftcar-viewer",
  scheme: "leftcar",
  version: "0.1.0",
  orientation: "default",
  userInterfaceStyle: "automatic",
  icon: "./assets/branding/leftcar-xr-icon-source.png",
  splash: {
    image: "./assets/branding/leftcar-xr-icon-source.png",
    resizeMode: "contain",
    backgroundColor: "#080B1D",
  },
  android: {
    package: "dev.leftcar.viewer",
  },
  plugins: ["expo-router"],
  experiments: { typedRoutes: false },
};

export default config;
