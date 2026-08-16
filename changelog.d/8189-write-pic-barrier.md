`perf(codegen)`: the **static write PIC** no longer pays three unconditional
`gc-leaf` bookkeeping calls per write. `o.x = v` with an untyped RHS drops
**98 instructions per write** — 18.80% of the whole program on a 12M-write
microbench (6.254 G → 5.078 G retired), peak RSS unchanged (11,776 → 11,760 KB).

`lower_put_value_static_write_ic`'s `put.pic.hit` block called
`js_string_addref_if_heap_string` + `js_gc_note_slot_layout_aware` +
`js_write_barrier_slot` (plus a pre-store load of the slot's old value, and an
RS4GC root reload per helper argument) whenever `pointer_possible` was true —
and `pointer_possible` is a COMPILE-TIME claim about the RHS, so it is true for
every `o.x = v` whose RHS is an untyped local. The value was very often a plain
double at every execution.

That arm now goes through `emit_jsvalue_slot_store_pointer_tested` (#7511),
already shipped on the class-field store path, which asks the same question
ONCE inline of the bits being stored — the question all three callees ask first
anyway, one at a time, across three cross-crate calls — and branches over all
three. The store itself stays unconditional.

Measured on the quiet M1 mini (load 1.4), best-of-5, exit codes and stdout
hashes checked every run; instrument noise floor 0.14% (min-to-max spread of an
unchanged control). Both compilers built from the same `-p` set; the two
`libperry_runtime.a` are **byte-identical**, so the runtime is not a variable.

| | base | fix | Δ |
|---|--:|--:|--:|
| `const v = f(); o.x = v` ×12M, instructions | 6,254,387,406 | 5,078,361,239 | **−18.80%** |
| …per write | — | — | **−98.0 instr** |
| …peak RSS | 11,776 KB | 11,760 KB | −0.14% |
| 19-program GC corpus, instructions | 122,374,730,879 | 122,372,664,497 | −0.002% |
| 19-program GC corpus, peak RSS | 1,397,296 KB | 1,397,360 KB | +0.005% |

**The corpus number is a no-regression check, not a measurement of this lever,
and the reason is worth recording**: an IR census over all 19 programs finds
**zero `put.pic.hit` blocks**. The static write PIC never fires anywhere in that
corpus, so "0.00% on the corpus" says nothing about the change and everything
about the corpus. Reporting it as a null result without checking whether the
subject was live would have been CLAUDE.md hazard 4.

Against the sibling dynamic-key IC on the same program shape, the static PIC's
write cost **104 instructions more** per write; it now costs **6 more**. With
#8184's measured baseline of 118 instructions for this store, the new cost is
~20 — parity with the dyn IC's 22, by two independent derivations.

### The layout-note argument, verified rather than inherited

`pointer_tested` calls `js_gc_note_slot_layout`, not the `_aware` variant, and
skips it entirely when the stored bits carry no heap pointer. That drops the
*clearing* half: an old pointer overwritten by a double no longer removes the
slot's side-mask bit, so the slot stays conservatively scanned. Safe here for
two independent reasons, both re-checked against `main` rather than taken from
#7511:

* A stale-SET mask bit is strictly weaker than `GC_LAYOUT_UNKNOWN`, which is the
  collector's DEFAULT state for a generic object. `heap_payload_slot_selection`
  turns `Masked` and `All` into the same `HeapChildSlot::Child` items — only the
  telemetry `ReadKind` differs — so the worst case is the collector examining a
  slot that holds a double, exactly what it already does for every
  unknown-layout object. It can never STRAND a child.
* `layout_note_slot`'s one arm that MUST fire (`SlotVerdict::Downgrade`, a
  pointer landing in a slot a typed descriptor declared raw-f64) is unreachable
  from a PIC hit twice over. It is guarded by `claimed_intact`, and
  `GC_OBJ_TYPED_LAYOUT_INTACT` (`0x1000`) is a member of
  `WRITE_PIC_BLOCKING_FLAGS` (`0x1907`) which all four `hit` conjunctions
  require CLEAR — verified on `main`, `expr/proxy_reflect.rs:51` and the
  `flags_clear` term. And it needs a pointer value, which is the case
  `pointer_tested` does not skip.

