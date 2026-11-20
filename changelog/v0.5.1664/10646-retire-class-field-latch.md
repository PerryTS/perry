### Static-key class-field reads drop the per-access latch (−18% on the guard's fast path)

Every static-key class-field read gated its fast path on
`@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED`. That is an `external global`, so on
arm64 reading it cost `adrp` + a GOT `ldr` + a dependent `ldrb` through it + a
compare — four instructions and two dependent loads, before the guard had
looked at the receiver at all. It could not be hoisted: the runtime flips it
mid-execution when a descriptor or accessor lands on a class prototype, so the
load was `volatile` by necessity.

That authority now rides on a value the guard already had to load. Each class
gains a `@perry_class_guard_shape_*` expectation, seeded at module init with
the class ShapeId and registered with the runtime;
`disable_class_field_inline_guard` poisons every registered slot with
`u32::MAX`. ShapeIds are allocated from `[0x8000_0000, 0xC000_0000)` and never
reused, so a poisoned expectation can never match a live object — every guard
misses and routes to the IC, exactly what the latch bought.

It is deliberately a SEPARATE global from `@perry_class_shape_id_*`: that one is
stamped into every new instance by `js_object_alloc_class_inline_keys_stamped`,
so poisoning it would brand live objects with a bogus ShapeId rather than close
a fast path. Subclass arms and imported-class stubs carry the poisonable
expectation too, and an imported-stub rewrite that lands after a disable
re-poisons rather than resurrects.

Measured on arm64 (`-Os` + `llc -O2 -mcpu=apple-m1`): one `o.a` on a typed
receiver goes from 28 to 23 executed fast-path instructions (−18%) with one
fewer dependent load; a probe making 16 reads on one receiver goes from 595.2
to 563.0 executed instructions per call (−5.4%, `/usr/bin/time -l` instructions
retired, differenced over iteration count). The per-read marginal is −2 rather
than −5 because LLVM already hoisted the latch's GOT base register across
accesses within a function. The latch is gone from `$generic`, `$spec_b` and
the copy the inliner leaves in the caller — the last of which is the code that
actually executes.

### The compiler's copy of the GC header layout is now gated

`perry-codegen` does not depend on `perry-runtime`, yet it bakes the collector's
header layout into emitted code: the inline `new` path stores a packed
`GcHeader` word as a compile-time constant, and every class-field /
element-shape / method-probe guard masks that word against a literal. Thirty-six
restatements across ten files, with the agreement held by a code comment — the
`debug_assert_eq!` that looked like enforcement compared codegen's constant to a
string literal, a tautology that never referenced the runtime and is compiled
out of `release` and `perry-dev` anyway. A flag renumbered in the runtime
compiled clean, passed every suite, and shipped a compiler whose allocator baked
one bit layout while the collector read another.

`scripts/check_gc_header_constants.py` (in `lint`) re-derives every restatement
from the runtime constant it quotes, including composites and the fused 32-bit
masks. A registered constant that stops existing fails, so a fix deletes its own
entry, and a new header-shaped `const` in a watched file must be registered or
exempted with a reason. Writing the registry found five restatements a
module-scope grep misses, because they are declared inside function bodies.

`shape_descriptor_census` gains a matching requirement: the class-field
precheck must read its expectation VOLATILE from the poisonable global, since a
lowering that hoisted that load would reopen a fast path the runtime has closed
and would still satisfy a shape-only assertion.
