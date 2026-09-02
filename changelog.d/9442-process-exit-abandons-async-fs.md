### Fixed

- **`process.exit()` no longer completes async fs work that Node abandons.**
  Five fire-and-forget `fs.promises.appendFile` calls followed by
  `process.exit(0)` in the same tick landed five records where Node lands zero.
  This is a divergence in the opposite direction from the usual truncation
  report: perry wrote data the program had deliberately walked away from, so an
  error path that exits on purpose could leave partially-written or duplicate
  state committed behind it.

  The scope question the issue asks has a clear answer, and it is not the
  expected one: **perry's exit path drains nothing.** `js_process_exit` runs the
  exit sequence and calls `libc::_exit`; a pending `setTimeout`, a queued
  `net.Socket` write and a pending microtask are all dropped there exactly as
  Node drops them. What diverged is upstream of the exit: perry's *async* fs
  write entry points did the write SYNCHRONOUSLY inside the call and returned an
  already-settled promise (or a deferred callback over an already-committed
  write). There was no in-flight work left to abandon, so `process.exit()`
  "worked" for the wrong reason.

  The four entry points now park the operation on a zero-delay callback timer —
  the mechanism `fs::stream`'s `schedule_drain` already uses — instead of
  performing it inline. That buys three properties at once, none of them newly
  invented: the timer queue GC-roots the closure and its captured path / data /
  options / sink values; a pending callback timer is a live event source, so a
  program that ends by draining its loop still lands every record and an
  `await` on the returned promise still resolves; and `process.exit()`
  terminates through `libc::_exit` without ticking the timer queues, so the
  parked write is dropped the way Node drops it.

  Argument validation that throws still runs on the calling turn, so a bad path
  or a bad options object raises where it did before rather than turning into
  an uncaught exception inside a timer.

  Affected files:

  - `crates/perry-runtime/src/fs/deferred.rs` (new) — the parked operation and
    its four schedulers.
  - `crates/perry-runtime/src/fs/mod.rs` — module registration.
  - `crates/perry-runtime/src/fs/callbacks.rs` — `fs.writeFile(…, cb)` and
    `fs.appendFile(…, cb)` park instead of writing inline.
  - `crates/perry-runtime/src/node_submodules/fs_promises.rs` —
    `fs.promises.writeFile` / `fs.promises.appendFile` return a PENDING promise.

  Validation: `test-files/test_gap_9442_process_exit_abandons_async_fs.ts` and
  its CommonJS half `…_cjs.cts`, byte-compared against node 26.5.1. Eight roles:
  the reported promise repro, the callback form, `writeFile`, a pending timer,
  and four controls that keep a fix from over-abandoning — a synchronous write,
  an *awaited* write that genuinely completed before the exit, a program that
  ends naturally with no `process.exit()` at all (which must still land all five
  records, and is also what pins the #9441 idle-exit fix), and a
  `process.on("exit")` listener whose own synchronous write must still land.
  On unfixed `origin/main` the first three roles report 5, 5 and 1 records
  against Node's 0, 0 and 0, and the `exit-listener` role reports 4 against
  Node's 1; all four controls already matched and still match.

  The `exit-listener` role also answers the question the issue raises about the
  neighbouring known divergence: the `exit` event and the abandoned work do not
  share a root. The listener already fires on an explicit exit (#9403) and its
  synchronous write lands in both engines; only the pending async writes differ.
