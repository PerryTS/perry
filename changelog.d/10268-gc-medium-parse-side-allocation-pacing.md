### Fixed

**GC: a `JSON.parse` loop over medium documents retained ~32 MB of dead tape per cycle.**

`records_array_16k:parse` was the one row of the 50-cell JSON matrix still losing
on peak RSS: **74.98 MiB against Node 26.5.1's 65 and Bun's 71** on the bench
mini. It now peaks at **48.86 MiB** — 25 % under Node — with CPU 2.8 % *better*
than before.

**Where the bytes were.** A lazily-parsed document's memory is not in the arena.
Parsing the 13 197-byte `records_array_16k` fixture puts ~1.1 KB in the nursery
(the `LazyArrayHeader`, its sparse cache and bitmap) and ~24 KB of tape in a
`json_tape_store` side allocation. Every pacing input a parse boundary reads is
denominated in *arena* bytes, so the young generation saw 1/24th of what the
process was holding. `PERRY_GC_DIAG=1` over the row's 11 284 iterations at
`origin/main`: **eight collections, every one a full mark-sweep from
`alloc_point_old_reclaim`**, each firing at `external_side=33.6 MB` with
`arena_total` between 3 and 8 MB, `old_in_use=0`, and `from_space` never above
6.9 MB against a 16 MB nursery cap. The row's only pacing was the old-reclaim
growth band happening to read those side bytes — i.e. 32 MB of dead tape per
cycle.

**The fix, in two halves.**

* A third arm on `tiny_parse_generational_collection_due`, keyed on
  `external_side_live_bytes()` with the `max(floor, baseline)` growth band
  old-gen reclaim already uses, based at the reading the last collection left
  behind. A futile collection (an old-owned cluster whose tape survives)
  re-bases the band at the surviving value, so repeats space out geometrically
  instead of livelocking.
* The band's counterweight: all reported side-buffer releases stay in the
  old-reclaim pressure term until the next full
  (`external_side_old_reclaim_pressure_bytes`). Only a full returns arena
  capacity, and on these rows the external term was paying for that too;
  draining it with cheap minors alone took `records_array_1m:sparse` from seven
  fulls to one, the arena's dirty pages from 29 MB to 55 MB, and peak RSS from
  63.5 to 73.6 MiB *even though live external bytes had halved*. The
  cumulative term restores the full cadence on that measured workload. It
  also includes mutator-side releases (tape materialization, regex scratch,
  native-addon adjustments and buffer replacement), so it can trigger fulls
  earlier than the previous live-only term on other workloads.

**Measured** on the bench mini, 9 interleaved rounds, best of each, all 50 cells
of the JSON matrix (peak RSS, MiB / CPU, ms):

| row | main | this | ΔRSS | ΔCPU |
|---|---|---|---|---|
| `records_array_16k:parse` | 74.98 / 155.2 | 48.86 / 150.8 | **−34.8 %** | −2.8 % |
| `records_array_16k:sparse` | 65.17 / 149.5 | 44.48 / 142.5 | −31.8 % | −4.7 % |
| `records_array_8m:roundtrip` | 129.00 / 146.7 | 99.16 / 139.6 | −23.1 % | −4.8 % |
| `records_array_16k:roundtrip` | 63.72 / 143.5 | 51.12 / 152.0 | −19.8 % | +5.9 % |
| `records_array_8m:parse` | 97.84 / 134.2 | 85.44 / 131.7 | −12.7 % | −1.9 % |
| `records_array_8m:sparse` | 98.39 / 132.7 | 86.00 / 129.2 | −12.6 % | −2.6 % |
| `records_array_1m:roundtrip` | 61.36 / 162.9 | 58.23 / 165.5 | −5.1 % | +1.6 % |
| `numbers_1m:parse` | 62.81 / 160.7 | 60.28 / 164.8 | −4.0 % | +2.6 % |
| `records_array_1m:parse` | 65.78 / 157.7 | 64.75 / 158.9 | −1.6 % | +0.8 % |
| `heterogeneous_1m:parse` | 59.05 / 160.3 | 62.52 / 159.4 | **+5.9 %** | −0.6 % |

The remaining 40 rows move by less than 1 % on both axes. `heterogeneous_1m:parse`
is the one row that grows: its arena capacity goes 5.24 → 8.39 MiB (one extra
live non-general block plus one in-place promoted block) for a live-external
reading that falls — the standing cost of running copying minors on a row that
previously ran none — while its nine full collections are preserved exactly.

`gc_ratchet` (14 probes, 7 repeats each, `main` vs this): correctness passes on
all 14, and **`heap_used_bytes` and `heap_total_bytes` are bit-identical on every
probe** — the gated retention counters do not move. Peak RSS (min of 7) stays
within ±0.5 %, the largest being `03_cross_gen_writes` 25.25 → 25.38 MiB
(+0.13 MiB) and `01_nursery_churn` 27.17 → 27.30 MiB; wall clock runs from
`08_map_set_sidetables` −5.1 % to `06_string_retention` +1.2 %.
