# Standalone WASI support (#11375)

Perry's experimental `--target wasi` produces a WASIp2 component with its
runtime and collector in linear memory. It needs a WASI host with WebAssembly
exception handling enabled, and no JavaScript host. This is separate from the
browser `wasm`/`web` targets.

The pinned wasi-sdk 34 build imports **WASI 0.2.12** interfaces. Hosts must
provide the imported members as well as Wasm exception handling; a generic
"Preview 2 supported" claim is insufficient. Wasmtime **48.0.0** is the tested
host. A macOS check with Wasmtime **44.0.0** first required `-W exceptions=y`
and then failed because `wasi:cli/exit@0.2.12`'s `exit-with-code` was missing.

## Output versus hosting

WASI versions here mean 0.1 (Preview 1), 0.2 (Preview 2), and 0.3
(Preview 3), independently of core WebAssembly proposal support.

| Path | Current contract | Main gap |
| --- | --- | --- |
| Browser `wasm` / `web` output | Core Wasm with Perry's JavaScript bridge | Not standalone WASI; raw Wasm output omits generated async JavaScript |
| WASI 0.1 output | No supported compiler target | Defer until a concrete P1-only deployment requires it |
| WASI 0.1 hosting in native Perry | Partial `node:wasi` lifecycle/import surface over wasmi | Most syscalls, operational preopens, correct randomness/clocks, guest exit |
| WASI 0.2 output | Command components on Linux/macOS; initial synchronous library components qualified on Linux | Guest resources, HTTP and broader component qualification |
| WASI 0.2 component hosting | Optional native `perry/component` host with typed WIT values and guest resources; Linux qualified | Host-defined resources, built-in WASI/HTTP providers and other platforms |
| WASI 0.3 output/hosting | Not implemented | Native component async ABI, futures/streams, scheduling and cancellation |

Perry's existing `perry/plugin` API loads native shared libraries; it does
not load Wasm components. A command component with a `wasi:cli/run` export
also does not provide arbitrary typed plugin exports. See
[the WASI roadmap](WASI_ROADMAP.md) for the proposed hosting and reusable
component contracts, their priorities, and acceptance criteria.

