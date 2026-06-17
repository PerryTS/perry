# perry-container-compose fuzz targets

Three libfuzzer targets cover the parser surface:

| Target | Catches |
|---|---|
| `compose_yaml_parse` | YAML parser panics, malformed-input handling, integer overflow in field types |
| `env_interpolation` | `${VAR}` / `${VAR:-default}` parser DoS — unbalanced braces, deep nesting |
| `compose_spec_json_round_trip` | parse-vs-serialise drift (silently-dropped fields, untagged-union ambiguity) |

## Running

```bash
# nightly required for libfuzzer-sys
rustup toolchain install nightly
cargo install cargo-fuzz

cd crates/perry-container-compose/fuzz

# Run a target indefinitely (Ctrl-C to stop)
cargo +nightly fuzz run compose_yaml_parse

# Time-bound run (for CI)
cargo +nightly fuzz run compose_yaml_parse -- -max_total_time=300

# Inspect a crash
cargo +nightly fuzz fmt compose_yaml_parse <crash-file>
```

## CI

The container CI workflow runs each target for 5 minutes nightly on
main. Crash artifacts are uploaded as workflow artifacts; if any
crash is found, the job fails and the artifact is ready to reproduce
locally.

## Adding new targets

1. Create `fuzz_targets/<name>.rs` with a `fuzz_target!(|data: &[u8]|
   { ... })` body
2. Register it in `fuzz/Cargo.toml` under `[[bin]]`
3. Add to the CI matrix in `.github/workflows/container-tests.yml`
