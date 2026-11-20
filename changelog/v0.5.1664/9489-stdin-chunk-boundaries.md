### Fixed

- **`process.stdin` chunks by read-buffer boundary, not by line (#9489).** The
  fd-0 reader called `read(2)` **one byte at a time** and, in cooked flowing /
  pull mode, cut a `'data'` chunk at every `\n`. 1 MB of `"line\n"` therefore
  cost 1,048,576 read syscalls and produced **200,000 `'data'` events in
  2.10 s**, where Node delivers **16 in 0.04 s** — a 52x slowdown. Input with
  no newline in it arrived as a single chunk, which is what pinned the cause
  on newlines rather than on size.

  The reader now reads a 64 KiB block per syscall (Node's own pipe read size)
  and emits one chunk per read. Line splitting stays where it belongs — in the
  consumer: readline still slices `'line'` events out of the same byte stream,
  and the raw-mode keypress path still receives the single-byte chunks its
  escape-sequence reassembly depends on (those are now staged and enqueued
  with one mutex acquisition per read instead of one per byte). The mode
  atomics are still consulted per byte, so a `setRawMode` / `pause` flip
  arriving mid-block lands on exactly the byte it used to.

  User-visible consequence in claude-code: under the event flood its stdin
  consumer gave up partway, truncating a piped prompt to a **random** prefix
  (612k / 455k / 394k / 374k chars of 1,000,000 across four runs). Note the
  bytes were never lost by the engine — a gap fixture that counts them under
  the unfixed reader receives all 1,000,000 — so the truncation is the
  bundle's own back-pressure logic reacting to event *count*; removing the
  flood removes the trigger.

  Fixture `test-files/test_gap_9489_stdin_chunk_boundaries.ts` asserts total
  bytes, exact content and an order-of-magnitude event count (`< 100`) for
  200k-line, no-newline and 1k-line inputs, plus a three-spaced-writes control
  that must still produce **three** events — so gluing the whole stream into
  one chunk fails it.
