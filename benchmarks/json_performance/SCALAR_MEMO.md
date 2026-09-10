# Scalar projection R2: memoization and dispatch experiment

Status: **rejected for landing**. R2 preserves the smaller-array scan wins and
fixes much of R1's reuse penalty, but five CPU rows have slower medians with
nonoverlapping sample ranges. Two native-checker findings also remain open.
Measured implementation: `46e9b34c62edd97463b192f64f377fd4e705eebd` (full SHA in the linked provenance).
No PR has been opened for this experiment.
R1's measurements and rejection remain in [SCALAR_PROJECTION.md](SCALAR_PROJECTION.md).
Reference main is `53df2c671fff33b1f8f624432372c2401db8b1d0` (0.5.1530).

R1 removed record construction on scalar scans but made every reuse control
slower. R2 addresses both causes: save scalar results and put the probe behind
the existing dynamic index dispatcher's ordinary Array/Object branches.

## Cache contract and consumer audit

`LazyArrayHeader.materialized_bitmap` still exclusively identifies exposed
elements. With a bit set, the corresponding value is the authoritative record
or array element and must preserve identity, mutations, getters and prototypes.
With a bit clear, a nonzero slot can memoize a number, boolean or null for one
fixed property. The header stores that property's exact encoding (length plus
up to seven ASCII bytes), never a pointer or a lossy hash. Another property
takes ordinary indexing. A failed probe does not select a property.

Existing slots start zeroed. The scalar +0 is stored as tagged int32 zero and
decoded back to f64 +0; -0 retains its distinct bits. No additional bitmap,
cache allocation, initialization pass, managed reference or GC policy is added.
The header gains one u64. The inline cache path is limited to 64-bit targets;
the runtime helper uses an explicit u64 length matching its declared ABI.

The complete search of production readers/writers found:

| Consumer | Required behavior retained |
| --- | --- |
| `gc/layout_slot_visit.rs`, LazyArray descriptor | Select slots only by bitmap; memo values do not become GC edges. |
| `json_tape.rs`, `lazy_cached_count` | Count exposed elements only; scalar hits do not trigger batch construction. |
| `json_tape.rs`, `lazy_get` | Read a slot only when exposed; otherwise construct its record, overwrite the memo, barrier it and set its bit. |
| `json_tape.rs`, batch reparse overlay | Overlay only exposed elements, retaining existing mutations and identity. |
| `json_tape.rs`, slow full materialization | Use bitmap-selected values; reconstruct every unexposed record from the tape. |
| `json/stringify_api.rs`, lazy stringify | Only exposure bits force materialization; scalar reads leave original JSON authoritative. Existing canonicalization limitations remain. |
| `json_tape/mutation.rs`, indexed setters/deletion | Materialize before mutation; the materialized pointer closes memo access. |
| `object/object_ops/define_property.rs` | Materialize lazy receivers before installing indexed values or accessors. |

The codegen path checks lazy brand, forwarding (in the existing dispatcher),
magic, materialization, bounds and descriptors before any memo or exposed slot
load. Exposed slots feed the existing property PIC. Scalar hits bypass record
construction and that PIC. Cold scalar reads call the noncollecting runtime
probe; misses keep the ordinary dynamic index helper. IndexGet's specialization
order is preserved by passing an optional probe down to its dynamic tier.

## Measurements

Quiet M1 Mac mini (8 GiB), Node 26.5.1 and Bun 1.3.14. Each row has seven fresh
process trials per engine, interleaved with identical work counts. CPU is user
plus system time per operation. Peak RSS is the process peak, including startup.

Both windows passed the load <= 2.5 / no-competing-workload checks:

- Reuse: 2026-09-10 11:41:17–11:41:41 UTC; 336 timed trials, 12 full-count Node
  oracles; one million iterations per trial, zero warmup. Parse happens before
  the timed region. [Window](results/quiet-scalar-projection-r2-access/window.json),
  [raw timings](results/quiet-scalar-projection-r2-access/timing.jsonl).
- Focus: 2026-09-10 11:43:12–11:45:52 UTC; 280 timed trials and 50 verification
  outputs. Every timed checksum was checked against its complete iteration and
  warmup count. [Window and exact cases](results/quiet-scalar-projection-r2-focus/window.json),
  [raw timings](results/quiet-scalar-projection-r2-focus/timing.jsonl).

