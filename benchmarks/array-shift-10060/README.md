# Array.shift queue drain (#10060)

`array-shift-queue.ts` is the complete standalone source embedded in
[#10060](https://github.com/PerryTS/perry/issues/10060), including its original
driver. `array-push.ts` is an ordinary push control using the same seeded input,
driver, and ordered checksum helpers. Both workloads use the same executable logic before
and after the change.

## Representation and memory cost

The eight-byte `ArrayHeader` still contains length and capacity. Capacity now
counts available slots from **logical element zero to the physical allocation
end**. The existing GC allocation size gives physical capacity; their difference
is the queue front offset. Runtime and generated indexing share this formula.
Native bindings use the existing `perry_ffi::js_array_get` accessor; general array
elements can no longer be read by assuming a fixed eight-byte header offset.
Fresh compiler-created rest arguments and internal shape-key arrays remain
unshifted.

A dense shift reads and clears one slot, decreases length and remaining capacity,
and invalidates element-shape evidence. It performs no survivor copy, allocation,
or per-survivor layout/barrier rebuild. The final shift resets capacity to the
full physical capacity, allowing empty-array reuse. A complete dense drain is
linear in the number of elements, rather than repeatedly processing lengths
`n-1, n-2, ...`.

GC enumeration uses the logical live range. Pointer-free and all-pointer layout
proofs survive a shift; index-specific mixed masks become UNKNOWN and trace the
live slots. Removed physical slots contain HOLE, so retaining the allocation does
not retain the removed values. Survivor addresses stay fixed and existing
old-to-young dirty-page coverage stays valid. Growth copies the logical range to
a fresh unshifted allocation, transfers layout state, and replays barriers;
shifted sources cannot use the old address-translation shortcut. Array-growth
forwarding continues to preserve aliases.

There is no extra header word, side table, or allocation for a pure drain. Both
versions retain the original backing capacity throughout a pure drain; consuming
a prefix does not release its bytes to the allocator. The 10,000-element unit
witness keeps the exact original allocation throughout the drain, then restores
its capacity from one remaining slot to 10,000 on empty. This is a retained
capacity accounting claim, not an RSS measurement. Alternating shifts and pushes
can exhaust the remaining tail and grow earlier; growth normalizes storage and
remains geometric in the live capacity requirement. Mixed arrays trade their
indexed tracing mask for a conservative live-slot scan until layout is rebuilt.
Ordinary unshifted accesses also pay the extra allocation-size/capacity loads;
the push control below measures one consequence of that shared representation.

Receivers with indexed descriptors, custom prototypes, sparse storage or
sealed/frozen restrictions take the live property-aware shift path. Indexed
operations happen before the final length write, preserving partial effects and
exception order when length is non-writable.

## Reproduction

Measured on Windows 11 (10.0.26200), x86-64 AMD Ryzen 5 7640HS, 6 cores / 12
logical processors, with Node **v26.5.1**. The before compiler and both archives
were rebuilt from pristine main
`603b074ace01464bc66fc07cc8d532f26ccf5a0f`. The after JSON records the implementation
commit; later commits only add evidence and name the changelog fragment.

Both builds used the same Rust nightly 1.100.0 toolchain and release profile,
with `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`:

```sh
cargo build --release --locked -p perry -p perry-runtime-static -p perry-stdlib-static
python benchmarks/array-shift-10060/run.py \
  --perry target/release/perry --output benchmarks/array-shift-10060/after.json
```

On Windows the compiler path ends in `.exe`; `LLVM_SYS_221_PREFIX` and PATH point
to the local LLVM 22 installation. The driver selects matching runtime archives
beside the compiler, compiles with `--no-auto-optimize --no-cache`, resolves Node's
actual executable past any launcher shim, and runs each process sequentially.
To measure the baseline, pass its separately built compiler and use `before.json`
as the output. JSON includes compiler, runtime, stdlib, Node and queue-source
SHA-256 hashes.

Input setup is outside the timer and resets the PRNG before **every** invocation.
Warmup requires both at least 200 ms of measured work and five runs. Each of seven
samples contains at least 20 ms of measured work; the result is the median
per-invocation time. Every invocation checks checksum consistency, and completed
Node/Perry pairs must match. The 60-second process timeout includes setup, warmup
and sampling. A timed-out engine skips later sizes, exactly as in the issue.
Timer/checksum overhead is included. Timings are evidence from this host, not a
universal Node shift complexity claim.

## Measurements

Times are median milliseconds per complete workload invocation. Raw results are
[before.json](before.json), [after.json](after.json), and [comparison.json](comparison.json).

### Queue drain

| n | Node before | Perry before | Node after | Perry after |
|---:|---:|---:|---:|---:|---:|
| 100 | 0.002051 | 0.016557 | 0.001818 | 0.001971 |
| 1,000 | 0.039113 | 1.388167 | 0.036471 | 0.019170 |
| 10,000 | 0.393892 | 401.922800 | 0.363189 | 0.192403 |
| 100,000 | 519.977100 | TIMEOUT | 394.911100 | 1.933736 |
| 1,000,000 | TIMEOUT | SKIPPED | TIMEOUT | 19.305250 |

The 100,000-element Perry process now completes within the original timeout.
The 10,000-element drain falls from 401.923 ms to 0.192 ms (about 2,089x on
this host). The one-million-element Perry drain takes 19.305 ms. Node retains its
60-second timeout at one million in both sweeps.

Log(time)/log(n) least-squares slopes, with explicit completed size sets:

- Common to **both engines, before and after**, `[100, 1000, 10000]`: Perry
  **2.193 -> 0.995**; Node **1.142 -> 1.150**.
- After only, common `[100, 1000, 10000, 100000]`: Perry **0.998**, Node **1.701**.
- After Perry over `[100, 1000, 10000, 100000, 1000000]`: **0.999**. No Node
  one-million point is used in any fit.

All completed Node/Perry pairs match their order-sensitive checksums. For one
million elements, Perry returns **755413900**, matching an independent untimed
Node traversal of the same seeded sequence; [checksum-reference.json](checksum-reference.json)
contains that reference program. This does not convert the timed-out Node shift
process into a completed benchmark sample.

### Ordinary push control

| n | Node before | Perry before | Node after | Perry after |
|---:|---:|---:|---:|---:|---:|
| 100 | 0.002307 | 0.004118 | 0.001779 | 0.003272 |
| 1,000 | 0.023579 | 0.029980 | 0.019149 | 0.023626 |
| 10,000 | 0.213551 | 0.322097 | 0.191966 | 0.248349 |
| 100,000 | 2.103980 | 3.468786 | 1.919945 | 2.633288 |
| 1,000,000 | 24.169400 | 36.186200 | 21.401300 | 25.561100 |

Common sizes for every control fit are `[100, 1000, 10000, 100000, 1000000]`.
Perry slopes: **0.995 -> 0.983**; Node: **0.999 -> 1.016**. Every checksum
matches. Ordinary push remains linear and shows no slowdown in these runs. Node
also improves in the later sweep, so small constant-factor differences should be
treated as host/run variability on this shared development machine.

## Validation

The release compiler and matching runtime/stdlib archives build successfully.
Tests use the repository's pinned Node v26.5.1. Runtime Rust tests run with one
thread; test-profile overrides are `DEBUG=0`, `OPT_LEVEL=1`, `CODEGEN_UNITS=16`.

- Five new runtime/collector tests pass, including a no-copy 10,000-element
  drain, holes, aliases, growth, empty reuse, actual relocation of mixed-array
  survivors, and old-to-young edges through repeated growth/refill.
- Both new TypeScript fixtures match Node. The queue fixture also passes with
  `PERRY_GC_FORCE_EVACUATE=1`, `PERRY_GC_VERIFY_EVACUATION=1`,
  `PERRY_GC_FROMSPACE_SCAN_ABORT=1`, and `PERRY_GC_DIAG=1`: **12 copying minors**,
  with actual copied objects and no stale-pointer diagnostic. It is registered
  in `test-parity/gc_repsel_corpus.txt`.
- Seven existing array/JSON fixtures match Node: array splice spread/dispatch,
  proxy array mutators, short JSON array storage, primitive JSON arrays, JSON
  array-element overflow fields, and grown-array stringify. The queue covers
  indexing, slice/reverse/copyWithin/fill/unshift/splice/map/concat and length
  changes. The observable fixture covers custom-prototype holes, indexed
  getters/setters, non-writable length, sealed/frozen arrays and exception order.
- All **144 array-related codegen tests** and **33 FFI tests** pass. Production
  `cargo check --lib` also passes separately for `perry-ext-http` and
  `perry-ext-better-sqlite3`, preserving their FFI-only dependency boundary.
- Changed-crate formatting, test registration, GC store-site inventory, address
  classification, GC root-holder audit and its self-test pass. The census pin was
  refreshed after reviewing its unchanged non-moving collector window. No
  recorded raw-handle/unrooted-local debt ceiling was raised.

Broader checks were run and compared with pristine main; they are not claimed
as clean full-suite runs:

| Check | Result on this Windows host | Pristine-main comparison |
|---|---|---|
| Runtime unit suite | 3,440 passed, 1 failed, 4 ignored | Same allocator telemetry failure reproduced independently |
| Full codegen unit suite | 1,457 passed, 3 failed, 1 ignored | Exactly the same tests and counts |
| Standard-library unit suite | 59 passed, then abort | Same young-log assertion at the same test after 59 passes |
| HTTP unit binary | Link failure, 8 missing async/TLS symbols | Same missing symbols |
| better-sqlite3 unit binary | Link failure, 12 missing async/FFI symbols | Same missing symbols |

The runtime failure is
`emergency_full_trace_is_excluded_from_ordinary_pause_stats` (Windows reports
allocator trimming as `executed`, while the test expects `unsupported`). The
codegen failures are `the_clone_entry_is_shrink_wrapped_frameless`,
`split_native_construction_lowers_precise_roots_before_rs4gc`, and
`split_native_construction_propagates_shadow_backend_to_workers`. The stdlib abort
is `listeners_provider_roots_readable_snapshot_across_array_allocation`, asserting
that `closure.dynamic_props` lacks a young-log entry.

Five script-lint entries also fail on pristine Windows main: changeset and release
pipeline self-tests, whole-workspace `cargo fmt` (Windows command-length error
206), public benchmark artifact freshness, and the unrooted-local per-file
baseline check. Formatting each changed crate separately passes. The full
conformance corpus was not run locally. Windows exception-handling fixtures use
`PERRY_RS4GC=0` because the default Windows statepoint backend rejects funclet EH
(#7354); the new GC fixture uses the default backend and forced moving collection.
