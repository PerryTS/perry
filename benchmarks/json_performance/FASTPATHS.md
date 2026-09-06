# First JSON fast-path patch

Base: Perry 0.5.1520, commit `454daac4f8fc667ab4bc85b7b5b36c8bae56ae28`.
Branch: `codex/json-fastpaths-1520`.
Draft PR: [#9849](https://github.com/PerryTS/perry/pull/9849).
Runtime implementation commit: `2a05ac309`.

## Measured results

The final quiet-M1 run passed all 152 output comparisons, 456 timing trials and 432 retained-output memory trials. Each timing cell uses three fresh processes and identical iteration counts across freshly built baseline Perry, patched Perry, Node 26.5.1 and Bun 1.3.14. CPU is process user + system time. The [quiet-window record](results/fastpaths/window.json), [full tables](results/fastpaths/tables.md), [raw-data summary](results/fastpaths/summary.csv), [provenance](results/fastpaths/changed-provenance.json) and [validation record](results/fastpaths/validation.json) accompany these results.

| Input | Parse CPU: baseline → patched | Speedup | Stringify CPU: baseline → patched | Speedup |
|---|---:|---:|---:|---:|
| 7 B object | 0.366 → 0.364 µs | 1.01× | 0.196 → 0.197 µs | 1.00× |
| 109 B record | 0.811 → 0.752 µs | 1.08× | 0.563 → 0.547 µs | 1.03× |
| 1 KB object | 1.759 → 0.588 µs | 2.99× | 1.135 → 0.385 µs | 2.95× |
| 0.89 MB records, eager object envelope | 4.548 → 4.028 ms | 1.13× | 2.363 → 2.224 ms | 1.06× |
| 7.12 MB records, eager object envelope | 35.050 → 31.341 ms | 1.12× | 19.859 → 18.829 ms | 1.05× |
| 17.77 MB records, eager object envelope | 87.357 → 78.276 ms | 1.12× | 101.724 → 98.155 ms | 1.04× |
| 1.05 MB ASCII string | 1.440 → 0.204 ms | 7.06× | 0.981 → 0.207 ms | 4.74× |
| 0.99 MB escaped text | 2.613 → 2.038 ms | 1.28× | 1.904 → 1.904 ms | 1.00× |
| 0.88 MB Unicode text | 2.040 → 0.984 ms | 2.07× | 1.642 → 0.987 ms | 1.66× |
| 1.07 MB numeric array* | 2.265 → 1.575 ms | 1.44× | 7.907 → 7.918 ms | 1.00× |
| 50,000-key object (0.98 MB) | 405.493 → 10.801 ms | 37.54× | 2.768 → 2.508 ms | 1.10× |

*Array-root parse can return a lazy Perry array; stringify always starts with a fully materialized value. The eager object envelopes measure complete record construction. Array-root record parsing improved 24–28% at 0.89–7.12 MB; eager record parsing improved about 12–13%. Tiny JSON is essentially unchanged. Heterogeneous-array stringify measured 1.2% slower; see the complete matrix rather than treating the largest gains as universal.

The numeric-array regression seen in the [first iteration](results/fastpaths-initial/tables.md) is resolved: final numeric-array parse is 1.44× faster than baseline. Numeric stringify remains unchanged and is a priority for the next patch.

Against the other engines (CPU µs per call):

| Workload / operation | Patched Perry | Node | Bun |
|---|---:|---:|---:|
| 7 B object / parse | 0.364 | 0.082 | 0.045 |
| 7 B object / stringify | 0.197 | 0.037 | 0.040 |
| 0.89 MB eager records / parse | 4,027.732 | 2,792.169 | 2,132.577 |
| 0.89 MB records / stringify | 2,224.395 | 842.254 | 970.328 |
| 1.05 MB ASCII string / parse | 204.024 | 370.518 | 68.654 |
| 1.05 MB ASCII string / stringify | 206.698 | 107.323 | 97.802 |
| 1.07 MB numeric array* / parse | 1,575.042 | 3,121.271 | 3,209.760 |
| 1.07 MB numeric array / stringify | 7,918.434 | 1,931.303 | 2,951.000 |
| 50,000 keys / parse | 10,801.000 | 5,586.000 | 4,123.000 |
| 50,000 keys / stringify | 2,507.533 | 6,339.822 | 665.150 |

This closes the catastrophic wide-object gap, but Perry still takes about 1.9× Node and 2.6× Bun CPU there. ASCII-string parse now beats Node in this test while Bun remains faster. Tiny-call overhead, general object construction, number formatting and output copying remain important.

Retained-output RSS was mostly unchanged. These are whole-process MiB after the loop, with all N results still live:

| Retained results | Baseline RSS | Patched RSS |
|---|---:|---:|
| 200,000 tiny parsed objects | 24.91 | 24.83 |
| 16 eager 0.89 MB record graphs | 67.03 | 67.17 |
| 32 parsed 1.05 MB strings | 56.80 | 56.97 |
| 32 stringified 1.05 MB strings | 59.78 | 59.91 |
| 16 parsed 50,000-key objects | 73.61 | 75.62 |
| 16 stringified 50,000-key objects | 44.73 | 45.92 |

The wide-object index adds temporary storage and roughly 2 MiB of RSS in the retained-parse case. Its Rust storage is dropped before the object parse returns; the allocator can retain freed pages. Stringify preparation also parses its input, so that allocation history can affect the stringify RSS measurement. These measurements do not establish a strict memory-neutral contract for the existing caches and lazy-array metadata.

## Implementation

The original [audit](REPORT.md) identified three expensive loops that can be
improved independently of collection scheduling:

- Depth preflight walked every byte of long quoted strings. It now skips quoted
  spans with a bounded vector scanner while retaining the depth guard and its
  behavior on malformed input. Large flat scalar arrays use byte searches to
  prove depth is at most one; quotes or nested container openers fall back to
  the full depth scan. Syntax validation remains in the parser.
- Duplicate detection in wide objects searched every preceding key. Objects with
  more fields after 128 unique keys now use a temporary index of interned key
  identities, with one hash lookup per field. Smaller objects retain linear
  lookup. Output order and duplicate replacement are unchanged. A key-index
  microbenchmark showed that indexing at 32 keys costs more than linear search
  on medium objects; the later threshold avoids that overhead for ordinary
  sizes while retaining linear scaling for very wide objects.
- Stringify scanned ordinary strings bytewise and allocated temporary formatted
  strings for control escapes. It now uses the shared vector scanner, copies the
  ordinary prefix once, and writes six-byte Unicode escapes directly.

The scanners use NEON on AArch64, SSE2 on x86-64, and bounded word loads for tails
and other targets. They never require readable padding beyond the input slice.

## Memory and GC boundary

The target for the plain-data path is: rooted input, local scratch, returned
output. Allocation for the returned object graph or output string is intrinsic;
scratch and bookkeeping should not accumulate with the number of calls.

This patch adds no persistent cache or GC root holder. The wide-object index owns
only temporary Rust storage and borrows key identities already owned by the
existing parse key cache. It is dropped when that object parse returns, while
collection is still suppressed. The scanners allocate nothing, and control
escaping removes temporary allocations.

The existing parse entry/exit hooks, `gc_bump_malloc_trigger`, lazy-array
materialization and collection thresholds are unchanged. Existing retained key
and shape caches, stringify buffer capacity, and lazy-array metadata still need
separate investigation; this patch does not establish a strict memory-neutral
contract for the whole JSON API.

User callbacks require a separate path: getters, proxies, `toJSON`, replacers and
revivers can allocate, mutate inputs and re-enter JSON. Their active values must
remain visible to GC. Suppressing collection across arbitrary user callbacks
would not implement the plain-data contract safely.

A separate pre-existing correctness gap surfaced in the whitespace probe:
`JSON.stringify(JSON.parse("[" + " ".repeat(1024) + "1,2,3]"))` retains the
1024 spaces on the lazy path in both baseline and patched runtimes (1031 output
characters instead of Node's 7). The new flat-array validation fixture copies
the parsed elements into a fresh array before comparing values. The
stringify-only performance worker already starts from fully materialized data.
This patch does not fix lazy serialization's retention of source whitespace.

The follow-up memory contract should be measured in bytes, not just entry counts:
4096 interned names or 256 shapes can still retain a large payload. It should
distinguish live output bytes, temporary peak bytes, retained cache capacity,
and GC metadata. Repeated calls that discard their outputs should reach a stable
memory plateau; holding N results should grow with those N output graphs, not
with an additional history of the parsing work. Returning one result can expose
an entire graph, so the collector still needs enough object metadata to trace
that graph correctly after the call.

## Work after this patch

Re-profile the complete API after the A/B. The original audit points to these
remaining targets:

1. Replace general-purpose float formatting in stringify with an allocation-free
   shortest-number encoder that preserves JavaScript's formatting rules.
2. Reduce stringify's scratch-to-result copy and bound retained buffer capacity.
   Compare a direct output builder with segmented scratch; neither should turn
   a small surviving string into ownership of a much larger temporary buffer.
3. Reduce parse output construction costs: key lookup, shape construction and
   per-value allocation/registration. Prefer block-level accounting where the
   collector's object tracing requirements permit it, with scratch outside the
   returned graph.
4. Resolve the lazy-array materialization and metadata-retention pathology with
   the GC work. A quickly returned tape is not a win if consuming the result
   causes large allocation or collection costs.
5. Build guarded plain-data stringify specialization around stable shapes,
   falling back before invoking observable user code. Tiny JSON needs lower
   fixed entry and object-construction costs; wider SIMD alone cannot supply it.

## Reproduction

Build fresh baseline and changed runtime/stdlib wrappers from isolated checkouts
with the same package set and release settings:

```sh
cargo build --release --locked \
  --config 'profile.release.package.perry-runtime.codegen-units=16' \
  --config 'profile.release.package.perry-stdlib.codegen-units=16' \
  -p perry-runtime-static -p perry-stdlib-static
```

The overrides retain the repository's optimization levels (O3 for the runtime,
size optimization for stdlib) and the release wrappers' thin LTO while reducing
local build time. Record archive hashes and source provenance.

For this controlled runtime A/B, compile `worker.ts` once with the pinned
development compiler's `--no-auto-optimize --no-cache --no-link` flags. Link that
exact same object to each freshly built runtime archive. The worker resolves
entirely from the runtime archive, including its file-input helpers; the stdlib
wrappers were rebuilt as well but are not linked into this workload.

```sh
perry compile worker.ts --no-auto-optimize --no-cache --no-link -o worker.o
cc worker.o "$RUNTIME_DIR/libperry_runtime.a" -lc \
  -Wl,-dead_strip -Wl,-no_exported_symbols -o worker
```

This explicit link step is intentional: the CLI's exact-source stamp check
rejects pairing the pinned development compiler with a different runtime source
tree, even for a runtime-only experiment. No build stamp is modified. The
application object hash, both archive hashes, link commands, source hashes and
output validation establish what the A/B actually measures. It is not a fresh
build of the compiler or a measurement of an official release package.

Stage the executables as `.work/baseline-worker` and `.work/worker`. Generate/stage
the same fixtures as the original audit. On the quiet benchmark Mac, run:

```sh
python3 with_lock.py fastpaths
```

This checks both outputs against Node before measuring both operations on all
19 fixtures, then measures retained-output memory separately. Each timing cell
uses the same iteration count across all four arms and three fresh processes.
Default GC behavior stays enabled. The wrapper refuses a busy benchmark host,
owns a token-scoped lock, and records load before and after the run.

## Validation

The 44 JSON runtime unit tests pass with `RUST_TEST_THREADS=1 cargo test --locked
-p perry-runtime json::`. This includes the new scanner, depth-oracle, escaping
and wide-object tests, plus existing deep-nesting and WTF-8 regressions. The
scanner/escaping leaf tests also pass at O3; the x86-64 scanner tests compile
successfully for Linux but have not been executed on x86-64 in this run.

The GC root-holder inventory and Node-version consistency checks pass. The
repository file-size gate reports two pre-existing violations in
`crates/perry-codegen/src/inprocess.rs` and `crates/perry-hir/src/lower/tests.rs`;
both are byte-identical to the clean baseline. Modified runtime files remain
below the limit. The address-classification inventory passes, with an existing
stale-ratchet notice for `perry-stdlib/src/crypto/kdf.rs`.

The 21 compiled fixtures match Node on both runtimes using the same application objects, including 42 flat-array validation cases. GC stress seeds 17 and 9013 pass with from-space protection and evacuation verification enabled; each arm reports 9/11 copying collections, 19,278/19,101 moved objects and 2,331,144 loop polls. Default GC behavior remains enabled in the performance measurements.
