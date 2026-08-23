# Repository agent rules

## React quality gate

- After changing React, React Native, JSX/TSX, styles, or component behavior, run `npx -y react-doctor@latest . --verbose` from the repository root.
- Completion requires a React Doctor score of `100 / 100`. Fix every reported issue in source; do not hide findings with ignore rules, score overrides, or generated-file edits.
- Re-run the repository typecheck and relevant tests after React Doctor fixes. If the tool itself cannot run, report that as an unverified blocker instead of claiming the gate passed.
