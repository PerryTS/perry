#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo test -p perry-runtime --release gc_mutable_root_contract -- --nocapture
cargo test -p perry-codegen --test shadow_slot_hygiene
python3 scripts/gc_ffi_root_sources_gate.py
python3 -m unittest tests/test_gc_1090_evidence_report.py
scripts/run_memory_stability_tests.sh
