import "../global.css";
import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useAppTheme } from "../src/theme";
import { LanguageProvider, useAppLanguage } from "../src/i18n";
import { initializeRustra } from "../src/rustra";

initializeRustra();

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: false,
    },
  },
});

function NavigationStack() {
  const { colors, isDark } = useAppTheme();
  const { t } = useAppLanguage();

  return (
    <>
      <StatusBar style={isDark ? "light" : "dark"} />
      <Stack
        screenOptions={{
          headerStyle: {
            backgroundColor: colors.bgSurface,
          },
          headerTintColor: colors.textPrimary,
          headerTitleStyle: {
            fontWeight: "700",
            fontSize: 15,
          },
          headerShadowVisible: false,
          contentStyle: {
            backgroundColor: colors.bgCanvas,
          },
        }}
      >
        <Stack.Screen
          name="index"
          options={{
            headerShown: false,
          }}
        />
        <Stack.Screen
          name="host"
          options={{
            title: t.viewer.navHost,
            headerBackTitle: t.common.back,
          }}
        />
        <Stack.Screen
          name="catalog"
          options={{
            title: t.viewer.navCatalog,
            headerBackTitle: t.common.back,
          }}
        />
        <Stack.Screen
          name="pairing"
          options={{
            title: t.viewer.navPairing,
            headerBackTitle: t.common.back,
          }}
        />
      </Stack>
    </>
  );
}
export default function RootLayout() {
  return (
    <QueryClientProvider client={queryClient}>
      <LanguageProvider>
        <NavigationStack />
      </LanguageProvider>
    </QueryClientProvider>
  );
}
