Restored `ret undefined` for a constructor symbol that cannot replace `this`.

This is the **second**, independent cause of #8648's regression — the one
`field_init.rs` (#8653) did not touch. It is not about inheritance, despite the
symptom set: `benchmarks/issue-8289/cycles.ts` has no `extends` anywhere and was
1.68x.

#8630 gave every standalone `<Class>_constructor` symbol a completion block that
ends in `js_ctor_return_override(this, <return slot>, …)`, so a derived `super()`
whose base hands back a replacement object can publish it to the caller. The
motivation is right, but it also changed the ORDINARY constructor's return value
from `undefined` to `this` — and `lower_call/new.rs` says in its own comment what
that costs: *"the fast arm ran no constructor, which is `undefined` — the same
thing an ordinary ctor body returns — so `emit_ctor_return_override` below yields
the instance on both arms"*. That sentence stopped being true. The caller's
`is_undef` test went from never-taken to always-taken, and the call it guards is
not cheap: `constructor_return_overrides_this` probes the typed-array registry,
the buffer registry, callability, the Proxy registry, `arguments`, `clean_arr_ptr`
(which walks GC forwarding chains) and finally the GC header — per construction,
to answer "yes, an object" and hand back the value the caller already had. Under
RS4GC the call is also a statepoint, so the live-pointer set spills and relocates
around it.

Why the affected set looked like an inheritance story: `ctor_prologue_stores`
skips the constructor call entirely when the whole body is a run of
`this.<f> = <param>` stores, so those classes never reach the symbol. That is
exactly `micro_ctor` and `tree_wide` (1.00x). One literal initializer
(`this.peer = null` in `cycles.ts`) or a `super()` disqualifies the plan, the
call is emitted, and the class pays.

The fix publishes `this` only when a replacement can exist. `ctor_chain_can_replace_this`
(now shared, in `new_helpers.rs`) walks the heritage chain and answers `true` for a
value-bearing `return` in any constructor on it, a native base, a dynamic
`extends`, an id-only parent edge, or a class missing from `ctx.classes` —
conservative on every edge it cannot see. Everything else returns `undefined`
exactly as before #8630; every caller (`lower_call/new.rs`, the synthesized
default-derived path in `codegen/method.rs`, and the runtime construct paths,
which discard the value outright) already maps `undefined` onto its own receiver.

`field_init.rs`'s own copy of this predicate is replaced by the shared one. The
copy looked for the constructor in `class.methods` under the name `"constructor"`;
HIR keeps it in `class.constructor` and never puts it in `methods`, so the
value-returning arm could not fire. The shared version also uses
`ctor_body_has_value_return`, which walks `try`/`switch`/`for-of` bodies that
`collectors::mutation`'s version did not — that now-superseded copy is deleted.

Measured (instructions retired, `/usr/bin/time -l`, vs the pre-#8630 numbers in
the issue):

| bench | pre-#8630 | main `c2da034` | this change | |
|---|---|---|---|---|
| two-class `new B(x, y)` loop | 998,471,071 | 1,648,244,291 | **1,007,905,144** | 1.65x -> **1.01x** |
| `cycles.ts` | 1,301,925,013 | 2,182,711,384 | **1,330,975,040** | 1.68x -> **1.02x** |
| plain-class control | 381,756,402 | 372,826,080 | 373,056,337 | 0.98x (unchanged) |

Program output is byte-identical on all three.

Differential against Node 26.5.1 (`--experimental-strip-types`), 21 constructor
semantics cases: every one is byte-identical to what `main` prints, and 18 of 21
match Node. The three that do not (`super()` from inside an arrow, an uncaught
`ReferenceError` after a base ctor throws inside `try`, and `class X extends
Error`) fail identically on `main` — pre-existing, untouched by this change. The
cases that pin the semantics being preserved all match Node: a base constructor
returning a replacement object seen by a subclass field initializer, a derived
constructor returning an object, a derived constructor returning a primitive
(TypeError), a conditional value return, and a value return inside `try`.

`lower_call/ctor_return_publish_tests.rs` pins all four directions at the IR
level — this is invisible to every behavioural test, since both spellings produce
the same program output.
