# WASI Preview 1 output (`--target wasip1`)

**Status: in progress, tracked in [#13259](https://github.com/PerryTS/perry/issues/13259).**
`--target wasip1` does not exist yet; this page is the contract the series
implements, so each step can be reviewed against it. Nothing here changes
`--target wasi` (WASI 0.2 components), the browser `wasm`/`web` targets or the
native targets. [WASI_P1_HOSTING.md](WASI_P1_HOSTING.md) is the unrelated
`node:wasi` input support.

`--target wasip1` is a sibling of `--target wasi`. It reuses the same wasm32
lowering, the same wasi-sdk link recipe and the same runtime layout, and emits
a **core module** (not a component) for hosts that provide
`wasi_snapshot_preview1`. Reusable component authoring stays on the Component
Model; see [the roadmap decision](WASI_ROADMAP.md#decision-support-p1-inputs-and-a-qualified-p1-output).

What lands in which step, in order:

1. This documentation.
2. The runtime builds for `wasm32-wasip1`; networking operations return "not
   supported".
3. The `--target wasip1` compiler target, with hello, filesystem, exception
   and forced-GC acceptance and a check that the module imports only
   `wasi_snapshot_preview1`.
4. A runtime bundle with the Node-module crates (http, zlib, ws and others).
5. Inbound TCP and signals on standard imports, qualified with a reference
   host, then the opt-in `perry_net` outbound and DNS imports.
6. The opt-in `perry_host.discard_pages` import.
7. Further wasm32 code-size and relocation options, then release packaging of
   the P1 runtime archive beside the P2 one.

## Host requirements

- A core-module host with `wasi_snapshot_preview1`.
- WebAssembly exception handling. Generated try blocks and the C runtime use
  LLVM's SjLj lowering on wasm EH, as on P2, so the host needs
  `-W exceptions=y` (Wasmtime) or the equivalent.
- No other imports unless the program was compiled with one of the opt-in
  extensions below.

## Baseline profile

Only standard `wasi_snapshot_preview1` imports. The compiler's default output
imports nothing else, whatever Node modules the program links.

Works: arguments and environment, clocks, random, stdio, filesystem through
preopens, `poll_oneoff` timers, exceptions, the collector, and promise
microtasks. `parallelMap`, `parallelFilter` and `spawn` run sequentially on
the current agent, as on P2.

Inbound TCP uses `sock_accept`, `sock_recv`, `sock_send` and `sock_shutdown` on
listeners the embedder pre-opens (see [Environment conventions](#environment-conventions)).
Plain Wasmtime's p1 layer does not implement these four calls (it reports
`ENOTSOCK`), so a module that serves TCP needs an embedder that does. Modules
that do not use sockets run on plain Wasmtime.

While the module is idle it waits in a single `poll_oneoff` over every open
socket, the signal stream, fd 0 once the program reads standard input, and the
next timer deadline. The reference host in `test-wasi/p1-host` (Unix only;
built and exercised by the `wasi-execution` job) implements these calls over a
single descriptor table and is what the inbound-TCP, signal and stdin cases
run under.

Fails with a documented error:

| Operation | Result |
| --- | --- |
| Outbound `net` connect, `dns.lookup` (no `perry_net`) | "not supported" (`ENOTSUP`), reported as the usual asynchronous socket error or lookup error |
| `listen()` with no pre-opened listener left | `EADDRNOTAVAIL` |
| `child_process` creation | catchable `ERR_NOT_SUPPORTED`, as on P2 |
| Memory discard (no `perry_host`) | no-op |

Preview 1 has no socket creation, `connect` or name resolution, and no way to
release memory pages, so those cannot be standard imports. Existing
non-standard conventions (WasmEdge's `sock_open` family, WASIX) are not
implemented by Wasmtime. The extensions below are Perry-named, documented and
opt-in at compile time.

## Import gating

| Compile setting | Imports of the output |
| --- | --- |
| default | `wasi_snapshot_preview1` only |
| `PERRY_WASM_NET=1` | adds `perry_net.tcp_connect`, `perry_net.sock_error`, `perry_net.resolve` |
| `PERRY_WASM_DISCARD=1` | adds `perry_host.discard_pages` |

Without a setting the runtime links import-free stubs for that extension
(outbound returns "not supported", discard is a no-op), so the module has no
such import and runs on a host that lacks it. The same runtime archive serves
every combination; the compile setting only decides which member is linked.
Both variables change the link and are part of the compile cache key.

## `perry_net` ABI

Every function returns an `i32` WASI errno (0 is success; the numbering is
`wasi_snapshot_preview1`'s `errno`, which is also wasi-libc's). Pointers are
wasm32 linear-memory offsets (`u32`), all integers in memory are
little-endian, and out-parameters are `u32`.

| Import | Signature | Purpose |
| --- | --- | --- |
| `perry_net.tcp_connect` | `(addr_ptr, addr_len, fd_out_ptr) -> errno` | Start a non-blocking outbound connection |
| `perry_net.sock_error` | `(fd, err_out_ptr) -> errno` | Result of the pending connection |
| `perry_net.resolve` | `(host_ptr, host_len, family, fd_out_ptr) -> errno` | Name lookup for `dns.lookup` |

### `tcp_connect`

`addr` is UTF-8 `host:port`, where `host` is a name, an IPv4 literal or a
bracketed IPv6 literal (`[::1]:80`); the string is split at the last colon.
The call never blocks and the host resolves names asynchronously.

On success `*fd_out` is a new non-blocking stream-socket fd whose resolution
and connection are in progress. The guest owns it and must `fd_close` it.
Closing it before completion cancels the resolution and any connection
attempt. The host tries resolved addresses in order.

Readiness: `poll_oneoff` reports the fd ready (writable) only once the
connection has succeeded or failed, never while resolving or connecting.

### `sock_error`

After the fd is reported ready, `*err_out` is the final result: 0 when
connected, otherwise an errno. While the connection is still pending the call
returns `EINPROGRESS` (26).

A name-resolution failure of any kind (not found, temporary failure, any other
reason) is reported as the reserved value **2000**, outside the errno range.
Perry maps it to `ENOTFOUND` with syscall `getaddrinfo`, as libuv does. A host
that resolved the name but could not reach it reports the real errno
(`ECONNREFUSED`, `EHOSTUNREACH`, and so on), not 2000.

### `resolve`

`family` is 0 (any), 4 or 6. `*fd_out` is a guest-owned, non-blocking,
readable byte stream: `fd_read` may return `EAGAIN` and `poll_oneoff` reports
`FD_READ` readiness. Closing it early cancels the lookup. Read it with
`fd_read`, not `sock_recv`.

The stream is a status byte followed by zero or more 17-byte records, and its
EOF ends the answer:

| Part | Meaning |
| --- | --- |
| byte 0 | status: 0 ok, 1 not found, 2 temporary failure, 3 other |
| record | family byte (4 or 6), then 16 address bytes |

An IPv4 address is its 4 octets followed by 12 zero bytes; an IPv6 address is
its 16 octets. An empty stream with status 0 is treated as status 3.

`resolve` serves only the synchronous `dns.lookup`: the guest waits on the fd
with `poll_oneoff`, so the call blocks the thread until the answer, as P2's
resolver does. Connections made by `net` use `tcp_connect` and do not block.

### Rights the embedder grants

| fd | Rights |
| --- | --- |
| listener | `FD_FDSTAT_SET_FLAGS`, `SOCK_ACCEPT`, `POLL_FD_READWRITE`; `fd_fdstat_get` reports `socket_stream` |
| accepted or `tcp_connect` stream | `FD_READ`, `FD_WRITE`, `SOCK_SHUTDOWN`, `POLL_FD_READWRITE`, `FD_FDSTAT_SET_FLAGS` |
| `resolve` stream, signal stream | `FD_READ`, `POLL_FD_READWRITE`, `FD_FDSTAT_SET_FLAGS` |

## `perry_host.discard_pages`

`discard_pages(ptr: i32, len: i32) -> i32` offers freed, 4 KiB-aligned pages
back to the host (for example with `madvise`). The return value is:

- `1`: the range now reads as zero. The runtime may rely on that.
- `0`: the host did nothing. The runtime stops calling.

The WebAssembly memory-control proposal's `memory.discard` has the same
zero-fill semantics and would replace the import if it ships.

## Environment conventions

These need no import; they are part of the baseline profile.

- `PERRY_LISTEN_FDS`: comma-separated fds of pre-opened listening sockets.
  Each `listen()` claims the next unclaimed one in order and ignores the
  requested port. When none is left, `listen()` fails with `EADDRNOTAVAIL`.
  Without the variable the runtime scans fds 3 to 63 for stream sockets.
- `PERRY_LISTEN_ADDRS`: the bound addresses of those listeners, in the same
  order (`ip:port` or `[v6]:port`), reported by `server.address()`.
- `PERRY_SIGNAL_FD`: a readable byte stream. Each byte is a Perry-defined
  signal number, independent of the host OS:

  | Byte | Signal |
  | --- | --- |
  | 1 | `SIGHUP` |
  | 2 | `SIGINT` |
  | 10 | `SIGUSR1` |
  | 12 | `SIGUSR2` |
  | 15 | `SIGTERM` |

  The host maps its native signals to these numbers. A byte is delivered to
  `process.on(...)` listeners. With no listener, all but `SIGUSR1` end the
  process with exit status 128 plus the number, as in Node; `SIGUSR1` is
  ignored. Other bytes are ignored.

The runtime sets `O_NONBLOCK` (`fd_fdstat_set_flags`) on every adopted listener
and on the signal fd; if that fails, `listen()` reports the error and the
signal stream is disabled. Connections it accepts are requested non-blocking
through `sock_accept`'s flags. When `fd_read` on the signal fd returns 0
(EOF), the runtime removes it from the poll set, closes it and never polls it
again.
