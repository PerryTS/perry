# RFC: rooting by construction

**Status:** proposal. Nothing in this document is implemented.
**Problem:** [The GC rooting invariant](gc-rooting-invariant.md) — #7154, #7184,
#7192, #7206, #7211.

## The case

Five instances of one bug in about a day. Each was found by a different means,
each took hours to localise, and each fix was two lines. The fixes are not the
cost; the *representability* is. Today a lowering author can write the wrong
thing, and nothing between their keyboard and a crash five GC cycles later
objects.

The current defences are all detection, and they run at increasing distance from
the mistake:

| defence | catches | latency |
|---|---|---|
| code review | what a reviewer happens to notice | minutes, unreliable |
| `gc_root_dominance_check.py` | dominance violations in emitted IR | one CI run |
| `PERRY_GC_ZEAL` / from-space protect | the *consequence*, if timing cooperates | a test run, flaky |
| a user's crash | everything, eventually | days |

The static checker is a genuine improvement and should stay. But it is still a
post-hoc pass over generated artefacts: it tells you the IR you produced is
wrong, not that the code you wrote cannot produce it. V8 made the opposite
choice with `Handle` / `HandleScope` / `DisallowGarbageCollection`, and the
reason is instructive — V8 has far more GC-touching call sites than perry, and
manages them with a type discipline rather than with a linter.

**The question this RFC answers: can perry's Rust codegen make an unrooted live
value across a collection point fail to compile?**

Short answer: yes, for four of the five real bugs, with a change that is
mechanical but wide.

## Why the type system is currently absent

Perry's codegen represents an SSA value as a **`String`**:

```rust
let obj = ctx.block().call(I64, "js_object_alloc", &[(I32, &tcid), (I32, &n)]);
// obj: String   -- the register name, e.g. "%r10"
ctx.block().call_void("js_object_set_field_by_name", &[(I64, &obj), ...]);
```

`String` is `Clone`, has no lifetime, and carries no information about what it
holds or whether it is still valid. Every value in the emitter — an `i32` loop
counter, a `double`, a GC pointer, a slot index — has the same type. There is
nothing for a rule to attach to. That is the root cause of the *class*, as
distinct from the root cause of any one bug.

There are ~2500 builder call sites (`~2080` `.call(`, `~416` `.call_void(`)
across 35 files in `crates/perry-codegen/src`.

## Proposed API

Three types and one rule.

```rust
/// A register holding a GC-managed value that is NOT rooted.
///
/// Borrows the emitter immutably. Not Clone, not Copy.
pub struct Raw<'e> {
    reg: String,
    _emitter: PhantomData<&'e Emitter>,
}

/// A shadow-slot root. Outlives collection points; cannot be read directly.
pub struct Rooted {
    slot: SlotIdx,   // only obtainable from ShadowFrame::alloc_slot
}

/// A register holding something the GC does not manage: i32, double, bool,
/// a slot index. Freely copyable, no lifetime.
pub struct Plain(String);
```

The whole design rests on **splitting the emitter's methods by whether they can
collect**:

```rust
impl Emitter {
    /// Cannot collect. Takes &self, so outstanding `Raw` handles stay valid.
    pub fn emit_pure(&self, ...) -> Plain { ... }

    /// CAN collect. Takes &mut self, which ends every outstanding `Raw` borrow.
    pub fn emit_call(&mut self, sig: CollectingCall, args: &[Arg]) -> Raw<'_> { ... }
}

impl Rooted {
    /// Re-read the slot. The returned Raw is valid until the next &mut emit.
    pub fn get<'e>(&self, e: &'e Emitter) -> Raw<'e> { ... }
}

impl<'e> Raw<'e> {
    /// Consume this register into a root. The only way to make a Rooted.
    pub fn root(self, e: &mut Emitter, frame: &mut ShadowFrame) -> Rooted { ... }
}
```

The rule falls out of the borrow checker with no new machinery:

