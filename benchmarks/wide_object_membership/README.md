# Wide-object membership and `Object.assign` (#10059)

Current main at `603b074ace01464bc66fc07cc8d532f26ccf5a0f` reproduces both reported
problems. The three TypeScript workloads in this directory are byte-identical
to the issue attachments. The baseline and fixed sweeps use the same source
bytes, Node v26.5.1, sequential size order, and matched Perry compiler/runtime
archives.

## Change

`own_key_present` now delegates to the shared keys-array lookup. A complete
shape index can prove both presence and absence, so a missing destination key
in `Object.assign` does not fall through to a scan of every preceding key.
When the index is missing, shortened, or otherwise incomplete, lookup retains
the dense-slot fallback. The keys-array length and slot indexes are already
`u32`, so removing the unrelated 65,536 guard does not widen their
representation.

`own_key_present_via_index` also consumes the index's explicit `Found`,
`Absent`, and `Unindexed` verdicts. This preserves the exact fallback for an
untrusted miss rather than silently treating an index that declined to answer
as authoritative.

## Method

- Apple M1 Max, 10 logical cores, macOS 26.5, arm64; Node v26.5.1; Perry
  0.5.1532.
- Compiler and both matching static archives built together with
  `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build --release --locked -j 4
  -p perry -p perry-runtime-static -p perry-stdlib-static`.
- Each source compiled with `--no-auto-optimize --no-cache` and an explicit
  matching `PERRY_RUNTIME_DIR`.
- Sizes run in their issue order. Each process keeps the original warmup (at
  least 200 ms and five runs), seven samples of at least 20 ms, and a 60-second
  timeout.
- The host was busy and load varied materially. Exact constant-factor ratios
  are descriptive; the checksum repair and slope classification are the
  useful results.

Baseline load average started/ended at `[11.49, 25.14, 30.11]` / `[24.25,
25.97, 30.06]`; fixed at `[23.43, 25.68, 29.88]` / `[43.52, 32.26, 31.92]`.

## Membership correctness

Times are median milliseconds. A `no` checksum match is the reported cutoff,
so that timing is not evidence of correct performance.

| workload | n | Node before | Perry before | checksum | Node after | Perry after | checksum |
|---|---:|---:|---:|:---:|---:|---:|:---:|
| `in` | 100 | 0.0038 | 0.0411 | yes | 0.0134 | 0.0680 | yes |
| `in` | 1,000 | 0.0462 | 0.3969 | yes | 0.0937 | 0.5921 | yes |
| `in` | 10,000 | 0.6241 | 4.1646 | yes | 1.6149 | 8.1271 | yes |
| `in` | 100,000 | 8.2211 | 59.4115 | **no** | 22.6291 | 127.2595 | yes |
| `in` | 1,000,000 | 145.1689 | 653.3018 | **no** | 254.7871 | 2,756.4532 | yes |
| `hasOwnProperty` | 100 | 0.0069 | 0.0890 | yes | 0.0143 | 0.1259 | yes |
| `hasOwnProperty` | 1,000 | 0.0780 | 2.5566 | yes | 0.2123 | 1.5102 | yes |
| `hasOwnProperty` | 10,000 | 0.9632 | 207.5987 | yes | 4.2234 | 15.4140 | yes |
| `hasOwnProperty` | 100,000 | 14.2578 | 47.8556 | **no** | 39.2262 | 184.3443 | yes |
| `hasOwnProperty` | 1,000,000 | 442.9680 | 876.0386 | **no** | 546.1051 | 3,357.0082 | yes |

After the fix, all membership checksums match Node through one million keys.
The fitted Perry-minus-Node exponent delta is `0.061` for `in` and `-0.049`
for `hasOwnProperty`, consistent with linear work in the number of probes.

The focused boundary fixture separately confirms Node-equivalent results on a
65,537-key object for present values, present `undefined`, missing and inherited
keys, short and Unicode names, deletion/reinsertion, and direct reads. On the
base build, every own membership result becomes false while direct reads remain
correct.

## `Object.assign`

| n | Node before | Perry before | ratio | Node after | Perry after | ratio |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 0.0298 | 0.1454 | 4.89× | 0.1692 | 0.1790 | 1.06× |
| 1,000 | 0.3838 | 6.4520 | 16.81× | 0.8589 | 2.9133 | 3.39× |
| 10,000 | 9.1615 | 528.0727 | 57.64× | 16.3402 | 30.1276 | 1.84× |

The Perry-minus-Node fitted exponent delta changes from **0.536 to 0.121**,
below the issue's 0.25 target. All copied-value checksums match Node.

The compiled boundary fixture also crosses the former cutoff before exercising
strict-set behavior. It matches Node byte-for-byte for non-writable data
properties, getter-only and setter accessors, source getter/setter order,
symbols, and rejection of new keys on a non-extensible target. The base build
fails the non-writable-property case because the false membership answer makes
the precheck treat it as a new property.

## Artifacts and reproduction

The pristine base artifact hashes are:

```text
perry                  63cb83cf87044443184687ac5eee3376a21dd46c7a1cbf3074789926f99d7b36
libperry_runtime.a     3c0467d1d7ebcd8bd53e98c43e027ff7e93b306024f68b9ba453f383fb8a6fb4
libperry_stdlib.a      a13fd68f3b01f814c7fcd031f57b34e978bc00612317f3c20bef68928f7067af
```

The fixed artifact hashes are recorded in `fixed.json`; its `artifact_source`
contains the exact runtime diff SHA-256 used for the build. `baseline.json` and
`fixed.json` retain all medians, run counts, checksums, source hashes, ratios,
slopes, host metadata, and load averages.

Run a sweep with:

```sh
python3 benchmarks/wide_object_membership/measure.py fixed \
  --compiler target/release/perry \
  --runtime-dir target/release \
  --artifact-source "$(git rev-parse HEAD)" \
  --require-checksum-match \
  --output target/wide-object-membership/fixed
```

## Validation

- Runtime unit suite: 3,525 passed, 4 ignored, zero failures, single-threaded.
  This includes the new exact-boundary test and existing object membership,
  descriptor, enumeration, tombstone, shape-index, and moving-GC coverage.
- The new 65,537-key compiled fixture matches Node byte-for-byte. A broader
  object-named parity sweep completed 19/19 fixtures with no mismatches,
  compilation failures, or crashes before an unrelated ext-wrapper fixture
  requested its own fresh feature build; the sweep was stopped there to keep
  the already-validated compiler and archives coherent. The focused fixture
  and all runtime tests completed independently.
- Test-registration and GC runtime-root-holder audits pass.
- `pre-tag-check.sh --quick` passes every check except the pre-existing public
  benchmark evidence freshness gate. Its registered public source/harness
  inputs are unchanged by this branch, as are all version and release metadata.
