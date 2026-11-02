# WebSocket AOT acceptance

Perry's concrete comparison workload is the unchanged TypeScript state machine
from [guest271314/websocket-server](https://github.com/guest271314/websocket-server/tree/d7f94f5634f733415f1f52547fe45b3794d9d41a).
It reads stdin and writes stdout; a socket bridge supplies the connection.
The acceptance harness pins both the server and upstream clients by SHA-256.
It exercises native output, a Preview 1 core module, and a WASI 0.3 command
component against Node 26.12.1 and independently generated expected wire bytes.

The [release build receipt](validation/websocket-aot-release-20261102.json) qualifies
27 functional phases at source `6ea71cc1a3`, including both unchanged upstream
clients on all three Perry output formats. The P3 text client keeps its original
four-second deadline: the initial sequential host missed it, and enabling the
pinned engine's parallel compiler passes it with four compiler threads. This
compiler pool uses Rayon; guest I/O continues through Turnloop, with the strict
Tokio ban passing. The native compiler and coherent native archives use `prod`;
the P1 and plain P3 runtime archives use release. All were freshly built at the
recorded source. All 124 indexed evidence files, their sizes and hashes, and
their exact regular-file set were verified locally.

| Functional build artifact | Bytes |
| --- | ---: |
| Native executable | 7,542,624 |
| Preview 1 core module | 5,845,114 |
| WASI 0.3 command component | 6,075,064 |
| Optional native P3 command host | 33,102,952 |

The [balanced production cohort](validation/wasi-balanced-production-cohort-20261102.json)
subsequently passes the same 27 phases at `3a20cda3b5` with freshly built native
artifacts and runtime archives. Its native executable is 7,550,816 bytes, P1
module 4,748,649 bytes and P3 component 4,955,774 bytes. These sizes qualify the
builder's functional output; the five-run profile experiment and scriptc
timings below retain their original source/artifact identities. All 486
indexed cohort evidence files were verified locally. The same cohort also
passes the compound/cancellation and resource/transport regressions with
positive moving-GC witnesses. Full WASI 0.3 support remains incomplete.

These are measured release build sizes. The
[pinned scriptc comparison](validation/websocket-scriptc-comparison-20261102.json)
adds directional process measurements for native and P1 output. The
[earlier functional receipt](validation/websocket-aot-20261101.json) retains its
original source and build profiles; its separate 45 command regression phases
and eleven compiled P3 commands were not reexecuted by this release workload.

This application passes while broader WASI 0.3 qualification remains incomplete.
A separate resource-bearing future factory failed evacuation verification under
forced moving GC at this release receipt's source. The
[later weak-slot repair receipt](validation/wasi-moving-weak-slots-20261102.json)
qualifies the fix at `5cd1705734`: all 17 resource phases and 20 transport phases
pass, with 17,789 and 12,898 relocated guest objects respectively. The fresh
instrumented runtime uses that source; its native compiler and plain reference
runtime remain sourced at `6ea71cc1a3`. Full WASI 0.3 qualification remains open.

Both Perry and [scriptc 0.4.7](https://github.com/vercel-labs/scriptc/releases/tag/v0.4.7)
compile the unchanged source and pass all three independent wire cases in
native and P1 form. Both native executables are stripped. Values below are
Perry / scriptc; times and memory are medians of five rotated process runs.

| Output | Binary bytes | 20 MiB seconds | Peak RSS, KiB |
| --- | ---: | ---: | ---: |
| Native | 7,542,624 / 184,944 | 11.96 / 10.17 | 43,280 / 1,812 |
| P1 | 5,845,114 / 158,221 | 16.63 / 13.88 | 121,176 / 59,652 |

These runs use a shared Linux host, warm OS caches, cold processes, regular-file
stdin and discarded stdout. Time includes startup and the launch wrapper;
RSS includes each engine/host process. Separate regular-file controls verify
all 20,974,867 expected output bytes for every measured artifact and host
configuration. P1 uses the same Node 26.12.1 host with a program-name argument:
with an empty argument list, Node's `args_get` returned `EINVAL` for scriptc's
C startup. The matching scriptc WASI runtime pack and Zig toolchain are pinned
in the receipt. Process timings cover this workload; sustained socket
throughput and the complete jz/js2 comparison remain to measure. Both evidence
archives were verified locally: all 434 indexed files, sizes and hashes match.

The measured gaps put runtime footprint and memory use ahead of tuning the
roughly 18–20% process-time difference. Native size differs by about 41 times
and P1 size by about 37 times. The scriptc P1 artifact provides a concrete
small-module reference alongside the upstream WasmGC example.

A [same-source runtime profile experiment](validation/websocket-wasi-size-profiles-20261102.json)
at `5cd1705734` isolates the WASI runtime optimization setting. Package features,
ThinLTO and required unwind arguments remain identical. Each variant passes
the three independent wire cases and a separate exact 20 MiB regular-file
output control before five rotated process measurements.

| Runtime optimization | P1 module bytes | Reduction | Median time change |
| --- | ---: | ---: | ---: |
| `3`, control | 5,873,817 | — | — |
| `s`, selected | 4,748,649 | 19.16% | +1.5% |
| `z` | 3,650,936 | 37.8% | +7.2% |

The WASI archive builder selects `s` with the existing ThinLTO profile. These
measurements qualify the P1 experiment; the subsequent builder change still
requires its fresh P1/P2/P3 qualification. Native build profiles are unchanged.
The separate `z` plus FatLTO counterexperiment increased final module size and
process time, so it was rejected. All 121 indexed files in the profile evidence
archive were verified locally, including both runtime archives, modules,
compiler-argument logs, wire outputs and source. Sustained socket throughput
remains unmeasured, and the remaining footprint gap is still substantial.

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

The pinned comparison records stripped raw/gzip sizes and whole-host process
costs. A complete performance comparison still needs the jz/js2 artifacts and
sustained socket measurements with matching bridge/engine versions. The
upstream README's js2wasm size remains an upstream claim. The earlier acceptance
smoke timings retain their functional scope.

The footprint comparison also spans different memory models. The
[js2wasm project](https://github.com/loopdive/js2) emits WasmGC and requires
post-MVP engine features. Perry's WASI output carries its linear-memory runtime
and collector. Record required engine features and host memory alongside guest
bytes when reproducing the comparison. Fresh release archives incorporating
the integrated runtime pruning still produce a roughly 5.85 MB P1 module;
the large footprint remains a competitive gap. Further size work should
attribute the remaining retained functions. A WasmGC backend would be a separate compiler
project from completing the current WASI 0.3 component profile.
