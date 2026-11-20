**fix(runtime): `fs.watch` / `fsPromises.watch` use OS change notifications instead of re-walking the tree every 25 ms (#9591)**

Every watcher was a 25 ms `setInterval` whose tick re-walked the WHOLE watch
target (`read_dir` + `symlink_metadata` per entry) and diffed two maps, on the
main thread: ~3.4 µs per file per tick, 40 ticks a second. Watching 3 000
files for 5 s cost 2.03 s of CPU (41 % of a core) against node's 0.03 s.
claude-code watches its cwd; the field session's cwd held 362 295 files, which
extrapolates to ~1.2 s of walking per 25 ms schedule — a wedged event loop,
#9588's symptom exactly.

The OS now reports changes. `notify` (inotify on Linux/Android,
`ReadDirectoryChangesW` on Windows, kqueue on the BSDs) or, on macOS, FSEvents
bound at runtime through `dlopen` (`watch_fsevents.rs`) runs on its own thread
and queues each event; the queue follows the event pump's producer protocol
(push, then `js_notify_main_thread()`), and a runtime pump slot drains it once
per loop turn and routes each event to the watchers it concerns —
`'rename'` for create / remove / move, `'change'` for data and metadata
writes, filenames relative to the watched root. Nothing walks anything on a
timer any more. Watching 3 000 files for 4 s now costs single-digit
milliseconds of CPU, and a change surfaces in milliseconds instead of at the
next tick.

Instances mirror libuv's sharing: non-recursive watchers share one instance
per JS thread, refcounted by canonical root — `fs.inotify.max_user_instances`
defaults to 128 and a chokidar / `tsc --watch` style consumer opens one
`fs.watch` per directory, so one instance per watcher would have failed at the
129th. Recursive watchers get their own instance, because notify keys its
per-path bookkeeping by path and a recursive root sharing an instance with a
non-recursive watch of one of its subdirectories would clobber it. Liveness
moved from the ref'd interval timer to a new runtime has-active slot
(`register_runtime_has_active`, the counterpart of `register_runtime_pump`), so
`persistent: false`, `ref()`, `unref()` and `close()` release the loop as before.
Backend errors (an exhausted inotify watch table while a recursive watcher
adds a new subdirectory, for instance) now reach `'error'` listeners as
Node-shaped fs errors, or the uncaught-exception path when there is none.

macOS does not use notify's FSEvents backend on purpose: `fsevent-sys` links
CoreServices through `#[link]` metadata that does not survive perry's custom
link step, and every `fs` importer retains the watcher (the module table pins
`js_fs_watch`), so every such binary would have needed `-framework
CoreServices` — an umbrella that drags CoreFoundation — on its link line, the
launch-time cost perry keeps off console binaries. The ten CoreFoundation /
CoreServices entry points are resolved with `dlopen` at the first `fs.watch`
call instead; a program that never watches never loads the framework, and the
link line is unchanged. Flag classification follows libuv (rename beats
change; latency 0.05 s so bursts coalesce as under node), delivery uses a
private dispatch queue (libdispatch is in libSystem), and the stream is
rebuilt when the path set changes, as libuv and notify do.

The walker survives only as the fallback for when the OS watch cannot be
established (watch limit, unsupported target, or `PERRY_FS_WATCH_POLL=1` as a
diagnostic switch). It runs on its own thread — the walk never blocks the loop
— and paces itself to 5 % of one core: each walk's duration times 20, clamped
to [25 ms, 5007 ms] (the old cadence at the bottom, `fs.watchFile`'s default at
the top). A 3 000-file tree polls every ~200 ms under it; the 362 k-file tree
every 5 s, off the main thread, instead of 40 times a second on it.

Verification: `crates/perry/tests/issue_9591_fs_watch_native_events.rs` is the
issue's bar — watch 3 000 files for a 4 s window, assert < 5 % of a core AND
that a new file is reported within a second (the unfixed walker burns ~1.6 s
in that window); the same for the forced poller at a 12.5 % budget.
`test-files/test_gap_9591_fs_watch_events.ts` pins the event contract against
node for the callback, single-file, recursive and promise-iterator forms.