> A `Raw<'e>` holds a shared borrow of the emitter. Emitting anything that can
> collect requires `&mut`. Therefore **a `Raw` cannot be used across a
> collection point** — the compiler rejects it.

```rust
let obj = e.emit_call(OBJECT_ALLOC, &[..]);       // Raw<'_>, borrows e
e.emit_call(SET_FIELD, &[obj.arg(), ..]);         // needs &mut e
let boxed = obj.nanbox(&e);                       // ERROR: obj borrows e,
                                                  //        which is mutably
                                                  //        borrowed above
```

The fix is the correct code, and it is the shortest path out of the error:

```rust
let obj = e.emit_call(OBJECT_ALLOC, &[..]).root(&mut e, &mut frame);
e.emit_call(SET_FIELD, &[obj.get(&e).arg(), ..]);
let boxed = obj.get(&e).nanbox(&e);               // re-read, correct
```

Note that `Rooted::get` returning a fresh `Raw<'e>` also enforces the *second*
half of the contract that `temp_root.rs` documents today in prose: **re-read
after every collection point**, never cache the load. A cached `Raw` simply does
not survive the next `&mut`.

### Implementation note

`emit_pure` taking `&self` while appending to the instruction buffer needs
interior mutability — a `RefCell<Vec<Insn>>` inside `Emitter`. That is the one
piece of real machinery this design requires, and it is contained to the
builder. The `RefCell` is never held across a call into user code, so the
runtime borrow panics are not a practical hazard.

`CollectingCall` vs pure is decided by a table with the same one-sided bias the
checker already uses: **a callee is collecting unless it appears in a
`NON_COLLECTING` list whose every entry names the runtime line that proves it.**
That list already exists, in `gc_root_dominance_check.py`. It should move into
Rust and become the single source of truth both consume.

## Would it have caught the real bugs?

| bug | shape | caught? |
|---|---|---|
| **#7192** root store after a collecting call | `%obj` used after `js_call_function` | **Yes.** `Raw` used after `&mut` emit — borrow error. |
| **#7206a** method receiver across the argument list | receiver loaded, args lowered, receiver used | **Yes.** Lowering an argument is an `&mut` emit; the receiver `Raw` is dead. Author must hold a `Rooted` and `get()` after. |
| **#7206b** computed-read base across the key expression | base materialized, key lowered, base used | **Yes.** Identical mechanism. |
| **#7211** `ClassExprFresh` predicate asks the wrong question | rooted only if *initializers* can collect | **Yes, and most valuably.** There is no predicate to get wrong: `js_object_set_field_by_name` is a `CollectingCall`, so the class object's `Raw` cannot survive the loop. The author is forced to `root()` — the cleverness that caused the bug becomes unexpressible. |
| **#7184** slot index outside the pushed frame | `js_shadow_slot_bind(i32 N)` with `N >= frame size` | **Partly.** Not a liveness bug, so the borrow checker is silent. It *is* fixed by construction if `SlotIdx` is only obtainable from `ShadowFrame::alloc_slot()` and the frame's `enter(n)` count is derived from the number allocated, rather than both being written by hand. That is a worthwhile companion change and is cheap. |

Four of five by construction, the fifth by making the frame own its own slot
numbering. That is a strong enough result to justify the work.

## Migration cost

The honest number is large but the distribution is favourable.

- **~2500 builder call sites**, 35 files. Most are *not* GC-managed: loop
  counters, `double` arithmetic, NaN-box bit twiddling, slot indices. Those
  become `Plain`, which is `Copy` and imposes nothing. A rough read of the call
  sites suggests **300–500 genuinely handle GC pointers** — the ones in
  `expr/`, `lower_call/`, and the object/array/closure literal paths.
- **8 `rooted_handle_begin` sites** exist today, so the *explicit* rooting
  surface is currently tiny. That is the point: the sites that need rooting and
  do not have it are the bugs.
- The work is mechanical and the compiler drives it: change a signature, follow
  the errors. It does not require understanding each lowering, only the local
  data flow the compiler points at.

