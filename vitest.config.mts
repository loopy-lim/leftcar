import { defineConfig } from 'vitest/config';

// `.worktrees/` holds gitignored frozen snapshots from past verification
// sessions; their tests pin old versions and reference files that only exist
// inside the snapshot, so the main suite must not collect them.
export default defineConfig({
  test: {
    exclude: [
      '**/node_modules/**',
      '**/dist/**',
      '**/cypress/**',
      '**/.{idea,git,cache,output,temp}/**',
      '**/.worktrees/**',
    ],
  },
});
