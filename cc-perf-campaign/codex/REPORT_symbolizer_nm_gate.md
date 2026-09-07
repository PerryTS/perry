# Frame symbolizer `nm` gate report

Implementation SHA: `de719a1f01223e1674ce48097e90ceb0625e5ac4`

The default static-symbol miss now reads one cached `PERRY_STACK_SYMBOLS`
boolean, compares the frame's `dladdr` base with one cached executable-image
base, and formats one `rt+0x...` string. It does not call `current_exe`, build
the static index, or spawn `nm`. Frames outside the executable image retain the
raw `0x...` form. With the flag enabled, the existing `nm -nC` index and
demangled `rt:{name}+0x{off}` output remain in place.

Verification:

- `rustfmt --check --edition 2021 crates/perry-runtime/src/error_stack_frames.rs`: pass.
- `git diff --check`: pass.
- `cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1 error_stack symbol`: not run: disk (7 GB free; required minimum is 12 GB).
- `cargo build --release -p perry-runtime --features wasm-host -j4`: not run: disk (7 GB free; required minimum is 12 GB).
- Default-path and opt-in `nm` tests: written as isolated child-process witnesses, but not run: disk. The default witness asserts `rt+0x...` and a test-only spawn counter of zero; removing the gate makes that counter non-zero. Sabotage execution: not run: disk.
- Runtime env-knob inventory: skipped because `scripts/check_gc_env_knobs.py` and `gc/tests/env_knob_parse.rs` inventory only GC-family knobs (plus an explicit GC/runtime allowlist), not general runtime flags.

Gate: PERRY_STACK_SYMBOLS
