**`fs.writeFileSync` now throws when the write fails, and `mode` is honoured
on every write and append that creates a file.** Same defect class as the
`appendFile` swallow in this PR, found while fixing it.

```js
writeFileSync("/no/such/dir/f", "x")      // was: returned  now: throws ENOENT
writeFileSync(p, 42)                      // was: wrote     now: ERR_INVALID_ARG_TYPE
writeFileSync(p, "414243", "hex")         // was: wrote "414243"  now: writes "ABC"
writeFileSync(p, "x", { mode: 0o600 })    // was: 0644     now: 0600
```

`writeFileSync` carried a **private copy** of the write op inside
`js_fs_write_file_sync_options` that reported failure by returning `0`, and
every caller — the codegen lowering, the namespace/computed dispatch entry —
discarded it. The promise and callback forms were never affected: both already
ran `write_file_path_or_fd_result`, which returns a Node-shaped fs error.
The sync form now runs that same core, so the three agree.

**The private copy diverged in more than error reporting.** It read its payload
with `bytes_from_value` rather than `consume_write_file_input`, which is where
`writeFile`'s argument validation and its `encoding` option live. So
`writeFileSync` also accepted values Node rejects (`42`, `{}`, `null` — all
`ERR_INVALID_ARG_TYPE`) and ignored `encoding` entirely, writing the six
characters `414243` where Node writes the three bytes `ABC`. Deleting the copy
fixes all three at once; that is the argument for routing through the shared
core rather than bolting a status check onto the duplicate.

**`mode` was dropped on the create path.** `open_file_for_write_flag` never
passed it to `open(2)`, so every file Perry created landed `0666 & ~umask`
regardless of the request — including claude-code's session transcript, which
asks for `0600` and got `0644`: **world-readable.** The mode now reaches
`open(2)`, which also gives Node's create-only rule for free (an existing file
keeps its permissions; the fixture pins that by chmod-ing a file and rewriting
it with a mode). Three call sites, all in the write/append family.
`writeFile`, `writeFileSync`, `appendFile` and `appendFileSync` were each
verified against Node.

`test_gap_9421_write_file_sync_throws` covers the three sync call shapes
(named import, namespace, computed), the promise and callback forms as
controls, argument validation, `encoding` as both a bare string and an options
field, `flag: "a"`, and `mode` on all four surfaces. On unfixed `main` it
diverges from Node on 14 lines; with the fix it is byte-identical.
`test_gap_9421_append_file_promise_reject` gains a `writer-mode` assertion on
the transcript it reconstructs.
