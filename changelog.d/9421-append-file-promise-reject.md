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

**Found while fixing, NOT fixed here:** the `mode` option is ignored on both
append paths. `appendFile(p, data, { mode: 0o600 })` and the `appendFileSync`
form open `0666 & ~umask`, so a freshly created file lands `0644` where Node
lands `0600` — the claude-code transcript is world-readable under Perry.
`writeFile` and `mkdir` honour their `mode`; only append does not.
