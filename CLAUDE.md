# Repository instructions

## React quality gate

- After changing React, React Native, JSX/TSX, styles, or component behavior, run `npx -y react-doctor@latest . --verbose` from the repository root.
- Work is complete only at `100 / 100`. Resolve all findings in the implementation; never suppress findings or tune the checker merely to raise the score.
- Re-run TypeScript checks and the relevant test suites after fixes. If React Doctor cannot execute, state the missing verification explicitly.
