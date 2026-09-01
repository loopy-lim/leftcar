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
  name: "Leftcar Viewer",
  slug: "leftcar-viewer",
  scheme: "leftcar",
  version: "0.1.2",
  orientation: "default",
  userInterfaceStyle: "automatic",
  icon: "./assets/branding/leftcar-viewer-icon-source.png",
  splash: {
    image: "./assets/branding/leftcar-viewer-icon-foreground.png",
    resizeMode: "contain",
    backgroundColor: "#09090B",
  },
  android: {
    package: "leftcar.ll3.kr",
    adaptiveIcon: {
      foregroundImage: "./assets/branding/leftcar-viewer-icon-foreground.png",
      monochromeImage: "./assets/branding/leftcar-viewer-icon-monochrome.png",
      backgroundColor: "#09090B",
    },
  },
  plugins: [
    "expo-router",
    [
      "expo-camera",
      {
        cameraPermission: "컴퓨터의 연결 QR 코드를 스캔할 때만 카메라를 사용합니다.",
        recordAudioAndroid: false,
        barcodeScannerEnabled: true,
      },
    ],
  ],
  experiments: { typedRoutes: false },
};

export default config;
