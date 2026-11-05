### Windows: `node:net` errors report libuv's `err.errno`, not the negated Winsock code (#10385, #10403)

The turnloop net transport computed `err.errno` as the negated OS code at two
sites: `turnloop_net::errors::map_error` (every socket/listen error) and
`js_perry_net_errno_for_code` (the reverse lookup `perry-ext-net` and
`perry-ext-http`'s deferred listen errors use). On Linux and macOS libuv's errno
is by definition the negated `errno`, so that is correct there. On Windows libuv
has its own `-4xxx` space: `EADDRINUSE` is **-4091**, not -10048, so
`err.errno === -4091` never matched. The committed gap fixtures could not see
it because they print only `errno<0`.

Both sites now go through `libuv_errno(code, os)`: on non-Windows it is still
`-os` (byte-identical behaviour), on Windows it maps the Node code name to
libuv's number (values read from Node 26.5.1's `util.getSystemErrorMap()`),
`UV_UNKNOWN` (-4094) for a name libuv has no number for, and 0 when no syscall
ran. The Windows table is compiled under `cfg(test)` too, so a host-independent
test pins its values and cross-checks every code it shares with
`util_syserr`'s filesystem table (#10452).

`agent_loop.rs` also records why `max_operations = 32_768` is NOT lowered on
Windows although turnloop's IOCP operation slab is eagerly committed there
(~36 MB of an idle HTTP server's working set, measured on Windows under
#10385): a smaller ceiling reinstates the #10351 connection refusal; the fix
belongs in turnloop (reserve the slab contiguously, commit to the high-water
mark).

This is the remainder of PR #10403, ported onto main. The rest of that PR is
already on main and was not re-carried: the turnloop integration history it was
stacked on landed as #10354; the Itanium landingpad shape for windows-msvc,
`eh_lsda.rs`, `perry-ext-http`'s Windows errno and the `node_compat_matrix.mjs`
fixes landed as eee081c04c; the `reorder_child` and `lru_subclass` link breaks
were fixed differently by 7b90108d17 (`/ALTERNATENAME` fallbacks instead of a
`lru-subclass` feature plus a `perry.exe` link shim); the child-process
`Option<CpPipe>` cfg(windows) fix and the `heap_generation` profile test are
already on main in their own form.

Validation for the port: macOS/aarch64 unit tests plus
`cargo xwin check --target x86_64-pc-windows-msvc`. The Windows behavioural
numbers (turnloop gap fixtures 15/16, the 10,000-connection ceiling, the full
Windows gap run of 790/835 after clearing stale auto-optimize archives) were
measured by #10403 on `turnloop/integration` and are not re-verified here.
