# Capture backend A/B

Use the same display, resolution, frame rate, and motion sequence for both
backends. `screenCaptureKit` is the supported default. `cgDisplayStream` is an
explicit display-only compatibility path because Apple obsoleted its public API
in the macOS 15 SDK.

For each backend, collect the final `getStatus` response from ten fresh stream
starts as newline-delimited JSON. Include one sleep/wake restart among the ten.
The session is successful only when `state=running`, `frames>0`, and `error` is
empty. Static screens may legitimately report `fps=0` after the first frame.

Run:

```sh
pnpm benchmark:capture -- artifacts/sck.jsonl artifacts/cg.jsonl
```

The report compares first-frame delivery, capture callback interval p95,
capture-to-encode p95, UDP send p95, drops, and the ten-connection gate. Optional
`cpuPercent` and `gpuPercent` fields can be added to samples collected with the
macOS performance tools; absent fields are reported as zero rather than guessed.
