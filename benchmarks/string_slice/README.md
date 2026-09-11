# String suffix parsing (#10061)

The three TypeScript sources are copied unchanged from [issue #10061](https://github.com/PerryTS/perry/issues/10061).
`measure.py` runs each engine serially, with the issue's five input sizes and
60-second process timeout. The workload itself checks every warmup and measured
checksum, warms for at least 200 ms and five invocations, and reports the median
of seven samples of at least 20 ms each. Input setup remains outside the timer.

## Implementation and limits

Materialized `slice`, `substring`, and `substr` now share a bounded WTF-8 boundary
walker. A boundary between the two UTF-16 units of an astral scalar retains the
requested high or low surrogate, encoded as WTF-8. Result byte length, UTF-16
length, and lone-surrogate flags agree. Complete byte ranges use the existing
rooted copy; split boundaries are assembled in Rust-owned memory before any
destination allocation can move or collect the source.

The native compiler also keeps an eligible suffix local as its ordinary rooted
source plus three scalar offsets on the stack. `s = s.slice(k)` advances that
cursor, and `.length`/`charCodeAt(i)` read relative to it. The byte cursor can stop
between an astral scalar's surrogate halves. Consuming the whole input has linear
total decoding work and performs no substring allocations or suffix copies.

Eligibility is conservative: a mutable local declaration in the function's outer
statement list, only discarded self-assignments from `slice` with an omitted or
nonnegative constant start, and only length and constant-index `charCodeAt`
consumers. Return values, aliases, captures, other writes, negative/dynamic slice
bounds, an explicit end, and other string consumers keep ordinary materialized
strings. A runtime string-tag guard preserves the existing property/method path
when a TypeScript string annotation actually holds a different kind of value.
This is compiler scalar replacement, not a new public string representation;
the flat string layout and FFI ABI are unchanged. Unselected loops can still
incur repeated suffix copying; general escaping substring views are separate
representation work.

Memory policy: an eligible cursor retains its original source through the
ordinary local GC root until that root is released; its state contains no
interior pointers. It creates no shared backing-store chain, cache, or persistent
GC root. A small materialized slice owns its bytes and retains no source string.
Moving-GC coverage asserts that the source really relocates while a cursor sits
between surrogate halves, and that a separately retained slice remains valid.
The compiled stress fixture overwrites the original binding and allocates inside
the parse loop.

The change leaves trim operations (#10054), general Unicode random indexing
(#10055), and HIR `for-of` iteration stride (#10062) independent.

## Reproduction

Base: `603b074ace01464bc66fc07cc8d532f26ccf5a0f` (pristine main), Perry
`0.5.1532`; Node `v26.5.1`; native Windows x64. Compiler and both matching static
archives were built together with:

```powershell
$env:LLVM_SYS_221_PREFIX = 'C:\llvm'
$env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = '16'
cargo build --release --locked -j 6 -p perry -p perry-runtime-static -p perry-stdlib-static
$env:PATH = 'C:\llvm\bin;' + $env:PATH
$env:PERRY_RUNTIME_DIR = (Resolve-Path target/release).Path
python benchmarks/string_slice/measure.py --perry target/release/perry.exe --output benchmarks/string_slice/fixed.json
```

The release optimization level and thin LTO are unchanged; 16 codegen units are
used in both arms. `baseline-artifacts.json` records the pristine compiler and
archive hashes. Result files include source hashes and engine versions. Timing
runs are serialized with each other and with local builds; this is a shared
development host, so constant factors are diagnostic rather than a quiet-host
performance claim. Unicode timings are interpreted only after checksum parity.

The pristine reduction reproduces the issue exactly:

```text
Perry: 5:228,4:20013,3:55357,2:195,1:150,
Node:  5:228,4:20013,3:55357,2:56832,1:214,
```

The baseline ASCII exponent is 2.013 over completed sizes 100–10,000; 100,000
times out. Unicode fails checksum stability at 100 and mismatches at 1,000 and
10,000, so its baseline speed is not classified.

## Final measurements

CPU: AMD Ryzen 5 7640HS, 12 logical processors. LLVM 22.1.8.
Compiler/archive source revision: `0c5348ce5694654d8aa6e4477900ebc9fffe67de`.
`fixed-artifacts.json` records the matching compiler and archive SHA-256 hashes.
All ten original workload/size pairs complete with stable checksums matching Node.

### ASCII

| n | Base Perry ms / status | Fixed Perry ms | Fixed Node ms | Perry / Node | Checksum |
|---:|---:|---:|---:|---:|---:|
| 100 | 0.015718 | 0.005188 | 0.006745 | 0.77x | 464151292 |
| 1,000 | 1.390067 | 0.052812 | 0.080744 | 0.65x | 710929850 |
| 10,000 | 166.934100 | 0.526479 | 0.804260 | 0.65x | 535454277 |
| 100,000 | TIMEOUT | 5.251350 | 7.428233 | 0.71x | 35382078 |
| 1,000,000 | NOT RUN | 52.088600 | 71.593200 | 0.73x | 153135489 |

Fixed log-log slopes over all five sizes: Perry **1.000**, Node **1.002**.

### UNICODE

| n | Base Perry ms / status | Fixed Perry ms | Fixed Node ms | Perry / Node | Checksum |
|---:|---:|---:|---:|---:|---:|
| 100 | ERROR | 0.006948 | 0.010043 | 0.69x | 319467163 |
| 1,000 | 14.891150 (wrong checksum) | 0.070113 | 0.100875 | 0.70x | 431622199 |
| 10,000 | 1736.238200 (wrong checksum) | 0.699286 | 1.004910 | 0.70x | 36132863 |
| 100,000 | TIMEOUT | 7.232333 | 8.955500 | 0.81x | 49951631 |
| 1,000,000 | NOT RUN | 69.962600 | 89.631600 | 0.78x | 481167302 |

Fixed log-log slopes over all five sizes: Perry **1.002**, Node **0.985**.

## Validation and host limitations

- Final native reduction exactly matches Node: `5:228,4:20013,3:55357,2:56832,1:214,`.
- Final compiled boundary/aliasing fixture matches Node, including empty and
  negative bounds, both surrogate halves, lone surrogates, retained aliases,
  captured locals, stride-two reads, and a non-string runtime receiver.
- Final forced-GC fixture matches Node with 1,061 copying minors,
  168 moved objects and 1,055 loop polls; evacuation verification
  and from-space protection are enabled. Two runtime slice GC tests pass,
  including an assertion that the source address actually changes.
- Runtime suite: 3,439 passed, one failed, four ignored (`--test-threads=1`).
  All three new slice/cursor unit tests pass. The failing unchanged
  `emergency_full_trace_is_excluded_from_ordinary_pause_stats` assertion expects
  allocator trimming to be unsupported on this Windows host; it fails in
  isolation too.
- Compiler unit suite (candidate build): 1,460 passed, three failed, one ignored. All three new
  eligibility-analysis tests pass. The unchanged failures are a frameless-entry
  assembly assertion and two native-emission byte-equality assertions; each
  also fails in isolation on Windows. See `validation.json` for exact names.
- The broader 68-case string sweep on the initial candidate build has 47 passes,
  one parity mismatch, 19 compile failures, and one skip. Every compile failure
  reports the existing Windows RS4GC/WinEH restriction (#7354). The mismatch is
  `test_gap_tolocalestring_locale_options_9414`: Node's default locale is German
  on this host, while Perry formats default-locale rows as English. All three
  focused native fixtures were recompiled and rechecked on the final build.
- Test registration, Node-version consistency, GC root-holder/store/address
  inventories, local-binding proof audit, architecture checks, and public
  baseline harness tests pass. The Rust file-size gate passes. Recursive
  `rustfmt --check --edition 2021` on both changed crate roots passes; the
  workspace-wide `cargo fmt` invocation exceeds Windows' command-line limit.
- The quick pre-tag gate's published-benchmark freshness check remains red.
  Its fingerprinted inputs are identical to pristine base (recorded in
  `public-baseline-check.json`); this PR does not regenerate that unrelated
  published artifact. These host/gate limitations are reported, not counted
  as passing checks. Linux/macOS and full workspace checks were not run locally.

Re-run the affected suites with `cargo test --release --locked --lib -p
perry-runtime -- --test-threads=1` and `cargo test --release --locked --lib -p
perry-codegen`. Use the same build environment as above. The corpus sweep was
`bash run_parity_tests.sh --filter string` with `PERRY_SKIP_BUILD=1`, `PERRY_BIN`
and `PERRY_RUNTIME_DIR` pointing to the matching build. No version files change.
