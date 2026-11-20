### GC rooting: measure the right corpus, then fix the one rule that dominates it

Two linked changes: the gate's corpus was measuring the wrong population, and
once it measured the right one, 73% of what it reported was a single rule.

#### The corpus (#7280)

`scripts/gc_root_dominance_corpus.sh` compiles ~124 hand-written `test-files/`
sources. It reads **zero** in both modes the CI gate runs, and it read zero
while twenty lines of stock `zod` faulted deterministically under the from-space
protector. #7280 puts the gap in one sentence: *25 curated files pass while 20
lines of stock zod fail.*

That is a distribution problem, not a size problem. Both corpora, same compiler,
`--stale-registers --moving-only`:

| corpus | stale uses | dominant population |
|---|---|---|
| curated — 124 sources, 144 modules, 2378 functions | 116 | property-GET helper windows, `js_number_coerce`, `js_closure_callN` |
| dependency-scale — 81 modules, 62 MB, 12899 functions | 371 | `js_object_assign_one` 137, `js_new_function_construct` 102, `js_closure_call*` 30 |

The curated corpus produces 12 of the first population and 1 of the second. A
hand-written test allocates a couple of objects and calls a couple of helpers; a
library spreads objects into objects, boxes every mutable capture because its
closures outlive their frames, and builds values field by field out of data.
The rooting hazards live in the *shapes*.

So `scripts/gc_root_dominance_dep_corpus.sh` generates the second corpus from a
real npm dependency — `zod`, this repo's own `package.json` devDependency,
pinned by `package-lock.json` and governed by the same soak window as everything
else in that file — imported **by source path** so its modules compile natively
rather than falling back to V8 and emitting no IR to check.

**Nothing is sampled away.** All 81 modules and all 62 MB are checked. Emitting
costs ~8s; the two gated arms cost ~3s each, because they are linear in
instruction count. The `--stale-registers` budget is the expensive one (~5 min)
and the workflow says so where it runs.

