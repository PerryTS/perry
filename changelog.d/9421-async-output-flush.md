### Tests

- `test_gap_9421_async_output_flush` pins the async queue-and-flush write path
  #9421 attributes the truncated claude-code transcript to (#9421). It drives
  multi-line output from async callbacks, a `process.stdout.write` loop,
  interleaved `console.log`/`console.error`, output followed by an explicit
  `process.exit()`, output past one pipe buffer, and a transliteration of
  claude-code's own `SessionWriter` (`scheduleDrain` → `setTimeout(100)` →
  `await drainWriteQueue()` → `await appendFile`, next to the one
  `appendFileSync` record the report says is the only survivor). Perry matches
  Node byte for byte in every one, including on unfixed `main` — so the
  async-flush attribution is wrong. The `writer-exit-early` role reproduces the
  reported 1-vs-5 signature exactly, **under both engines**, by leaving before
  the 100 ms drain timer: the symptom identifies a run that ended too early,
  not a flush that failed.
