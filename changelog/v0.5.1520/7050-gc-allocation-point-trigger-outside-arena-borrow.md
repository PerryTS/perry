### `fix(gc)`: run the allocation-point GC trigger outside the `&mut Arena` borrow (#7022)

`test_gap_repsel_gc_stress` SIGSEGV'd deterministically in the 12
`gc_repsel_matrix.sh` arms carrying `PERRY_CONSERVATIVE_STACK_SCAN=off` — the
arms in which #7019's default-on evacuating young-gen scavenge is eligible at
all. The fault landed inside mimalloc's `realloc`, reached from the collector's
own `Vec` growth (`ValidPointerSetBuilder::step` → `RawVec::grow_one`): the
malloc heap was already corrupt on arrival.

**Root cause.** `Arena::alloc(&mut self, ..)` called `crate::gc::gc_check_trigger()`
from inside its own borrow. A collection *allocates into the arenas* — promotion
and C4b evacuation call `arena_alloc_gc_old`, an evacuating minor fills a
survivor semispace — and either can reach `Arena::install_fresh_block` →
`self.blocks.push(..)` on the **same** `Arena` the allocating frame is holding.
The `Vec` growth frees the buffer the outer frame goes on to index
(`self.blocks[idx]`, `for i in 0..self.blocks.len()`), and `&mut` carries
`noalias`, so the outer frame is equally entitled to have cached
`blocks.ptr`/`len` across the call.

Instrumented on the reproducer this is direct and frequent: 204 re-entries in a
single run, including `install_fresh_block gen=Old space=Old` while
`Arena::alloc` on the old arena holds the borrow, with `self.blocks`'s length
changing underneath it. The crashing stack is that shape exactly:
`js_array_grow → arena_alloc_gc_old → Arena::alloc → gc_check_trigger →
gc_collect_full_mark_sweep_with_trigger → … → _mi_theap_realloc_zero`.

The hazard predates #7019 but was **latent**: before the moving minor, a
collection triggered from an allocation point did not itself install arena blocks
anywhere near this often. That is why this is a PASS → FAIL correctly attributed
to #7019 without #7019 containing the defect, and it explains the discriminator —
the copying minor is only eligible when the conservative stack scan is off.

**Fix.** `arena_cell_alloc(*mut Arena, size, align)` is the new collecting entry
point: try the current block under a borrow that ends with the statement, run
`gc_check_trigger()` with **no arena borrow live**, then re-derive a fresh borrow
for the slow path. `Arena::alloc(&mut self, ..)` is now collection-free, and the
five thread-local entry points (`arena_alloc`, `arena_alloc_longlived`,
`arena_alloc_old`, `arena_alloc_gc_survivor`, `js_inline_arena_slow_alloc`) route
through it. The two that also touch `INLINE_STATE` keep those borrows short for
the same reason — `Arena::resync_inline_to_current` mutates `INLINE_STATE` from
inside the collection. When the GC runs is unchanged.

**Measured.** Same commit, same compiler, only the four arena files differing;
release build, auto-optimized binaries (with `PERRY_NO_AUTO_OPTIMIZE=1` the crash
does not reproduce at all, which is why the harness deliberately does not set
it); macOS arm64, node 26.5.0; `PERRY_GC_HEAP_LIMIT=8 PERRY_GC_INCREMENTAL=0
PERRY_CONSERVATIVE_STACK_SCAN=off`.

| `PERRY_GC_SCAVENGE_NURSERY_MB` | before | after |
|---|---|---|
| 1, 2, 3, 4, 5, 6, 8, 10, 12, 24, 32, 64 (× 3) | 0 | 0 |
| **16** (the default), × 20 | **139 × 20** | **0 × 20**, byte-exact vs node |
| default, no override, × 5 | **139 × 5** | **0 × 5** |

`PERRY_GC_FORCE_EVACUATE=1`, `+ PERRY_GC_VERIFY_EVACUATION=1` and
`PERRY_GC_FROMSPACE_SCAN=1` on top of the same base: 5/5 crashes before, 0/5
after. `cargo test --release -p perry-runtime --lib -- --test-threads=1`: 1521
passed, 0 failed.

**Matrix.** `scripts/gc_repsel_matrix.sh --arms all --pressure 8`, 440 cells:
`PASS=324 UNVER=91 XFAIL=1 FAIL=24`, from the `PASS=302 UNVER=91 XFAIL=1 FAIL=46`
baseline — **+22 PASS / -22 FAIL, no cell regressed**. `repsel_gc_stress` is PASS
in all 20 arms (was FAIL in 12), and so is `repsel_scalar_replaced_locals`
(#7023, was intermittently FAIL in 11) — the same defect. The residual 24 FAILs
are the two #6981 `p4a3` numarray rows, unchanged. Liveness is intact:
`evac_minor`/`force_evac`/`force_verify` still report `copy-minor 21/22`.

**Regression coverage** (`cargo-test`-visible, per #5960):
`arena::tests::allocation_point_gc_trigger_runs_with_no_live_arena_borrow` pins
that the trigger reached from `arena_cell_alloc` sees an arena-borrow depth of 0
(and that it was reached at all, so the test cannot pass vacuously);
`arena::tests::raw_arena_alloc_method_never_reaches_the_gc_trigger` pins that
`Arena::alloc` stays collection-free, so a refactor cannot move the trigger back
under the borrow. The borrow-depth probe is `cfg(test)`-only — production
allocation pays nothing. Teeth verified by sabotage: reinstating either half of
the old shape turns the matching test red.

**Investigation note.** #7022's dossier classified this as a missing rewrite of
an old→young remembered-set edge, from `PERRY_GC_FROMSPACE_SCAN`'s
`lost_dirty`/`dirty_but_missed` split. That classification was measuring noise:
extending the scan with two axes — is the offender's *owner* itself
`GC_FLAG_FORWARDED`, and is the slot inside the collector's own rewrite
enumeration — shows `enumerated=0` on every cycle of every run, with 99%+ of
offenders being the dead payload of `js_array_grow`'s permanent growth stubs
(#233), the rest uninitialized bytes inside an array's unused capacity (arena
blocks are recycled, not zeroed, and `move_young` copies the whole payload) and
unaligned words that `CopyingPointerSet::decode_bits` rejects by construction.
The scan's offender counts are unchanged by this fix. See #7050 for the full
split; a follow-up against #7041/#7035 will teach the instrument to report it.