`test-files/gc-dep-corpus/main.ts` is the only entry point and the rest of that
directory reaches the compiler by being imported from it, so the generator
**asserts that every `.ts` in the directory produced a module** — the check a
size floor could not be (#7278), since ~90 modules of `zod` swamp any count a
missing 40-line source would cross.

#### The rule (`crate::root_reload`)

> A load out of a shadow slot is a **copy** of a root. An evacuating minor
> rewrites the slot; it cannot rewrite the register. So every use a collection
> point can reach must re-read the slot — unless a store to that slot can also
> run on the way, in which case re-reading would observe an assignment the
> program made and the register is left alone.

Verbatim from `zod`'s object-spread lowering, before:

```llvm
  call void @js_shadow_slot_bind(i32 0, ptr %r7)   ; %r7 IS a root
  %r8  = load double, ptr %r7                      ; a COPY of the root
  %r9  = call double @baseFields()                 ; evacuates; rewrites %r7
  %r10 = call double @js_object_assign_one(double %r8, double %r9)  ; from-space
  %r11 = load double, ptr %r7                      ; the NEXT statement re-reads
```

Codegen was never unable to re-read the slot — the following statement does it,
because a fresh lowering emits a fresh load. The bug is that *within* one
lowering the load happens once, at the top, and the register is carried across
everything after it. That is why this is a pass and not another point fix:
`index_set.rs` alone lowers `object` before `value` at fifteen separate arms,
and the shape also appears in the object-literal spread, the inline class-field
store, `new`, property define, `instanceof` and the closure-call family.

The soundness half is the half `expr/temp_root.rs` already documents: re-reading
a *local* is not unconditionally safe, because `new C(g, bump())` where `bump()`
assigns `g` must pass the pre-`bump()` value. At IR level that objection is
decidable — an assignment to a shadow-slotted local is a `store` to its alloca
in the same function, and a local captured *and* mutated by a closure is boxed
instead, so it has no plain shadow slot to reload. Both halves are path
questions over the real CFG, answered the way the checker answers them: a
back-edge round trip is not an intra-iteration path, because re-entering the
load's block re-executes the load.

Where the reload was not needed — no call between the two points — LLVM's
EarlyCSE/GVN forwards it away, since nothing can clobber the alloca. Where it
was needed the intervening call is opaque and the load stays. The pass is close
to free exactly where it is redundant.

Recording is at the choke point, not at the thirteen `js_shadow_slot_bind` emit
sites: `LlBlock::call_void` and `LlFunction::entry_setup_call_void` see every
bind form, including the slow arm of #7088's inline diamond (which emits the
same call), so a fourteenth site cannot be added without the set noticing. And
the pass runs in `compile_module` before **any** rendering path, so the text
renderer and the in-process constructor (#7301) see the same IR — a pass living
inside `to_ir` would silently not apply to the other.

#### What it closes, on both corpora

| arm | curated | dependency-scale |
|---|---|---|
| `--stale-registers --moving-only`, total | 116 → **39** | 371 → **118** |
| …of which `source=slotload` | 102 → **25** | 272 → **19** |
| bind-anchored `--moving-only` | 0 → 0 | 0 → 0 |
| `--unrooted-allocas --moving-only` | 0 → 0 | 0 → 0 |

`js_object_assign_one` disappears from the dependency-scale report entirely
(137 → 0); `js_new_function_construct` goes 102 → 29.

Both budgets are now ratchets in `gc-root-dominance.yml` (`--max-stale 39` and
`--max-stale 118`), because until now this mode ran only by hand — its number
could move in either direction between one investigation and the next with
nothing to say so. It is a budget rather than an allowlist because the residual
is a *population*, not a list of triaged sites: what is left is the uses whose
slot the program itself reassigns inside the window, which need a temp root
rather than a re-read.

#### And one runtime-Rust rooting bug the corpus led to

`js_regexp_new` takes `pattern` as a raw `StringHeader*` and then allocates
twice — `js_string_from_str` for the canonical flags, `gc_malloc` for the header
— before storing that pointer into `RegExpHeader::pattern_ptr`. Either
allocation can run an evacuating minor, after which the argument names retired
from-space and the header keeps a **permanently** dangling `pattern_ptr`; the
borrowed `pattern_str` had the same exposure and fed the owned `.source` copy.

That is the runtime-Rust half of the invariant (#7249), which
`gc_root_dominance_check.py` is structurally blind to — it reads emitted IR and
cannot see a Rust local. Reproduction:

```
PERRY_GC_HEAP_LIMIT=8 PERRY_GC_INCREMENTAL=0 PERRY_CONSERVATIVE_STACK_SCAN=off \
PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 \
  ./<gc-dep-corpus-main>
```

Before: faults deterministically, `RETIRED FROM-SPACE`, `js_regexp_new + 3920`
← `js_regexp_construct` ← `perry_closure_…zod…regexes_ts__9`. The same fault
appears with the codegen pass alone, so it is not caused by it. After: clean.

No unit-test witness ships with it, and that is a deliberate statement rather
than an omission: forcing a *copying* minor inside a runtime function needs a
collection at an allocation point, and `gc/zeal.rs` documents why that level
does not exist (an allocation-point collection takes `force_full_scan`, which
makes the copying minor ineligible, so it would move nothing). A test was
written, its liveness assert refused to pass, and it was deleted rather than
shipped as a test that cannot fail for the right reason.

#### Measured after #7301 and #7305, not before

The backend rewrite (typed `LlInst`, two consumers of one finalized-item
visitor) and the move from setjmp/longjmp to `invoke`/`landingpad` both landed
while this was in flight, so every number above is re-measured on `b50e857c2`,
both sides, over identical paths. The dependency-scale parent count moved by
exactly one (370 → 371); nothing else changed.

Two consequences for the pass, both load-bearing:

* It inserts a **typed** `LlInst::Load` and rewrites operands on typed variants
  in place, never text, so `native_emit`'s `(typed, raw)` migration ratchet moves
  in the intended direction. And it runs in `compile_module` before *any*
  rendering path, so `to_ir` and the native C-API builder consume the same
  stream — a pass inside `to_ir` would have applied to one consumer only.
* An `invoke` is modelled as **both** a call and a two-successor terminator.
  Missing the call half would classify a throwing helper's window as
  non-collecting and drop every reload inside a `try`; missing the successor half
  would hide the unwind edge. The unwind edge is also why the rule is "reload at
  the USE" and not "reload after the call": a load from the slot is valid
  wherever it sits and reads whatever the collector last wrote, so it is correct
  on both edges without a placement decision.
