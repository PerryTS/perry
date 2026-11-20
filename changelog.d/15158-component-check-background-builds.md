`scripts/component_check.sh` stops background builds that a failing step left
running and prints their logs. Its parallel instrumented build reuses
`scripts/build_instr_runtime.sh` through the new opt-in
`PERRY_INSTR_BUILD_ONLY=1`, which stops after a `--locked` Cargo build.
