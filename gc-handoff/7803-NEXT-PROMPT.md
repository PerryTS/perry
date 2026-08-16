# Task: close the LAST #7803 window (seed 3)

Sessions 1–3 background: `gc-handoff/ZOD-NOTES.md` §1–§36. Session 4
(2026-08-15) is §37–§40 and CHANGED THE PICTURE COMPLETELY — read §39 and
§40 before anything else. This file replaces its previous self.

## State in five lines

The main #7803 root cause is FOUND and FIXED: the compact GC map collapsed
RS4GC (base, derived) pairs on the false premise "Perry has no interior
pointers"; for-of element cursors were walked as object starts and never
rewritten as base+delta (gc_map v4 + derived-aware walkers, on the branch).
Seeds 1, 2, 5 flip to clean (pre-fix ~2/3 aborting, fixed ordinals). ONE
window remains: seed 3, 3/3 abort, characterized in §40 to the exact slot.

## The residual, precisely (all one-run reproducible)

```
cd /Users/amlug/projects/perry/wt-7803
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
PERRY_RUNTIME_DIR=$PWD/target/release PERRY_NO_AUTO_OPTIMIZE=1 \
PERRY_DISABLE_BUILD_CACHE=1 PERRY_KEEP_SYMBOLS=1 \
  ./target/release/perry test-files/gc-dep-corpus/main.ts -o /tmp/zod

# CREATION-cycle abort in ~seconds-to-minutes (collection #186):
PERRY_GC_SCHEDULE_SEED=3 PERRY_GC_SCHEDULE_RATE=0.1 \
PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=0 \
PERRY_GC_NATIVE_SLOT_VERIFY=1 /tmp/zod
```

Facts the instruments already pinned (§40): victim slot = SP+40 of
`schemas_ts__138` at its `+0xEA0` js_closure_call1 statepoint (the this-save
around that call; record exists, lists the slot, has no derived entries);
creation = an ORDINARY traced copy-minor whose rewrite walk traversed the
frame (frames=20/records=7/locations=36); the slot value at creation = a
boxed POINTER_TAG interior into a strings array (target-8 reads as boxed
strings); `collector_classify=None` (plausible_gc_header rejects interiors)
vs global Survivor1=from — so the rewrite silently skips, forever. The
two-sided this-set trap proves the interior appears in the slot between the
save and restore WITHOUT passing through js_implicit_this_set.

## The open contradiction to break (start here)

A value saved coherent, in a walked slot, reads as a boxed interior at the
first in-suspension collection. Either (a) the SAVE stores a different
register/slot than the map attributes at +0xEA0 (slot/liveness attribution),
or (b) an earlier-suspension record's walk rewrote this stack address under
another interpretation. Next instrument (one edit in
`gc/roots/stack_maps.rs::verify_native_slots_post_walk`): on the failing
cycle dump ALL slot values of the matched record(s) (16/24/32/40/48) AND the
full list of records `match_records` returned — adjacent records within the
±16 window exist in this function (+0xfd4/+0xfd8 pairs) and a double-match
walking one frame under two records is unaudited.

## Instruments on the branch (all default-off, all one-run)

| knob | what it does |
|---|---|
| `PERRY_GC_NATIVE_SLOT_VERIFY=1` | abort at the CREATION cycle of a stale native slot, with cycle kind, rewrite-walk stats, collector classification, raw target header |
| `PERRY_GC_THIS_SET_CHECK=1|abort` | trap incoherent implicit-this values, both directions (incoming = frame slot corrupted, outgoing = cell corrupted) |
| pin-latch (always-on) | names owning frame/reg/offset/slot, raw slot word, target neighborhood, census-backed ENCLOSING object |
| `PERRY_GC_FROMSPACE_SCAN(_ABORT)` | now bounded at array length (§38 false positive fixed) |

## Traps that cost this session time — do not repeat

* Seeds do NOT port across binaries, and detection is an address lottery on
  top of a deterministic schedule window. Compare rates and windows, never
  single runs; the CREATION-cycle verifier removes the lottery entirely.
* The scan/latch "garbage headers" are just NaN-boxed words at `addr-8`:
  they do NOT discriminate stale-into-recycled from interior-into-live.
  §37's confident spray story died on that; so did the §38 slack false
  positive. The enclosing-object dump is the discriminator.
* `chain_walkable=false` on these binaries (x19-based roots) — every walk is
  the Itanium unwinder; fp-chain hypotheses are dead on arrival.
* Wrapper exit codes lie: `| head` SIGPIPEs long runs, `| tail` hides script
  failures. This session hit both. Grep to files.

## Chores before undrafting PR #8084

* Gap suite on THIS tree (session 4 ran it — check the result landed in
  §40/§41; rerun quiet if not): `PERRY_SKIP_BUILD=1 ./scripts/run_gap_tests.sh`.
* `scripts/gc_root_dominance_*.sh` reader fix (5f76bf5c7) also fixes MAIN's
  red nightly — consider cherry-picking it out as its own fast PR.
* The corpus budget in `gc-root-dominance.yml` is `--max-unrooted 3`; after
  the spread-new fix the residuals are 185 (`rel_ge`) + util 121 (read-only
  sinks) — tighten to 2 once re-measured.
* §33's 36-site `js_native_call_method` args_ptr population is still open
  (unrelated to the residual; separate issue recommended).