WASI components run through a host such as Wasmtime. The current repair was
tested on Linux x86_64 with Wasmtime 48.0.0, wasi-sdk 34,
nightly-2026-10-04 and LLVM 22. Earlier Windows x64 results are recorded below. See
[Wasmtime platform support](https://docs.wasmtime.dev/stability-platform-support.html)
and [the agreed scope, #11375](https://github.com/PerryTS/perry/issues/11375).

## Implemented contracts

- Arguments, environment variables, standard output, and filesystem access
  through explicit directory preopens. Denied filesystem access raises a
  catchable error.
- UTC date operations. Local-time operations use the existing UTC fallback.
- Nested exceptions, catch/finally/rethrow, runtime-originated errors and
  errors crossing promise callbacks. Generated try blocks and the C runtime
  use LLVM's wasm SjLj lowering, modern wasm EH, and wasi-libc `libsetjmp`.
  Unwinding restores the GC shadow stack and async context. Locals visible
  after setjmp stay in volatile stack slots through optimization.
- Perry's collector, including moving collections. GC slot descriptors
  distinguish native 32-bit pointers from 64-bit tagged values, including
  object metadata, promises, errors, regexps, buffer owners and lazy JSON.
  Map/Set allocation classifiers account for allocation padding on ILP32.
- Runtime and generated closure ABIs agree on wasm indirect-call signatures,
  including class-method function objects. Tagged values retain all 64 bits
  when reflecting on objects and classifying native handles.
- Timers, cancellation and promise microtasks continue while sockets are
  active. The WASIp2 transport uses nonblocking wasi-libc sockets and a bounded
  event-loop wake deadline; callbacks run after transport borrows are released.
- DNS through wasi-libc's WASIp2 resolver, and TCP listen/accept/connect,
  read/write, write-side shutdown and close. The runtime static archive bundles
  the existing net extension and its callback bridges with one Rust allocator.
  Network denial produces the socket's normal asynchronous error event.
- `parallelMap`/`parallelFilter` run sequentially; `spawn` is deferred on the
  current agent with promise adoption and rejection. Captured references and
  pending results remain rooted. There are no parallel WASIp2 worker threads.
- Child process creation throws a catchable `ERR_NOT_SUPPORTED` error with
  `child_process is not supported on WASI`, including fork and async creation.

The net extension builds its rustls client path with WebPKI roots on WASI;
TLS interoperability is not established by the TCP acceptance probes. UDP
(`mod-dgram`), Unix-domain/named pipes, native addons, host process creation,
and general platform-specific Node/Bun APIs are outside the tested profile.
This is not a claim of complete Node API compatibility. Fresh compiler/runtime
builds pass on Linux and macOS. Windows currently fails during linking, as
detailed below. Typed library components have a separate
[implementation and qualification record](WASI_COMPONENTS.md).

## Acceptance and CI

The 2026-10-21 repair, based on
`20fde48d3fa03d0a77699b7374f55e38cb143b59`, restores fresh compiler and
runtime builds and linked execution on Linux x86_64. Measured with
Wasmtime 48.0.0, wasi-sdk 34, LLVM 22 and nightly-2026-10-04:

- **6/6 smoke**, **20/20 acceptance**, and **10/10 comparisons with Node 26.5.1** pass.
- **9/9 exact-source ABI/closure lowering tests**, **16/16 focused native
  layout, builtin-entry and image-classifier tests**, and **6/6 runner tests**
  pass. The generated ABI table matches the source and the Node pin check passes.
- The native ABI checker passes after teaching it that `[DOUBLE; 6]` means
  six parameters. The former `js_eval_in_scope` arity report was a checker
  false positive; the codegen and runtime signatures already agreed.
- File-size, address-classification and GC-holder inventory gates remain red
  on the baseline. The address and GC-holder diagnostics introduce no new
  issue paths. This is not a clean full-CI result or a full native test sweep.

The runtime build uses all default features except `alloc-mimalloc`, as
selected by `scripts/wasi_build_runtime.py`, with 16 release codegen units.
The initial 689 compiler diagnostics were traced to pointer-bearing custom
sections, 64-bit layout assumptions and the missing owner-link socket
transport. Repairs use constructor-registered linear-memory image records,
explicit ILP32 descriptor layouts, preserved `BandDir` offsets and pinned
transport owners/completions. The original layout assertions remain effective.

Execution then exposed macro-generated symbols omitted by the ABI checker,
incorrect internal array FFI declarations and the missing runtime feature
installer. The repaired WASI link installs the program's selected runtime
features before user code. The new fixtures exercise static metadata across
moving GC, array prototype calls, URL accessors and repeated socket deadlines
across close/reconnect. These are execution results, not compile-only passes.

On macOS ARM64 on 2026-10-21, the Linux-produced repaired artifacts pass
**6/6 smoke**, **20/20 acceptance**, and **10/10 fresh comparisons with Node
26.5.1**, using native Wasmtime 48.0.0. SHA256 checks verify all 36 Wasm
artifacts and 74 fixture/configuration/expected-output inputs against the
Linux validation snapshot. The [validation receipt](validation/wasi-macos-20261021.json)
records the cases, tool versions, and hashes. This qualifies execution on that
host, not a macOS build of Perry. Logs and the artifact manifest are in the local audit's
`reports/2026-10-21-wasi-macos` directory.

Fresh Linux and macOS CI builds subsequently passed **6/6 smoke**, **20/20
acceptance**, **10/10 Node comparisons**, and **7,458 IR runtime-call ABI
checks** on 2026-10-21. See the [Linux job](https://github.com/PerryTS/perry/actions/runs/39574495557/job/119136673285)
and [macOS job](https://github.com/PerryTS/perry/actions/runs/39574495557/job/119136673071).
These jobs built both Perry and the WASI runtime from source on their hosts.
The [Windows job](https://github.com/PerryTS/perry/actions/runs/39574495557/job/119136673535)
built the compiler and runtime, but all six smoke links failed with
`wasm-ld: malformed uleb128, extends past end`. Current Windows execution
is therefore unqualified; the older success below does not supersede this failure.

Verified on Windows x64 on 2026-10-08 with the standard release runtime build:
**18/18 acceptance**, **6/6 smoke**, and **10/10 Node comparisons** pass. Native
GC regressions pass **171 tests** (one existing ignored test); WASI codegen unit
tests pass **10/10**, the component-linker regression passes, and the six Python
harness tests pass. The no-default-features WASI runtime check, runtime ABI
self-test/table check, Node pin consistency and Actions topology check pass.


The acceptance fixtures exercise bytes/DataView, UTC dates, preopened files,
permission denial, arguments/environment, GC edges under forced evacuation,
tagged reflection, exceptions across runtime/callback boundaries, timers,
DNS, loopback TCP, timers plus GC during TCP, socket reuse with deadlines,
static image metadata and builtin accessors, sequential thread fallbacks,
and unsupported child processes. The smoke suite adds classes, closures,
collections, iterators, values and exit status. Ten existing gap programs are
compared against the pinned Node 26.5.1 oracle, including advanced classes,
objects, JSON, regexp and async behavior.

The WASI codegen CI job is configured to run the complete acceptance, smoke and ten-case gap
suites on Windows, Linux and macOS. Linux and macOS pass; Windows has the
linker failure described above, repaired by [PR #12954](https://github.com/PerryTS/perry/pull/12954)
with fresh Windows qualification pending. The full platform matrix retains its
extended-suite routing. A separate Linux `wasi-execution` job runs the complete
command smoke, acceptance and pinned-Node comparison suites for core PRs and
in the sweep/full tiers. It builds the compiler and WASI runtime from the same
revision and feeds the required `pr-gate`; a planned job that is skipped,
missing, cancelled or failed makes that gate fail. Docs-only PRs skip execution.
This promotion uses the demonstrated Linux baseline in run39574495557.
The baseline repair landed in [PR #12931](https://github.com/PerryTS/perry/pull/12931);
the initial measurements above came from local/validation hosts, with the
subsequent per-platform CI results linked separately.
Build and per-case logs are retained under `/root/codex-wasi-20261021/logs` and
copied to the local audit's `reports/2026-10-21-wasi-implementation` directory.

## Reproducing on Windows

Configure the repository's pinned Rust and LLVM 22 toolchain first. Python
3.10+ is required. From the repository root in PowerShell:

```powershell
. ./scripts/wasi_toolchain.ps1
rustup target add wasm32-wasip2
cargo build --locked -p perry --no-default-features --features compile-cli,target-wasi
python scripts/wasi_build_runtime.py
python scripts/wasi_run.py target/debug/perry.exe --suite smoke --output .cache/wasi-smoke
python scripts/wasi_run.py target/debug/perry.exe --suite acceptance --output .cache/wasi-acceptance
python scripts/wasi_run.py target/debug/perry.exe --suite gap --node <path-to-pinned-node> --output .cache/wasi-gap
```

The checksum-pinned PowerShell installer supports Windows x64 and ARM64;
only x64 was tested. The runtime build uses standard release settings and
all runtime default features except the 64-bit-only allocator (`alloc-mimalloc`).
UDP transport is excluded by the wasm32 target configuration. LLVM development headers
are needed to build the wasm exception-model target-options shim.

On x86_64 or ARM64 Linux/macOS, configure tools with the checksum-pinned shell
installer, then use the same Python commands with the platform compiler path:

```bash
toolchain_env="$(./scripts/wasi_toolchain.sh .cache/wasi)" && eval "$toolchain_env"
```

The installer selects the upstream host architecture, uses the platform's
SHA256 tool, and quotes installation paths for bash. It also configures
shell-escaped C flags for runtime builds in paths containing spaces. Shell
build/smoke entry points delegate to the portable implementations. Installing
the tools alone does not establish Perry build or execution parity.
The ARM64 macOS installer passed a real download/checksum/extract cycle and
C-to-WASI command execution, including a cached installation path containing
spaces and an apostrophe. Intel macOS and ARM64 Linux asset pins are verified
against upstream release metadata; their installers have not been executed.

Fixtures run in separate working directories. Filesystem probes explicitly
preopen their working directory. Networking probes grant `-S inherit-network=y`;
DNS also grants `-S allow-ip-name-lookup=y`. Denial fixtures grant neither.
Arguments follow the component path. GC fixtures set `PERRY_GC_FORCE_EVACUATE=1`
and `PERRY_GC_VERIFY_EVACUATION=1` in the guest environment.

Harness and ABI checks:

```powershell
python scripts/test_wasi_run.py
python scripts/runtime_abi_check.py --self-test
python scripts/runtime_abi_check.py --check-wasm-abi
python -X utf8 scripts/check_actions_topology.py
cargo test --locked -p perry --bin perry --no-default-features --features compile-cli,target-wasi links_a_wasip2_component -- --nocapture
```

`--filter` accepts repeatable fixture-name substrings. Compile failures,
timeouts, missing artifacts, oracle failures and output/exit mismatches fail
the run. `--output` preserves `report.json`, compilation logs, stdout and
stderr; without it the files are temporary. The historical `core` suite remains
available for quick diagnostics; CI gates the complete `acceptance` suite.

## Native Preview 1 hosting

Running third-party Preview 1 core modules inside native Perry is a separate
profile. See [WASI_P1_HOSTING.md](WASI_P1_HOSTING.md) for the `node:wasi` host,
capability ownership, execution tests and limitations.

CI preserves per-case logs, reports, Wasm outputs, generated LLVM IR and
compiler objects under the `wasi-validation-<OS>-<SHA>` artifact. Failed
Windows jobs additionally retain the matching WASI runtime archive for
linker diagnosis. Locally, use `scripts/wasi_run.py --output <directory>
--keep-intermediates` to retain the same evidence without changing the
compiler backend. These test artifacts are not shipping release packages.
