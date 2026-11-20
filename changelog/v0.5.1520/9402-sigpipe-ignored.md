### Fixed

- **A truncating consumer no longer kills a compiled program.**
  `claude auto-mode defaults | head -2` exited **141** (128 + SIGPIPE) under
  Perry and 0 under node — deterministically, 3 runs out of 3. Every pipeline
  that stops reading early hit it: `| head`, `| grep -q`, `| less` followed by
  `q`, a client that closed its socket.

  The cause is structural rather than a mistake in any one function. A Perry
  program has its **own C `main`**, emitted by codegen, so it never runs Rust's
  `std::rt` startup — and that startup is where an ordinary Rust binary gets
  `SIGPIPE` set to `SIG_IGN`. A compiled program therefore inherited the
  signal's default disposition and died mid-write, with no JavaScript-visible
  event and nothing to catch. Node (through libuv) ignores the signal and lets
  the failing `write(2)` return `EPIPE` to the writer instead.

  - `crates/perry-runtime/src/os/signal.rs` — `ignore_sigpipe_at_startup()`
    installs `SIG_IGN`, once per process, and only over `SIG_DFL`, so an
    embedder's own disposition and a later `process.on('SIGPIPE', …)` are both
    left alone. Unix only: Windows has no `SIGPIPE`.
  - `crates/perry-runtime/src/gc/mod.rs` — called from `js_gc_init`, which is
    the first runtime call of every `main` / `perry_module_init`, so every
    compiled program gets it before a byte can be written.

  Ignoring the signal alone would have traded exit 141 for exit **134**:
  `std`'s `println!` turns the resulting `EPIPE` into a panic, and Perry builds
  with `panic = "abort"`. Node's console is specified never to throw
  (`node -e 'for(;;) console.log(1)' | head -2` exits 0), so:

  - `crates/perry-runtime/src/builtins/mod.rs` — the `console.*` family's
    `println!` / `print!` / `eprintln!` are shadowed with writers that drop the
    write error, which is exactly that contract. The shadowing is confined to
    the `builtins` tree, alongside the pre-existing harmonyos hilog override;
    diagnostics elsewhere in the runtime keep `std`'s macros.

  Validation: `test-files/test_gap_9402_sigpipe_truncating_consumer.ts`
  re-runs itself through `bash`, pipes 50 000 lines into `head -2`, and reports
  the **writer's** status. Byte-compared against node 26.5.1: node
  `writer-status=0`, Perry built from unfixed `origin/main` `writer-status=141`,
  Perry with this change `writer-status=0`.

  Known remaining gap, not addressed here: `process.stdout.write` **swallows**
  the `EPIPE` (`os_process_streams.rs` has always discarded the write result),
  where node emits an `'error'` event on the stream and exits 1 if it is
  unhandled. That is a stream-plumbing change, not a signal one.
