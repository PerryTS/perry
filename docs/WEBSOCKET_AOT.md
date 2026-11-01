# WebSocket AOT acceptance

Perry's concrete comparison workload is the unchanged TypeScript state machine
from [guest271314/websocket-server](https://github.com/guest271314/websocket-server/tree/d7f94f5634f733415f1f52547fe45b3794d9d41a).
It reads stdin and writes stdout; a socket bridge supplies the connection.
The acceptance harness pins both the server and upstream clients by SHA-256.
It exercises native output, a Preview 1 core module, and a WASI 0.3 command
component against Node 26.12.1 and independently generated expected wire bytes.

The [Linux acceptance receipt](validation/websocket-aot-20261101.json) qualifies
27 functional phases at source `02cf3068ae`, including both unchanged upstream
clients on all three Perry output formats. The P3 text client keeps its original
four-second deadline: the initial sequential host missed it, and enabling the
pinned engine's parallel compiler passes it with four compiler threads. This
compiler pool uses Rayon; guest I/O continues through Turnloop, with the strict
Tokio ban passing. Eleven compiled P3 commands and 45 command regression phases
also pass, including both stdin lifetime cases. All 505 indexed evidence files
and their exact regular-file set were verified locally.

| Functional build artifact | Bytes |
| --- | ---: |
| Native executable | 7,436,096 |
| Preview 1 core module | 5,844,453 |
| WASI 0.3 command component | 6,073,109 |
| Optional native P3 command host | 35,380,360 |

These are functional build sizes, not a release performance comparison. The
native archives use `perry-dev`; the separate WASI archives use release. Guest
archives remain sourced at `e48d556ab6`, with exact selected target dependency,
feature, lock-record and path-source equality proved against the newer native
source. They were not restamped as newly built at the native source.

The stripped P1 artifact contains 5,347,131 bytes of code (91.49%) and 478,665
bytes in its data section. A separate diagnostic build with names identifies
object/builtin support and GC as the largest symbol-prefix groups. That build
retains relocation padding, so its individual function sizes are diagnostic.
The next footprint work should target retained runtime code. Public async
plugin generation, full providers, installed platform packages and the rest of
the WASI 0.3 completion matrix remain outside this receipt.

Run `scripts/websocket_aot_acceptance.py` with a checkout at that commit, a
compiler built with `target-wasi`, coherent native archives, separate P1/P3
runtime archive directories, and the P3 contract checker:

```sh
python3 scripts/websocket_aot_acceptance.py \
  --perry /artifacts/native/perry \
  --native-runtime /artifacts/native \
  --p1-runtime /artifacts/p1 \
  --p3-runtime /artifacts/p3 \
  --p3-contract /artifacts/p3-contract \
  --upstream /sources/websocket-server \
  --output /results/websocket-aot \
  --live-clients
```

Set `WASI_SDK_PATH` to the qualified wasi-sdk 34 installation. Each runtime
directory contains `libperry_runtime.a`; native output also needs its matching
stdlib/component-host archives and GC effect sidecars. The checker is built
from `test-wasi/p3-entropy` with `--bin p3-contract`. Use a fresh output directory.
Run heavy builds and acceptance on the qualification host.

For the same socket-bridge setup as the upstream README, compile the unchanged
source with the corresponding native or Preview 1 runtime directory:

```sh
PERRY_RUNTIME_DIR=/artifacts/native /artifacts/native/perry compile \
  /sources/websocket-server/websocket-server-wasm-debug.ts \
  --no-auto-optimize --no-cache -o /artifacts/server-native
PERRY_RUNTIME_DIR=/artifacts/p1 /artifacts/native/perry compile \
  /sources/websocket-server/websocket-server-wasm-debug.ts --target wasip1 \
  --no-auto-optimize --no-cache -o /artifacts/server-p1.wasm
```

A socket bridge can launch either artifact. These equivalent `socat` recipes
follow the upstream setup; the acceptance harness supplies the same inherited
socket descriptors directly from Python:

```sh
socat TCP-LISTEN:8080,reuseaddr,fork EXEC:'/artifacts/server-native'
socat TCP-LISTEN:8080,reuseaddr,fork EXEC:'node --no-warnings /sources/websocket-server/node-wasi.mjs /artifacts/server-p1.wasm'
```

The unchanged upstream `node-wasi.mjs` also passes a separate exact control-wire
smoke with the P1 artifact. The full 20 MiB/client matrix uses the harness's
standard Node WASI host. No socket host extension is needed inside the module.

The byte tests cover the sample handshake, text, binary, ping/pong, close,
length boundaries through 1 MiB, and 20 MiB in 320 separate 64 KiB frames.
Both the output bytes and successful process exit must match. The optional
live tests connect the unmodified upstream text and 20 MiB clients through real
loopback sockets. The binary client requires free port 8080. The upstream
clients check counts and close status; the independent byte tests also check
binary contents. These checks cover this application's protocol behavior;
they do not establish complete RFC 6455 compliance.

P1 imports must pass `scripts/wasi_imports_check.py` without custom host-import
allowances. P3 must pass the exact 0.3.0 command contract checker. The harness
retains source, compiler/runtime artifact hashes, all expected/actual bytes,
phase statuses, and generated Node P1/Perry P3 command hosts.

The initial P3 run exposed two interacting issues: Perry's fs registry owned
libc's original standard descriptors, and [wasi-sdk 34's stdin destructor](https://github.com/WebAssembly/wasi-libc/blob/2e6fb9d8ee0cdf9e431fbcabe8af3115de000a13/libc-bottom-half/sources/wasip3_stdio.c#L61) drops
the outstanding completion future using the stream-drop operation. P2/P3 fs
entries now own descriptor duplicates, following Perry's Unix ownership
policy. Closing a registry entry or returning from a task releases its own
reference; libc's standard streams remain owned by the component instance.
Separate command regressions cover returning before EOF and consuming EOF.
This ownership change does not repair arbitrary direct uses of the SDK's
incorrect stdin destructor.

Functional acceptance is the first comparison milestone. Fair performance
qualification still requires release native archives, identical bridge/engine
versions, repeated cold starts, throughput and peak memory, and stripped raw
and compressed artifact sizes. Engine/host process costs must be reported
alongside guest costs. The upstream README's js2wasm size is an upstream claim;
we have not reproduced competitor artifacts. Smoke timings do not establish
that Perry is faster or smaller than another compiler.

The footprint comparison also spans different memory models. The
[js2wasm project](https://github.com/loopdive/js2) emits WasmGC and requires
post-MVP engine features. Perry's WASI output carries its linear-memory runtime
and collector. Record required engine features and host memory alongside guest
bytes when reproducing the comparison. The next Perry measurement should use
current main's runtime pruning and fresh release archives, then attribute the
remaining retained functions. A WasmGC backend would be a separate compiler
project from completing the current WASI 0.3 component profile.
