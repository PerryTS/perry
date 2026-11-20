### Fixed

- **`perry-runtime` failed to compile for `*-pc-windows-msvc`.**
  `gc::native_stack_scan::resolve_symbol` reached `libc::dladdr` and
  `libc::Dl_info` unconditionally. Both are POSIX-only, so the crate did not
  build on Windows at all — which is why the `native-roots-rs4gc
  (windows-latest, x86-64, PE)` arm of `gc-native-roots` failed in its **build**
  step rather than in a probe, taking `gc-native-roots-complete` red with it.

  The two `pthread_get_stack*_np` calls in the same file were already inside a
  `#[cfg(target_os = "macos")]` block; `resolve_symbol` was the only unguarded
  POSIX use. It is now `#[cfg(unix)]`, with a `#[cfg(not(unix))]` arm that
  reports the bare address. The scan is a debug-only diagnostic
  (`PERRY_GC_SCAN_NATIVE_STACK=1`), so the non-POSIX arm degrades symbolication
  instead of pulling in a platform symbolizer.

  `resolve_symbol`'s callers live in the ungated `run_native_stack_scan`, so the
  new arm is reachable on Windows and does not trip `dead_code` under
  `-D warnings` — the failure mode of #8306, where a fix applied to only one
  host's `cfg` arm could not be seen by the macOS lint run.
