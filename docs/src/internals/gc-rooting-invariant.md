# The GC rooting invariant (codegen)

Read this before you emit a call from a lowering.

## The rule

> **Any GC-managed value that is live across a collection point must be
> reachable from a root before that point.**
>
> A value read out of a root and held in an SSA register across a call **is not
> rooted**. It is a copy, and the collector cannot see copies.

Perry's GC moves objects. When an evacuating minor runs, it walks the roots,
copies live objects to old-gen, and **rewrites every reference it can reach**.
Anything it cannot reach keeps the old address. That address now points into
from-space, which is about to be reused.

A "collection point" is any of:

- an allocation (`js_object_alloc`, `js_array_alloc`, `js_closure_alloc`, string
  concatenation, boxing — anything that can take an arena block);
- a call that can allocate, which in practice means **almost every runtime
  helper**. `js_object_set_field_by_name` allocates: it performs the keys-array
  transition. `js_object_get_property` allocates: it can run a getter, which is
  user code;
- `js_gc_loop_safepoint`, the back-edge poll (only emitted under
  `PERRY_GC_MOVING_LOOP_POLLS=1`, off by default since #7161).

The safe default is that **a call collects unless you have read the runtime
source and proved otherwise**. The checker described below encodes exactly this
bias: its `NONCOLLECTING` set is the only place a call is declared safe, and
every entry names the runtime line that proves it.

## Why this class of bug is so expensive

Every violation presents the same way and none of it points at the code that is
wrong:

- the symptom is `TypeError: value is not a function`, or a SIGSEGV, **cycles
  later and somewhere else** — wherever the stale pointer is finally
  dereferenced;
- **no runtime GC probe can see it.** At the moment of the collection there is
  nothing for the collector to find, so a from-space scan, a verify-roots pass
  and a zeal run all come back clean. `PERRY_GC_VERIFY_EVACUATION` checks that
  reachable slots were forwarded; it cannot check a register it does not know
  exists;
- it is **invisible by default**, because the back-edge poll that triggers it is
  off. A green default test run says nothing about this class.

Four instances shipped in a single day. The detection lag, not the fix, was the
cost every time.

## The four ways it has actually broken

### 1. Slot index past the frame (#7184)

The root store was emitted, and it looked right. But the slot index fell outside
the frame pushed by `js_shadow_frame_enter`, so `js_shadow_slot_bind`
bounds-checked it and **silently returned**. The value was never rooted; the IR
says it was.

*Tell:* a `js_shadow_slot_bind(i32 N, …)` where `N >= the frame size`. There is
no diagnostic — the bind is a no-op by design, because a bounds-check that
panicked would be worse.

### 2. Root store after a collecting call (#7192)

The store was in-frame and correct, but emitted **after** a call that allocates.
Between the allocation and the store, the value lived only in a register.

```llvm
%obj = call ptr @js_object_alloc(i32 4)
%ret = call double @js_call_function(double %a)   ; can evacuate %obj
store ptr %obj, ptr %slot                          ; stores the OLD address
call void @js_shadow_slot_bind(i32 0, ptr %slot)   ; roots a dangling pointer
```

*Tell:* the resulting slot is *rooted* and *dangling* at the same time, which is
why it survives every "is it rooted?" check.

### 3. Method receiver across the argument list (#7206)

The receiver was loaded out of its root, then the argument expressions were
lowered — each of which can allocate — and only then was the call emitted with
the receiver still in the register loaded before the arguments.

*Tell:* a `load` from a root slot, followed by any lowering of a sub-expression,
followed by a use of the loaded register. **Re-read the root after every
collection point** instead of caching the load.

### 4. Computed-read base across the key expression (#7206)

`base[key]` — the base was materialized, then the *key* expression was lowered
(allocating a string, say), then the element read used the stale base.

*Tell:* two operands where one is evaluated first and used last.

### And the one that is still open (#7211)

`Expr::ClassExprFresh` roots its class object only when it believes the static
*initializers* can collect:

```rust
let protect_handle = !captured_args.is_empty()
    || !symbol_statics.is_empty()
    || !block_fns.is_empty()
    || any_may_trigger_gc(ctx, named_statics.iter().map(|(_, v)| v));
```

Every disjunct asks about code the *author* supplied. None asks whether the
lowering's **own emitted calls** collect — and the loop below unconditionally
emits one `js_object_set_field_by_name` per static, which does. So
`class C { static tag = tag }`, whose only initializer is an inert `LocalGet`,
takes `protect_handle == false` and goes stale.

This one is worth internalising, because it is the sophisticated version of the
mistake: the author *did* think about rooting, wrote a predicate for it, and the
predicate asked the wrong question.

> **`js_object_mark_class` does not save it.** That helper puts the object in
> `CLASS_OBJECT_VALUES`, which is a registered root and *is* forwarded. The
> object stays alive and the side table's copy stays correct — and the register
> is still stale, because the collector rewrote a different copy.
>
> **Reachability is not the invariant.** The invariant is that the register you
> are still going to use was rewritten. A side table roots *its* pointer, not
> yours.

## How to check your work

### 1. The static checker — run this one

It is the only instrument that sees this class before it crashes.

```bash
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
./scripts/gc_root_dominance_corpus.sh ir-corpus
python3 scripts/gc_root_dominance_check.py ir-corpus --moving-only \
  --allowlist scripts/gc_root_dominance_allowlist.json -v
```

It parses the emitted LLVM IR, builds per-function CFGs, computes real
Cooper/Harvey/Kennedy dominance, and reports any root store that does not
dominate a preceding collection point. It is one-sided: an unrecognised call
counts as collecting, so a gap in its model costs a false positive, never a
missed bug.

For a single file you are iterating on:

```bash
PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_INLINE_SHADOW_SLOT=0 \
  ./target/release/perry compile mycase.ts -o /tmp/mycase --trace llvm
python3 scripts/gc_root_dominance_check.py .perry-trace/llvm -v
```

Both env knobs matter. `PERRY_GC_MOVING_LOOP_POLLS=1` is what puts
`js_gc_loop_safepoint` in the IR, which the `MOVING` classification keys on;
without it the corpus **cannot express the bug**. `PERRY_INLINE_SHADOW_SLOT=0`
makes every root store the `js_shadow_slot_bind` call form the checker anchors
on.

`--stale-registers` (#7206) additionally catches values that are *never* rooted
— read out of a root and held in a register across a collection point. That is
the mode that found cases 3 and 4.

`--unrooted-allocas` (#7207) covers the remaining shape, and is the one the
bind-anchored check is structurally blind to: the value lives in a plain
`alloca_entry` for its whole lifetime, so there is no `js_shadow_slot_bind` to
anchor on and a scan that starts from binds calls the function clean. It found
`lower_call/new.rs`'s inline-ctor `this_slot` independently of any runtime
probe. **The gate does not run this mode yet** — its remaining hits are the
caches, staging arrays and inlined-callee params tracked as #7210, and they are
deliberately not in the allowlist, which covers the bind-anchored shape only.
Run it by hand when you touch an `alloca_entry` site:

```bash
python3 scripts/gc_root_dominance_check.py .perry-trace/llvm \
  --unrooted-allocas --moving-only -v
```

### 2. The runtime instruments — second, and mind the depth

From #7196:

- `PERRY_GC_ZEAL=1` — collect at every safepoint. Slow, thorough.
- `PERRY_GC_PROTECT_FROMSPACE=1` — `mprotect` from-space after evacuation so a
  stale read faults immediately instead of reading plausible garbage.
- `PERRY_GC_FROMSPACE_SCAN_ABORT` — now actually runs.

> **`PERRY_GC_PROTECT_FROMSPACE_DEPTH` defaults to 4, and that default produces
> FALSE GREENS.** Four levels of retained from-space is not enough to still be
> holding the block your stale pointer is in by the time it is dereferenced.
> **Use 800.** A clean run at the default depth means nothing.

And remember the ceiling on all of these: if the collection happens while the
only copy is in a register, there is nothing at that moment for any runtime
probe to notice. These instruments catch the *consequence*, later. The static
checker catches the *cause*, now.

## The CI gate

`.github/workflows/gc-root-dominance.yml` runs the checker on every PR over a
versioned corpus of ~99 `test-files/` sources chosen for the lowerings they
exercise (class expressions, constructors, object/array literals, computed
reads, property stores, closures, dynamic dispatch). The whole job is a few
minutes; the check itself is about three seconds over ~2000 functions.

It is built to be able to fail, against all four hazards in CLAUDE.md:

- the checker's exit status is the job's — no `continue-on-error`, no pipe;
- `concurrency` cancels pull-request runs only, so `main` runs are never starved;
- `--min-files` / `--min-binds` / `--min-funcs` refuse a clean verdict over a
  corpus too thin to have exercised anything, and the run prints
  `checked N functions / M modules` so a silently-empty run is visible;
- `--self-test` proves it still fires on planted fixtures, and
  `--seeded-violations 40` splices collection points into the **real** corpus IR
  and requires all 40 to be reported — that is the arm that catches the checker
  silently losing the ability to read perry's output.

### The allowlist, and why it is not a number

Known-remaining violations live in `scripts/gc_root_dominance_allowlist.json`,
one entry each with a fingerprint, an issue, and a written justification.

A numeric threshold cannot tell a new violation from an old one: fix one bug,
introduce another, and the total is unchanged and the gate stays green. Worse,
under deadline the cheapest way to green a red build is to raise the number by
one, and nothing in the diff says what was conceded.

So the checker enforces three properties:

1. **an entry that matches nothing fails the build.** When you fix the bug,
   delete the entry in the same PR. That is the ratchet, and it is why a fixed
   bug cannot leave a tombstone that quietly widens coverage later.
2. **an entry suppresses at most its `count`.** A second violation of the same
   shape in the same function is new, and fails.
3. **a violation with no entry fails**, regardless of how many entries exist.

Adding an entry is a code-review event. Bumping a `count` to green a build is
the exact thing this file exists to prevent.

### Promoting this gate

**As of this writing the job is NOT in branch protection's required contexts**,
which means it cannot turn a merge red — hazard 2, and the reason the #7211 hits
sat unread on `main` from #7198 onward while the job was visibly failing.

With the allowlist the job is green on `main`, so the remaining step is for a
repo admin to add `gc-root-dominance` to the required contexts. A workflow
cannot do this to itself. Until it is done, this is documentation.

## Rules of thumb

- **Root before you call, not after.** If a value must survive a call, its root
  store belongs above the call, unconditionally. Do not predicate it on a
  cleverness about which callees collect — that is bug #5.
- **Re-read the root after every collection point.** Never cache a load out of a
  root slot across a call. `rooted_handle_get` exists for this.
- **Evaluate-then-allocate is the hazard.** Any lowering with two or more
  operands where one is materialized before another is lowered needs the first
  one rooted.
- **`--trace llvm` and read it.** Three seconds of the checker beats a day of
  bisecting a `not a function` five cycles downstream.
- **When in doubt, root it.** A redundant shadow slot costs a store. A missing
  one costs a day, and it costs it to whoever hits the crash, not to you.
