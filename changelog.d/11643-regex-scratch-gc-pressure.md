perf(regex,gc): regex match scratch no longer drives full collections, and the nursery powers on small (#11549).

A pattern with more than 32 match registers, such as dotenv's 42-register line
pattern, could not use the thread's lent scratch cell, which held a fixed 32
registers. Every search of it built owned `MatchBuffers` and freed them again,
and each `perex_memory::Buffer` reported its bytes through
`gc_note_external_side_alloc` / `gc_note_external_side_free`. The free side
feeds `GC_EXTERNAL_SIDE_DRAINED_SINCE_FULL`, the released-bytes term of
old-reclaim pressure, so a hot loop of such searches accumulated phantom
pressure that matched no garbage on the heap and kept scheduling full
collections.

- The lent cell's registers are a growable per-thread `Vec`, sized to the
  largest program the thread has searched (capped at 4096 registers, 32 KiB).
  A search that fits no longer constructs or frees anything. Like the cell's
  frames and undo entries, this memory is the thread's and is not reported to
  the collector.
- `perex_memory::Buffer`, the operation-scoped compile and match scratch that
  the owned path still uses (nested searches, programs over the cap), is
  accounted as transient: new `gc_note_external_transient_alloc` / `_free`
  move only the live external-byte term. The bytes are still visible to
  pressure while they exist, but they no longer step the allocation-churn
  trigger or count as released pressure, and noting them never collects.
  `Reservation` and the replacement span/piece vectors, whose size follows the
  subject, keep the ordinary external accounting.

That alone was measured and not shipped before, because once the phantom
fulls stop, the young generation runs to its 16 MB cap before collecting and
peak RSS rose (+23% dotenv, +21% moment here). The second half keeps RSS flat:

- The scavenge nursery cap now **powers on at a quarter of the base** (4 MB at
  the default 16 MB, `NURSERY_CAP_SHRINK_SHIFT`), grows back through the
  existing debounced rule when survivor influx exceeds 4% of the cap (to the
  base within four minors, then the ×2/×4 scale as before), and shrinks back to
  the floor while influx stays under 1%. A copying minor is O(survivors), so a
  small Eden costs little where little survives, and peak RSS is set by the
  first nurseries: a shrink that only engaged later could not lower it
  (measured: 68.5 → 66.5 MB on dotenv). The tenured-proportional term
  (`old / 2`) is unchanged, so a large old generation still gets a large Eden.
- The #8122 allocation census now seeds at half the power-on cap rather than
  half the base, so it still runs before the first minor it exists to precede.

Measured (`callgrind` instructions per iteration fitted over n=2,500→10,000;
peak RSS by `getrusage` at 5k / 20k / 80k iterations; x86-64,
`PERRY_NO_AUTO_OPTIMIZE=1`; output identical to Node):

| | main instr | branch instr | main RSS (MB) | branch RSS (MB) |
|---|---:|---:|---|---|
| dotenv/parse | 3,293,191 | 2,259,517 (**−31.4%**) | 55.2 / 55.3 / 55.1 | 52.1 / 52.0 / 52.1 |
| moment/parse_format | 2,506,824 | 1,994,023 (**−20.5%**) | 65.2 / 65.2 / 101.6 | 62.7 / 69.6 / 98.0 |

A retaining program pays a few extra early minors while the cap climbs back
to the base. On a loop that keeps one object in ten, total instructions were
27% and 22% below main at 500k and 1M iterations and within +0.5% to +1.5% of
it at 2M and 4M, with peak RSS within ±5%. The climb stays debounced on
purpose: a program's first minor sees its start-up data survive (about 10% of
a 4 MB nursery on dotenv), and an undebounced jump to the base on that one
reading put dotenv's peak RSS back at 62 MB.

Not fixed here, and not new: under the generational collector moment's RSS
still climbs with N on main as well as here (flat at 62 MB with
`PERRY_GEN_GC=0`), so something native is released only by full collections.
The phantom fulls used to hide part of it; it wants its own issue.

Tests: `a_search_over_thirty_two_registers_borrows_the_lent_scratch` (64 of 64
searches took the owned path before; 0 now),
`regex_scratch_buffers_do_not_count_as_released_external_pressure` (a freed
8 KiB buffer added 8,192 bytes of released pressure before; 0 now) and
`the_nursery_starts_at_a_floor_and_follows_survivor_influx`, each
sabotage-checked against the unfixed code; five existing tenuring/trigger tests
updated to the new power-on cap with their intent kept; and
`test_gap_11549_regex_large_register_scratch` (large, nested and interleaved
programs; byte-identical to Node). No version bump.
