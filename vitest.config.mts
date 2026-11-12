import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    exclude: [
      ...configDefaults.exclude,
      // Preserve the Vitest 2 inventory when updating the runner to Vitest 4.
      "**/dist/**",
      "**/cypress/**",
      "**/.{idea,git,cache,output,temp}/**",
      "**/{karma,rollup,webpack,vite,vitest,jest,ava,babel,nyc,cypress,tsup,build,eslint,prettier}.config.*",
      "**/.worktrees/**",
      // CI checks the unikit-bridge sibling repo out into the workspace so
      // Cargo path deps resolve; its own test suite is not ours to run.
      "**/unikit-bridge-sibling/**",
    ],
  },
});
