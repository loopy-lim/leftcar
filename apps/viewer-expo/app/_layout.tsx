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
      <StatusBar style="dark" />
      <Stack
        screenOptions={{
          headerStyle: {
            backgroundColor: "#FFFFFF",
          },
          headerTintColor: "#0F172A",
          headerTitleStyle: {
            fontWeight: "600",
            fontSize: 16,
          },
          headerShadowVisible: false,
          contentStyle: {
            backgroundColor: "#F8FAFC",
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
            title: "화면 선택",
            headerBackTitle: "뒤로",
          }}
        />
        <Stack.Screen
          name="pairing"
          options={{
            title: "기기 페어링",
            headerBackTitle: "뒤로",
          }}
        />
      </Stack>
    </QueryClientProvider>
  );
}
