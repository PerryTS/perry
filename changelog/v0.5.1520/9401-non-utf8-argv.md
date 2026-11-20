### Fixed

- **A non-UTF-8 byte in `argv` no longer aborts the process.**
  `claude -p $'\xff\xfe\x80abc\xc3\x28'` died with **SIGABRT** and a raw Rust
  backtrace —

  ```
  panicked at library/std/src/env.rs:878:51:
  called `Result::unwrap()` on an `Err` value: "\xFF\xFE\x80abc\xC3("
  ```

  — where node prints the program's own output. `std::env::args()` panics on an
  argument that is not valid Unicode, and non-UTF-8 filenames are ordinary on
  Linux, so this was trivially reachable by anything that passes a path
  through.

  Node decodes `argv` leniently: every invalid byte becomes U+FFFD. Verified
  against node 26.5.1 — `$'\xff\xfe\x80abc\xc3\x28'` arrives as the eight code
  points `fffd fffd fffd 61 62 63 fffd 28`, which is byte-for-byte
  `String::from_utf8_lossy`.

  - `crates/perry-runtime/src/process.rs` — one `process_args_lossy()` over
    `std::env::args_os()`, so a single bad byte cannot resurrect the abort in
    a path nobody thought to check.

  Every `std::env::args()` reader in the runtime now goes through it. There
  were **nine**, all reachable, and the panic was not confined to
  `process.argv`:

  - `os.rs` `js_process_argv` — `process.argv`;
  - `node_submodules/trace_events.rs` — reads `argv` from **`js_gc_init`**, so
    the process died before a line of JavaScript ran, whatever the program did;
  - `process/permission.rs` (×3) — the permission-model flag scan;
  - `process/report.rs` (×2) — `process.report`;
  - `process/attributes.rs` — `process.title`;
  - `cluster.rs` (×2) — `cluster` exec-path defaulting;
  - `child_process/options.rs` — self-launch detection in `spawn`;
  - `process.rs` `process_argv0_string` — `process.argv0` / `execPath`.

  Three more outside the runtime, same shape, same fix:

  - `crates/perry-stdlib/src/commander.rs` and
    `crates/perry-ext-commander/src/lib.rs` — `program.parse()` with no
    explicit argv;
  - `crates/perry/src/main.rs` and `crates/perry/src/update_policy.rs` — the
    compiler CLI's own arguments, so `perry compile` on a non-UTF-8 path
    reports a diagnostic instead of a backtrace.

  Not touched (UI crates, out of this change's scope): `perry-ui-gtk4`
  `src/tray.rs`, `perry-ui-macos` `src/app.rs`, `perry-ui` `src/bin/styling-matrix.rs`.

  `std::env::var()` needs no equivalent change: it *returns* `Err` for a
  non-Unicode value rather than panicking, and the runtime has no
  `env::var(..).unwrap()`.

  Validation: `test-files/test_gap_9401_non_utf8_argv.ts` re-runs itself
  through `sh` (which is byte-oriented, so it can build an argument the source
  file cannot contain) and prints the decoded length, code points and UTF-8
  bytes. Byte-compared against node 26.5.1; Perry built from unfixed
  `origin/main` reports `child-status: null / child-signal: SIGABRT`, and with
  this change is identical to node.