All 616 timed checksums match their Node oracle. The medians below were
independently recomputed from the archived raw trials.

### Parse, scan and stringify CPU (microseconds per operation)

| Fixture | Operation | Main | R2 | Node | Bun | R2 vs main |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| records_array_16k | scan | 58.41750 | 21.95650 | 40.32825 | 35.50475 | -62.41% |
| records_array_1m | scan | 2897.43000 | 1363.10500 | 2745.33500 | 2200.83000 | -52.95% |
| records_array_8m | scan | 23696.28125 | 11866.18750 | 31594.37500 | 21562.93750 | -49.92% |
| records_array_20m | scan | 54337.75000 | 54711.93750 | 85484.18750 | 51823.43750 | +0.69% |
| records_array_20m | roundtrip | 104848.25000 | 105230.25000 | 97371.37500 | 68657.12500 | +0.36% |
| records_array_1m | parse | 916.18500 | 910.90500 | 2724.51000 | 2151.11500 | -0.58% |
| records_array_1m | stringify | 640.69531 | 640.69531 | 838.50781 | 957.60547 | +0.00% |
| small_record | parse | 0.09663 | 0.09634 | 0.33346 | 0.24336 | -0.30% |
| small_record | stringify | 0.04213 | 0.04214 | 0.10776 | 0.11803 | +0.02% |
| long_string_1m | stringify | 31.46167 | 34.43774 | 99.15210 | 92.79468 | +9.46% |

The 16 KiB, 1 MiB and 8 MiB scans use 62.4%, 53.0% and 49.9% less CPU than
main, respectively. Their complete sample ranges are below both Node and Bun.
The 20 MiB scan (+0.69%) and round-trip (+0.36%) regress with separated ranges.
Round-trip still takes 1.53 times Bun's CPU. Long-string stringify's +9.46%
median has overlapping ranges; it is unresolved variability, not a qualified
stringify improvement. No full 38-row or rotating/retention rerun was performed.

### Scan peak RSS (MiB)

| Fixture | Main | R2 | Node | Bun |
| --- | ---: | ---: | ---: | ---: |
| records_array_16k | 216.14 | 63.03 | 61.92 | 77.34 |
| records_array_1m | 413.72 | 67.17 | 130.22 | 99.58 |
| records_array_8m | 530.25 | 108.88 | 313.58 | 138.59 |
| records_array_20m | 691.44 | 691.48 | 529.38 | 325.81 |

At 1 MiB, peak RSS falls from 413.72 to 67.17 MiB; at 8 MiB it falls from
530.25 to 108.88 MiB. The 16 KiB scan still uses slightly more RSS than Node.
The 20 MiB path remains around 691 MiB versus Bun's 326 MiB.

### Parse-once reuse CPU (microseconds per iteration)

| Fixture | Pattern | Main | R2 | Node | Bun | R2 vs main |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 16k | repeat | 0.018569 | 0.005009 | 0.002714 | 0.004545 | -73.0% |
| 16k | random | 0.040543 | 0.041599 | 0.006669 | 0.009289 | +2.6% |
| 16k | fields | 0.084496 | 0.040952 | 0.005330 | 0.009243 | -51.5% |
| 16k | sequential | 0.035080 | 0.020802 | 0.003901 | 0.005749 | -40.7% |
| 1m | repeat | 0.018552 | 0.005012 | 0.002754 | 0.004523 | -73.0% |
| 1m | random | 0.048766 | 0.048472 | 0.010581 | 0.010748 | -0.6% |
| 1m | fields | 0.107949 | 0.112641 | 0.010738 | 0.014317 | +4.3% |
| 1m | sequential | 0.040568 | 0.015355 | 0.008241 | 0.007459 | -62.1% |
| 20m | repeat | 0.005639 | 0.005950 | 0.003026 | 0.004814 | +5.5% |
| 20m | random | 0.026199 | 0.025618 | 0.008607 | 0.010042 | -2.2% |
| 20m | fields | 0.027165 | 0.027838 | 0.011628 | 0.014636 | +2.5% |
| 20m | sequential | 0.016035 | 0.015434 | 0.006774 | 0.008251 | -3.7% |

