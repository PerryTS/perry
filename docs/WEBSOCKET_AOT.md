# WebSocket AOT acceptance

Perry's concrete comparison workload is the unchanged TypeScript state machine
from [guest271314/websocket-server](https://github.com/guest271314/websocket-server/tree/d7f94f5634f733415f1f52547fe45b3794d9d41a).
It reads stdin and writes stdout; a socket bridge supplies the connection.
The acceptance harness pins both the server and upstream clients by SHA-256.
It exercises native output, a Preview 1 core module, and a WASI 0.3 command
component against Node 26.12.1 and independently generated expected wire bytes.

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
libc's original standard descriptors, and wasi-sdk 34's stdin destructor drops
the outstanding completion future using the stream-drop operation. P2/P3 fs
entries now own descriptor duplicates, following Perry's native ownership
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