The same fact retires the `layout_note_conforming` arm: its comparand is
`GC_LAYOUT_SIDE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT` (`0x9000`), so at a PIC hit
it is provably false and the PIC passes `false` rather than emitting a load, a
mask, a compare and two blocks that always take the same edge.

### `emit_jsvalue_slot_store_pointer_tested` gains a `stem`

The emitter hard-coded `class_field_set.*` block names. It now takes a `stem`
like its sibling `emit_write_barrier_slot_generation_tested` already did; the
class-field callers pass `"class_field_set"` (IR unchanged) and the PIC passes
`"put.pic"`. This is not cosmetic — the IR census below identifies the guarded
arm by its label, so two call sites sharing a stem would let one site's guard
satisfy the other site's assertion.

### The block hazard, and how it is handled

`hit_end_label` is the merge phi's predecessor for the stored value. The new
emitter takes `ctx` and SPLITS BLOCKS, so on return `ctx.current_block` is its
`put.pic.gc_bookkeeping.done`, not `put.pic.hit`. Both the branch to the merge
and the label capture moved BELOW the call, with a comment saying so. Verified
in emitted IR: the merge reads
`phi double [ %r60, %put.pic.gc_bookkeeping.done.34 ], …`.

### #8185: a deleted write barrier passes every runtime probe

`docs/src/internals/gc-rooting-invariant.md` had 561 lines on the rooting
invariant and **zero occurrences of "barrier"**. The mirror-image rule is now
written down: for a missing or deleted write barrier the runtime instruments are
the ones that cannot see it, and a static IR assertion is the only detector. A
dropped barrier corrupts nothing at the store — it leaves the remembered set
merely INCOMPLETE — so turning it into an observable failure needs the parent
tenured, the child still young, a *minor* landing in that window, and that edge
being the only path to the child. `FORCE_EVACUATE`/`VERIFY_EVACUATION` verify
REWRITING, not REMEMBERING; `PERRY_GEN_GC=0` does not consult the remembered set
at all, so it makes the bug unreachable rather than visible. Recorded from
#8183: a release build with the barrier deleted passes that entire matrix
byte-identically, exit 0.

The section also documents the `GC_STORE_AUDIT` marker convention and is
explicit about its limit — `scripts/gc_store_site_inventory.py` audits the
comment CLAIM, not the IR, so deleting a barrier while leaving its
`GC_STORE_AUDIT(BARRIERED)` comment in place is a clean pass. Teaching it to
check claims against emitted IR is #8185's real long-term ask and is
deliberately **not** attempted here.

### The assertions now run on pull requests

#8183's barrier assertions lived in `crates/perry-codegen/tests/
native_proof_regressions.rs`. `test.yml`'s per-PR `cargo-test` arm is `--lib
--bins`, and `e2e-scoped` runs only suites the diff names — so they gated their
own PR and would have gated no future one, including this one, which moves a
store on a GC-managed slot. They are moved to
`crates/perry-codegen/src/expr/write_pic_barrier_tests.rs` (#5960), alongside
two new tests for the static PIC. The positive test pins presence of all three
bookkeeping calls, a `br i1` INTO the arm (an emitted block is not a reached
block — #8183's third sabotage left dead IR and initially passed), the TRUE/FALSE
edges, and the guard condition BY DEF-CHAIN rather than by nearby text. The
negative test pins that a statically-proven non-pointer RHS still emits a bare
store and no guard at all.

### Adjacent: two diagnostics cited a knob with no parser

Found while adding the doc section, because `scripts/check_gc_env_knobs.py`
rejected it. `PERRY_GC_ZEAL` has **no live parser anywhere in the tree**, yet
`gc/pin.rs`'s young-pin incoherence report PRINTS it in the reproduce command it
hands the reader, and `dyn_eval/mod.rs` names it as a live instrument. Both now
say `PERRY_GC_SCHEDULE_RATE`. A diagnostic that hands you a dead variable sends
you to run the DEFAULT configuration and read its green as a result — the same
"the gate ran but its subject never did" shape as the rest of this PR, one level
up.
