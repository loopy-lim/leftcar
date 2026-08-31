import { useColorScheme } from "react-native";
import { colors, type ThemeMode, type ThemeTokens } from "@leftcar/ui-tokens";

/**
 * Leftcar Viewer - Theme access & hooks
 */
export { colors, type ThemeMode, type ThemeTokens };

export function useAppTheme(): {
  mode: ThemeMode;
  colors: ThemeTokens;
  isDark: boolean;
} {
  const systemScheme = useColorScheme();
  const mode: ThemeMode = systemScheme === "dark" ? "dark" : "light";
  return {
    mode,
    colors: colors[mode],
    isDark: mode === "dark",
  };
}
