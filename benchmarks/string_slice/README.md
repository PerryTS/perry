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
