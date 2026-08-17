import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["packages/control-generated/**/*.test.ts"],
  },
});
