import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { colors } from "../src/theme";
import { initializeRustra } from "../src/rustra";

initializeRustra();

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: false,
    },
  },
});

export default function RootLayout() {
  return (
    <QueryClientProvider client={queryClient}>
      <StatusBar style="dark" />
      <Stack
        screenOptions={{
          headerStyle: {
            backgroundColor: colors.light.bgSurface,
          },
          headerTintColor: colors.light.textPrimary,
          headerTitleStyle: {
            fontWeight: "700",
            fontSize: 15,
          },
          headerShadowVisible: false,
          contentStyle: {
            backgroundColor: colors.light.bgCanvas,
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
            title: "컴퓨터 연결",
            headerBackTitle: "뒤로",
          }}
        />
        <Stack.Screen
          name="catalog"
          options={{
            title: "화면 선택",
            headerBackTitle: "뒤로",
          }}
        />
        <Stack.Screen
          name="pairing"
          options={{
            title: "연결 승인",
            headerBackTitle: "뒤로",
          }}
        />
      </Stack>
    </QueryClientProvider>
  );
}