**Incremental path, which matters more than the total:**

1. Land the types with an explicit, greppable escape hatch:
   `Raw::from_untrusted_register(String)` / `Raw::into_untrusted_register()`.
   Every un-migrated caller uses it. Zero behaviour change, zero risk.
2. Migrate one family at a time, highest-risk first: `expr/temp_root.rs`'s
   clients, then `lower_call/*`, then the literal paths. Each is its own PR.
3. `#[deny]` the escape hatch per-module as each module finishes, so migrated
   code cannot regress.
4. Keep `gc_root_dominance_check.py` in CI permanently as the backstop for
   whatever still goes through the escape hatch — and as the check on the
   `NON_COLLECTING` table itself, which the type system trusts and cannot
   verify.

Steps 1 and 2-for-one-family are a plausible next PR. There is no point at which
a half-migrated tree is worse than today's.

## Performance

- **Emitted code: identical.** These are compile-time wrappers over register
  names; the IR is unchanged.
- **Compiler runtime: neutral to slightly negative.** `Raw` is a newtype over
  `String`, so no extra allocation. The `RefCell` adds a borrow flag check per
  `emit_pure`, which is noise next to the `format!` calls already in the
  builder.
- **Compiler build time: slightly up**, from monomorphisation over the added
  lifetime. `perry-codegen` is already one of the slow crates; this is worth
  measuring before the wide migration, not assuming.
- **Risk of *more* rooting than today:** yes, and that is a real cost worth
  naming. When the borrow checker forces a `root()`, the author will insert one
  rather than reason about whether it was needed, and some will be redundant.
  A redundant shadow slot is one store and one bind. Given that the alternative
  is the bug this document exists about, that is the right trade — but it should
  be measured on the benchmark suite after the first family migrates, not waved
  through.

## What it cannot catch

Stating these plainly, because a safety mechanism believed to be total is worse
than one known to be partial:

- **A miscategorised callee.** If a genuinely-allocating helper is listed in
  `NON_COLLECTING`, the type system will cheerfully allow a `Raw` across it. The
  table is trusted input. This is why the checker must stay: it derives its
  verdict from the emitted IR, so the two failure modes are not correlated.
- **The escape hatch**, for as long as any caller uses it.
- **Runtime-side rooting.** `RuntimeHandleScope` in `perry-runtime` is a
  separate discipline over hand-written Rust; nothing here touches it.
- **Anything interprocedural.** A lowering that returns a `Raw` to a caller that
  then collects is caught only if the lifetime actually propagates — which it
  does for direct returns, but not across a `String` boundary or a struct field
  that erases the lifetime.
- **Correctness of the shadow frame itself** — that `enter(n)` matches the slots
  used, that the frame is popped on every path including unwinds. The
  `SlotIdx`-from-`alloc_slot` change addresses the first; the rest is separate.
- **Values rooted in a side table rather than a slot.** As #7211 shows,
  `CLASS_OBJECT_VALUES` roots *its own copy* and leaves the register stale. The
  type system would treat such a value as unrooted, which is the correct and
  conservative answer — but it means some code that is arguably fine today will
  be forced to add a slot.

## Recommendation

Adopt, incrementally, starting with step 1 and one family. The decisive argument
is #7211: an author who was *actively thinking about GC rooting*, who wrote a
four-clause predicate to decide whether to root, still got it wrong — because
the predicate asked about the user's expressions and not about the lowering's
own emitted calls. No amount of care or review reliably catches that. A type
that makes the value unusable after the call does, and it does so at the moment
the mistake is made rather than five GC cycles later in someone else's program.

**Not prototyped here.** `crates/perry-codegen/src/expr/` and `lower_call/` are
under concurrent edit (#7206 and the `js_closure_callN` work), and a
proof-of-concept worth anything has to touch exactly those files. The right
sequencing is: land the CI gate, let the in-flight lowering fixes merge, then
open step 1 as its own PR against a quiet tree.
