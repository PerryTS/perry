
### Fixed

- **`process.on("exit", …)` handlers now run.** They never ran at all: the
  generated event-loop epilogue emitted `beforeExit` and then went straight to
  cleanup, and nothing anywhere in the runtime ever emitted `exit`. The
  `process` EventEmitter accepted the registration, kept the listener alive and
  rooted it for the GC — the listener was simply never called, on any exit
  path, with no error and no diagnostic.

  This is silent data loss, not a cosmetic gap: `exit` is where a program does
  its last synchronous flush. claude-code registers **17** `exit` handlers —
  terminal-state restore, OpenTelemetry `forceFlush`, sandbox mount cleanup,
  graceful-fs queue drain — and every one of them was a no-op under Perry.

  Scope note, measured rather than assumed: the `claude --bare -p hi`
  transcript that motivated this (1 line under Perry, 5 under node) does **not**
  come from an `exit` handler and is **not** closed by this change. Snapshotting
  the file from a `prependListener("exit", …)` under node shows all five records
  already written before the first `exit` listener runs — they go through the
  session writer's async `insertQueueOperation` / `flush` path, while the one
  record Perry does write (`last-prompt`) is a direct `appendFileSync`. That
  remains an open, independent divergence.

  Node's exit sequence (`handleProcessExit`) is now one runtime function,
  `process::run_process_exit_sequence`, driven from every path that ends the
  process:

  - `crates/perry-codegen/src/codegen/entry.rs` — the natural-drain epilogue
    calls it after `beforeExit` and its microtask drain.
  - `crates/perry-runtime/src/process/env_misc.rs` — `process.exit()` runs it
    before terminating, and so does the fatal-path terminator
    `exit_after_current_thread_collection_teardown` (uncaught exception with an
    `uncaughtException` listener that rethrows, unhandled rejection).
  - `crates/perry-runtime/src/exception.rs` — an uncaught throw with no open
    `try` runs it *before* printing its report, which is the order node uses.
    The listeners are JS, so the fatal branch was lifted out of the
    `with_exception_state` access it used to run under.
  - `crates/perry-runtime/src/os/os_process_emitter.rs` — `js_process_emit_exit`
    does the emit itself, guarded to fire at most once. The guard is
    load-bearing: a listener may call `process.exit()` or throw, and node's
    answer to both is that the listeners *after* it never run.

  The sync-only half of the contract needed no suppression machinery, only the
  right splice point. Every caller terminates — or returns out of generated
  `main` — as soon as the emit returns, and nothing past it ticks the timer,
  `setImmediate` or `nextTick` queues, so a `writeFileSync` in a listener lands
  while a `setTimeout` scheduled beside it is simply never given a turn. The
  one piece of async work node *does* honour here is V8's microtask checkpoint
  after the emit returns to the top level, so the natural-drain arm ends with a
  promise-jobs-only drain: a `.then` queued by a listener runs, after every
  listener, and only on that path.

  Two smaller divergences in the same epilogue fell out of pinning it against
  the oracle:

  - `beforeExit` was emitted with a literal `0`. Node passes the code the
    process is about to leave with, so `process.exitCode = 5` made every
    `beforeExit` listener see the wrong number.
  - The status a listener sets is now honoured. Node re-reads
    `process.exitCode` after the listeners run, on every path: a handler
    assigning `9` turns a natural exit, a `process.exit(3)` and an uncaught
    throw all into status 9. Perry exited 5 where node exits 9.

  `process.exitCode` is published before the emit exactly where node publishes
  it — an explicit `process.exit(3)`, and the fatal paths, which force `1` even
  over an already-set code — and left alone on natural drain and a bare
  `process.exit()`, where a listener reading it must still see `undefined`.

  Validation: `test-files/test_gap_9403_process_exit_event*.ts` — three
  programs, one per process status (natural 0, explicit 3, listener-rewritten
  9) — byte-compared against node 26.5.1, covering handler order, the code
  argument and its arity, `once` / `prependListener` / `removeListener`,
  `beforeExit` firing first and being skipped on an explicit exit, a
  `writeFileSync` + read-back inside a handler, and `setTimeout` /
  `setImmediate` / `nextTick` / promise jobs. On unfixed `main` all three
  diverge — the natural-drain program prints 2 of node's 8 lines and the
  `exitCode` program exits 5 instead of 9.

  The fatal paths are pinned separately, against the same oracle: an uncaught
  throw and an unhandled rejection each run the handlers with code 1 and let a
  handler rewrite the status to 9; a handler that throws stops the ones after
  it and exits 1; a handler calling `process.exit(7)` stops the ones after it
  and exits 7. All five match node.

  Compiled claude-code 2.1.112 exits 1 on `--bare -p hi`, as node does. Before
  the companion optional-chain fix below it was SIGKILLed (137) — that is what
  making the handlers reachable exposed.

  `perry-runtime --lib` 2920 passed / 0 failed; `perry-codegen --lib` 1383 / 0;
  `perry-hir --lib` 371 / 0.

- **An optional call on a ternary receiver did not short-circuit.**
  `(c ? o : undefined)?.write(x)` returned the RECEIVER when `c` held, and threw
  `TypeError: Cannot read properties of undefined (reading 'write')` when it did
  not, where node returns `undefined` in one case and calls the method in the
  other.

  A separate, pre-existing defect, filed here because the fix above is what made
  it reachable: claude-code's very first `process.on("exit")` listener is
  exactly this shape —

  ```js
  (process.stderr.isTTY ? process.stderr
    : process.stdout.isTTY ? process.stdout : void 0)?.write(resetSequence)
  ```

  With both streams piped that value is `undefined`, so the listener threw, the
  throw escaped `process.exit()` (node propagates it to the caller too), and
  claude-code's `try { process.exit(q) } catch { process.kill(process.pid,
  "SIGKILL") }` fallback killed the process mid-shutdown. Status 137 instead of
  node's 1.

  `crates/perry-hir/src/lower/lower_expr/arm_optchain.rs` has a branch that
  destructures a receiver's lowered `Expr::Conditional` and reads its condition
  and then-branch as an optional chain's short-circuit test. It exists for
  `a?.b?.method(args)`, where the receiver really is a chain — but a ternary the
  user wrote lowers to the identical shape, and the branch claimed it. Same
  shape as #8090/#8109/#9403 above: a fast path claims the operation before the
  question that distinguishes the cases is asked. Lowered shape cannot answer
  "did a `?.` build this?", so the receiver's AST is now asked instead
  (transparently through parens and the erased TS wrappers).

  Validation: `test-files/test_gap_optional_call_conditional_receiver.ts`,
  byte-compared against node 26.5.1 — nullish tails in every spelling
  (`undefined` / `void 0` / `null`) and nesting depth, non-nullish tails that
  must still CALL rather than return the receiver, property-read and
  through-a-local controls, and the upstream-chain shapes the branch exists for
  (`a.b?.m()`, `a?.b?.m()`, `a?.b?.m?.()`). Fails on the parent commit. The
  standing optional-chain suite — #388, #4699 (both), #6719, #1111, #542,
  `test_optional_chain`, `test_optchain_builtin_method_call`,
  `test_parity_optional_chain_double_member_call` — is unchanged.
