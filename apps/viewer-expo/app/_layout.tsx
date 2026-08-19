import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

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
      <StatusBar style="light" />
      <Stack
        screenOptions={{
          headerStyle: {
            backgroundColor: "#0F172A",
          },
          headerTintColor: "#F8FAFC",
          headerTitleStyle: {
            fontWeight: "700",
            fontSize: 17,
          },
          headerShadowVisible: false,
          contentStyle: {
            backgroundColor: "#080B11",
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
            title: "호스트 연결",
            headerBackTitle: "뒤로",
          }}
        />
        <Stack.Screen
          name="catalog"
          options={{
            title: "소스 카탈로그",
            headerBackTitle: "뒤로",
          }}
        />
      </Stack>
    </QueryClientProvider>
  );
}
