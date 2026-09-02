### Fixed

- **`fs.createWriteStream` and `child.stdin.write()` no longer commit bytes
  that `process.exit()` abandons in Node; `console.clear()` survives an
  explicit exit.** Follow-up to #9442 (PR #9492), which fixed the four
  `fs.{promises.,}{writeFile,appendFile}` entry points; the same divergence
  class remained in three places, each measured against Node 26.5.1.

  **`fs.createWriteStream`** opened the fd at construction and every
  `write()` did `seek`+`write_all` inline, so `ws.write("a\nb\nc\n");
  process.exit(0)` left a 6-byte file where Node leaves *no file*. Node opens
  on a later turn (`_construct` → `fs.open` on the thread pool) and each
  write is a pool request. The stream now runs on successive event-loop
  turns — the #9442 mechanism (a zero-delay callback timer, GC-rooted, a live
  event source, never ticked by `_exit`): the open (then `'open'`, `'ready'`),
  the queued writes (then `'drain'` when a `write()` returned `false`, the
  completed writes' callbacks, the `end()` callback and `'finish'`), then
  the close (`'close'`). `write()` answers the back-pressure question from
  the queued length as Node does; `fd` stays `null` and `pending` true until
  the open; an open failure delivers a node-shaped error (`.code`,
  `.syscall`, `.path`) to every pending callback, then `'error'`, then
  `'close'`. Path validation still throws on the calling turn. A supplied fd
  skips the open and emits no `'open'`/`'ready'` — the previous synchronous
  replay of those events on `.on()` is now confined to read streams, which
  still open eagerly.

  **`child.stdin.write()`** did a blocking `write_all` into the pipe: it
  parked the main thread on a full pipe until the child read (a language
  server that stops reading hung the program), always returned `true`, never
  emitted `'drain'`, and at `process.exit()` committed every byte where Node
  commits only what the pipe accepts synchronously. It is now libuv's
  try-write: the bytes the pipe takes right now land on the calling turn
  (so a handshake-sized write — what the MCP stdio client sends — is committed
  exactly as before, exit or not); the remainder goes to a per-child drain
  thread and is reported back through the reactor's event queue, which fires
  `'drain'` and then the queued callbacks in Node's order and performs the
  close an `end()` left pending, so a child never sees EOF ahead of data it
  was sent. The return value is `writableLength < writableHighWaterMark`
  (64 KiB, Node's default for the stdin socket); `writableLength`,
  `writableHighWaterMark` and `writableNeedDrain` are maintained on the
  stream. Windows keeps the blocking write (no `O_NONBLOCK` on pipe handles).

  **`console.clear()`** wrote its fragment into Rust's line-buffered stdout;
  with no newline behind it, `process.exit()` — which terminates through
  `_exit` — swallowed it. It now writes Node's exact bytes
  (`cursorTo(0, 0)` + `clearScreenDown()`, `\x1b[1;1H\x1b[0J`, only when
  stdout is a TTY and `TERM` is not `dumb`) and flushes, as Node's synchronous
  TTY write does.

  **`fs.Utf8Stream` at exit — measured negative, no change.** The issue
  suspected buffered data was lost on explicit exit "that node does not
  have". Node 26.5.1 has no exit flush either: a chunk held back by
  `minLength` is lost on an explicit exit *and* on a natural one; only
  `end()`, `flush()`/`flushSync()`, or crossing `minLength` commits it.
  Perry already matched. The fixture pins the oracle so a later "flush on
  exit" cannot land as a parity improvement.

  Affected files:

  - `crates/perry-runtime/src/fs/stream.rs` — the pending-write queue and
    the open/drain/close turns; `fs/fd_ops.rs` — a string-flag open helper.
  - `crates/perry-runtime/src/child_process/{reactor,emitter,builder}.rs` —
    `CpStdin` (try-write, drain thread, `StdinWritten` completion event).
  - `crates/perry-runtime/src/builtins/console.rs` — `js_console_clear`.

  Validation, byte-compared against node 26.5.1, each failing on the
  unfixed tree (`fix/exit-lifecycle`, the #9492 tip) and passing after:

  - `test-files/test_gap_9493_write_stream_process_exit.ts` — exit in the
    same tick (no file), exit from `'open'` (file, no bytes), a supplied fd,
    natural drain, exit from `'finish'`, a sync control, and three printing
    roles pinning event order, the supplied-fd shape, and the open-error
    cascade.
  - `test-files/test_gap_9493_child_stdin_backpressure.ts` — small write +
    exit (lands), 4 MiB write + exit (pipe-capacity prefix only), the same
    with `end()`, natural drain (all of it), the `write() === false` →
    `'drain'` → callbacks sequence, and the small-write callback control.
  - `test-files/test_gap_9493_console_clear_tty.ts` — under a pty (python
    `pty.spawn`; `script(1)` needs a tty on its own stdin): exit right after
    `clear`, a preceding `console.log`, natural exit, `TERM=dumb`, and piped
    stdout.
  - `test-files/test_gap_9493_utf8_stream_exit_no_flush.ts` — the measured
    negative, with `end()`, `flushSync()`-in-`'exit'` and full-buffer controls.
