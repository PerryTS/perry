#!/usr/bin/env bash
# B5 hard gate: no byte-layout dependencies outside store and perry-abi.
set -euo pipefail
cd "$(dirname "$0")/.."
exec python3 scripts/check_buffer_layout.py "$@"
