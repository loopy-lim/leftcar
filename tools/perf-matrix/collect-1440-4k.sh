#!/bin/sh
set -eu
# Existing entry point, now with mandatory run context and explicit boundaries.
# No logcat clear and no implicit device/process selection.
exec bun "$(dirname "$0")/collect.mjs" "$@"
