# WASI native EH verification and Storage size (2026-10-27)

WASI code generation requires **LLVM >=22.1.5**, checked before module lowering against the selected backend: `LLVMGetVersion` for linked in-process LLVM, or the selected clang's version banner for the explicit external backend. Older/unknown external versions fail clearly; native targets keep their requirements. The diagnostic names LLVM #194184 and fix `5c9647b8e680` (22.1.5). No backend fallback is added. The shared Mac's LLVM 22.1.4 was left untouched.

The supplied extracted optimized glob `__620` witness retains 49 invokes and six catchpads (the original function had 53 invokes). Its real CFG compiles in-process on LLVM 22.1.8; the new test runs in WASI CI. No debugger, reducer, or failing 22.1.4 replay was used.

## Toolchains

| Location | LLVM version/source before | This PR |
|---|---|---|
| external-tools.json | No LLVM entry | No LLVM pin to raise |
| setup-llvm22 Linux | apt.llvm.org LLVM 22.x, floating patch | Enforce 22.x >=22.1.5; qb9 has 22.1.8 |
| setup-llvm22 macOS | Homebrew llvm@22 (llvm fallback), floating patch | Enforce 22.x >=22.1.5 |
| setup-llvm22 Windows | Official 22.1.8 tarball | Keep 22.1.8; assert floor for overrides |
| compiler-runtime inline macOS installs | Homebrew LLVM 22.x, floating patch | Enforce 22.x >=22.1.5 |
| gc.yml external Windows pair | Official 22.1.3 tarball | Raised to 22.1.8 |
| gc.yml external macOS pair | Unversioned Homebrew llvm | Reuse setup-llvm22's checked LLVM 22 pair |
| Release glibc 2.31 image | apt.llvm.org LLVM 22.x, floating patch | Enforce 22.x >=22.1.5 |
| Release musl image | Alpine edge LLVM 22 (22.1.8 noted), floating patch | Enforce 22.x >=22.1.5 |

## Verification

Final verification on qb9: WASI smoke, acceptance and pinned-Node gap suites passed; five native LLVM files match `/root/WEH2/native-before/` byte for byte. The four `wasm_eh` unit tests, including the glob codegen witness and version checks, passed. CLI banner stubs verified early rejection of old/unknown external LLVM, acceptance of supported versions, and selection of linked LLVM independently of external clang. Runtime ABI, changed Rust formatting, YAML and Actions topology checks passed. Whole-workspace fmt still reports existing differences outside this change.

```text
6/6 passed
34/34 passed
10/10 passed
```

## Probes at 16 and cost

Probe base: f9d23f9cffe9d98025c97e38d729e4f81a13e7e6. Both probe arms use qb9, PERRY_KEEP_SYMBOLS=1, PERRY_NO_CACHE=1, PERRY_NO_AUTO_OPTIMIZE=1, and the same source-matched wasm runtime; wasm-opt is absent. Cost settings match WEH3: Wasmtime 48, 1,000,000 iterations or awaited calls, n=5 medians including VM startup, repeated after the owned Storage compile finished. Both the concurrent run and repeat are retained in lane evidence. Shared-host contention can affect timings.

| Probe at 16 | f() base | f() PR | Component base | Component PR |
|---|---:|---:|---:|---:|
| fo0-16 | 9,248 | 9,248 | 10,509,135 | 10,477,442 |
| fo1-16 | 203,517 | 11,216 | 10,892,156 | 10,665,631 |
| k-tryfinally-16 | 174,648 | 9,675 | 10,675,279 | 10,478,473 |
| outlined-16 | 9,352 | 9,352 | 10,701,711 | 10,665,840 |

For outlined-16, helper arr plus wrapper is 3,608 + 10 → 1,959 + 10 bytes; f + helper + wrapper totals 12,970 → 11,321 bytes.

| Probe | Wall base (s) | Wall PR (s) | Base ns/op | PR ns/op |
|---|---:|---:|---:|---:|
| try-loop | 0.069139329 | 0.066380872 | 69.1 | 66.4 |
| async-loop | 0.340470565 | 0.355118628 | 340.5 | 355.1 |

## Storage

Storage source: `6dd28b433a8ab78aba24776b7c1ed8278fc2ad21`. Baseline is the manager-provided base-main SjLj build; PR uses qb9 LLVM 22.1.8, `PERRY_KEEP_SYMBOLS=1`, no compile cache or auto-optimize, and the supplied fakesdk clang-wrapper procedure. The full link reaches only the exact same 71 missing perry-stdlib symbols (#11377), with zero signature mismatches. Core links use `--allow-undefined --skip-wit-component`; these size artifacts do not establish a runnable Storage component.

Code bytes are the sum of function bodies, matching the provided baseline and package counter; vector/length framing is excluded. Try-bearing bytes are whole function bodies containing transport: setjmp/try calls in base, opcode-parsed `try_table` in PR. Their share is **72.2% → 23.716%**. Symbol/stripping settings are held as in the supplied procedure; host/toolchain differences remain a limit on attributing the full delta solely to the lowering.

| Metric (bytes) | Base main (SjLj) | PR (native EH) | Change |
|---|---:|---:|---:|
| Code function bytes | 646,080,190 | 235,521,472 | -63.55% |
| Bytes in functions with try transport | 466,210,387 | 55,855,697 | -88.02% |
| Core Wasm | 796,543,678 | 380,355,026 | -52.25% |
| Stripped + compressed relocations | 703,443,557 | 291,372,415 | -58.58% |

| Package/group | Base code bytes | PR code bytes | Change |
|---|---:|---:|---:|
| kubernetes | 252,250,887 | 42,898,351 | -82.99% |
| opentelemetry | 147,332,073 | 87,795,154 | -40.41% |
| perry generated (other) | 98,900,665 | 46,473,510 | -53.01% |
| storage src/ | 53,880,794 | 12,907,663 | -76.04% |
| aws | 24,930,467 | 9,380,813 | -62.37% |
| smithy | 17,312,886 | 6,383,923 | -63.13% |
| fastify | 17,113,748 | 5,814,993 | -66.02% |
| perry runtime | 9,902,679 | 10,019,429 | +1.18% |
| grpc | 9,659,916 | 4,914,661 | -49.12% |
| other/rust | 8,749,210 | 7,072,110 | -19.17% |
| tus | 4,719,369 | 770,526 | -83.67% |
| protobufjs | 377,964 | 368,059 | -2.62% |
| pinojs | 294,089 | 155,997 | -46.96% |
| nodable | 212,136 | 188,585 | -11.10% |
| js | 156,048 | 155,869 | -0.11% |
| platformatic | 105,605 | 40,287 | -61.85% |
| cache | 81,985 | 81,983 | -0.00% |
| ioredis | 71,725 | 71,636 | -0.12% |
| lukeed | 27,944 | 27,923 | -0.08% |