Repeated reads on 16 KiB and 1 MiB inputs improve 73.0%; sequential rescans
improve 40.7% and 62.1%. The remaining separated reuse regressions are 16 KiB
random (+2.6%), 1 MiB mixed fields (+4.3%) and 20 MiB mixed fields (+2.5%).
The 20 MiB repeated-read median is +5.5%, but its ranges overlap. These controls
still trail Node and Bun in CPU; fixing regressions versus Perry main does not
establish parity with either engine.

### Parse-once reuse peak RSS (MiB)

| Fixture | Pattern | Main | R2 | Node | Bun |
| --- | --- | ---: | ---: | ---: | ---: |
| 16k | repeat | 13.05 | 13.06 | 57.77 | 34.83 |
| 16k | random | 13.42 | 13.52 | 57.84 | 35.44 |
| 16k | fields | 13.17 | 13.28 | 57.97 | 35.98 |
| 16k | sequential | 13.17 | 13.08 | 57.77 | 35.28 |
| 1m | repeat | 17.56 | 17.61 | 63.72 | 37.95 |
| 1m | random | 18.48 | 18.58 | 66.59 | 38.92 |
| 1m | fields | 18.27 | 18.39 | 66.75 | 40.31 |
| 1m | sequential | 18.27 | 17.61 | 66.58 | 39.20 |
| 20m | repeat | 96.92 | 97.05 | 194.70 | 93.94 |
| 20m | random | 96.92 | 97.06 | 194.80 | 94.77 |
| 20m | fields | 96.92 | 97.08 | 195.00 | 95.34 |
| 20m | sequential | 96.94 | 97.08 | 194.81 | 94.59 |

## Validation and limits

- 301 JSON runtime tests and 10 JSON compiler tests passed. Additional compiler
  selections passed: 13 index-get, 25 property-get and 15 GC call-effect tests.
- The compiled fixture's 28 output lines match the pinned Node oracle in all
  nine parser/GC combinations. Each scheduled arm exercised 689 protected
  from-space retirements: 28,526 objects moved in auto/tape mode and 83,287 in
  direct mode. Nine mutation cases and 12 access checksum controls also passed.
- The native benchmark workers pass: 18 functions, 582 statepoints and 1,239
  relocates, no hazards or suppressions. Both shadow checks pass for all three
  modules. Actual scalar probe counts are 49 in the fixture, 12 in the access
  worker and 4 in the original worker; the calls are not statepoint callees.
- The full native fixture reports **two unresolved hazards**: the R1 accessor
  literal temporary across `js_closure_unbox_callee_checked`, and a global
  string argument across `js_get_string_pointer_unified`, exposed by the new
  `JSON.stringify(memo) === source` assertion. Three smaller controls did not
  reproduce the string finding. The relevant comparison lowering is unchanged
  from main, but an actual main-compiler reproduction has not established its
  inheritance. No exemptions were added, and full native validation is not green.
- The first script-lint pass was 72/74: public-baseline freshness plus one new
  open-coded string offset in a unit test. The test was changed to use the
  canonical string accessor and that inventory gate now passes. The compilation
  lint tier was skipped (73/74 script gates pass after the repaired gate rerun). The test-only follow-up reran all 301 JSON runtime tests successfully.

Measurement provenance is retained with each window:
[focus](results/quiet-scalar-projection-r2-focus/provenance.json) and
[reuse](results/quiet-scalar-projection-r2-access/provenance.json).
The source patches are losslessly gzipped; adjacent compression manifests
preserve original and compressed SHA-256 hashes. Validation evidence lives in
[the validation manifest](results/scalar-projection-r2-validation/manifest.json).
Later test/report changes do not relabel the frozen measured binaries.

## Next experiment

Route fully materialized lazy arrays into the existing ordinary-array guard,
sharing its bounds, forwarding, descriptor and prototype checks. That should
remove the added guard cost on the post-materialization path. Separately inspect
ordinary-array mixed-field code generation: the 20 MiB mixed-field regression
persists even though ordinary arrays avoid the scalar helper call. Do not assume
that moving a call off the hot path makes instruction placement or register
pressure free. Remeasure these controls before widening the benchmark matrix.
