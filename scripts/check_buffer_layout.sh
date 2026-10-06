#!/usr/bin/env bash
# B1 ratchet: allow existing current-layout sites, refuse new ones. B5 removes
# the baseline. Header sizes and access implementations remain unchanged here.
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 scripts/check_buffer_layout.py "$@"
