**A failed `appendFile` is now reported instead of being reported as a
success** — `fs/promises.appendFile` rejects, `fs.appendFileSync` throws, and
`fs.appendFile(path, data, cb)` calls back with the error, all carrying Node's
`code` / `errno` / `syscall` / `path`.

```js
await appendFile("/no/such/dir/f.txt", "x");  // was: RESOLVED   now: rejects ENOENT
appendFileSync("/no/such/dir/f.txt", "x");    // was: no throw   now: throws ENOENT
```

All three surfaces called `js_fs_append_file_sync_options`, which reports
failure by *returning `0`* rather than by throwing, and all three dropped that
status on the floor (`let _ = …`). Every append that failed — missing parent
directory, `EACCES`, `EISDIR`, a closed fd — looked like it had worked. The
op now goes through `js_fs_append_file_result`, which returns a Node-shaped fs
error value the way `write_file_path_or_fd_result` already did for
`writeFile`, so each surface renders it in its own idiom.

**This is #9421's missing transcript records.** `claude --bare -p hi` wrote 1
record where Node wrote 5, deterministically. Nothing was wrong with the
session writer or its flush timer — the writer's own recovery arm is

```js
try { await appendFile(p, chunk, { mode: 0o600 }) }
catch { await mkdir(dirname(p), { recursive: true, mode: 0o700 })
        await appendFile(p, chunk, { mode: 0o600 }) }
```

and it is *the only thing that ever creates* `~/.claude/projects/<slug>/`.
Under Perry the first append resolved, the `catch` never ran, the directory
was never created, and every queued record was discarded — with no error at
any layer, because the one function that knew about the failure had returned
it as a number nobody read. The single surviving record, `last-prompt`, is
written through `openSync(path, "ax", mode)` + `appendFileSync(fd, …)`;
`openSync` throws correctly, so that path took its recovery branch and made
the directory a few hundred microseconds *after* the drain had already given
up.

**Why only `--bare`.** Without it, auto-memory materialises
`~/.claude/projects/<sanitized-cwd>/memory/` about 47 ms before the first
transcript flush, which creates the transcript directory as a side effect; the
first append then succeeds and all 7 records land. `--bare` skips auto-memory
(along with hooks, LSP, plugin sync, attribution, background prefetches and
`CLAUDE.md` discovery), so nothing else made the directory and the bug became
reachable. The flag did not change the write path — it removed the accident
that was hiding it.

`test_gap_9421_append_file_promise_reject` covers the three surfaces and
replays the session-writer shape; on unfixed `main` it prints `resolved`,
`no-throw` and `writer-file: MISSING` where Node prints `ENOENT` and the two
records.

**Found while fixing:** `fs.writeFileSync` swallowed its errors the same way,
and the `mode` option was dropped on every create path — so the transcript
this issue is about was also landing `0644` instead of the `0600` the writer
asks for. Both are fixed in this PR; see
`changelog.d/9421-write-file-sync-throws.md`.

**Found while fixing, NOT fixed here** (separate lanes, filed on their own):
`process.stdin` delivers one `data` event per *line* rather than in 64 KiB
chunks — 1 MB of short lines is 200,000 events and 2.10 s against Node's 16
events and 0.04 s, and claude-code loses a non-deterministic tail of a piped
prompt as a result (394 KB–612 KB of 1 MB across four runs). And
`stream.setEncoding("utf8")` does not decode: fed bytes `0x00..0xFF` it yields
158 code units with raw `U+0080..U+00FF` where Node yields 256 with 128
`U+FFFD`, which puts invalid UTF-8 into the transcript and makes those lines
unparseable JSON. Neither is touched by this PR.
