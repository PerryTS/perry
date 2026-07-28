# RFC: Type-Driven Representation Selection — unbox-by-default for statically-typed values

**Status:** Draft / RFC, v2 (2026-07-27)
**Scope:** all value classes — scalars, strings, objects, typed arrays, closures — not only numeric code.
**Driving benchmark:** bcryptjs `_encipher` (real, unrolled) — must compile to unboxed native and beat V8.

## 1. Problem

Perry's canonical value representation is the **NaN-boxed `double`**. Native machine types
(`NativeRep::{I32,U32,F64,F32,I1,Ptr,…}`) exist, but they are **region-local overlays**: the
`NativeRep` doc comments say it — *"region-local," "optimizer-local," "Public ABI remains
`JsValue`,"* — and a whole `materialize_*_to_js_value` family re-boxes a native value at every
JS-visible boundary. Concretely, verified in emitted IR:

- A provably-i32 loop accumulator `l` is stored as `alloca double`; post-`-O3` it is a
  **`phi double`** carried across the loop back-edge, so **every iteration** does
  `fptosi double %l` (unbox, twice) and `sitofp i32 → double` (rebox). LLVM cannot remove them —
  the canonical representation is `double` and the `| 0` truncation isn't a provable no-op on a
  double. (Counts survive `-O3`: fptosi=3, sitofp=6, `phi double`=3 in a trivial 8-element mixer.)
- Function params are `double %argN` — the ABI is uniformly boxed. A hot function called millions
  of times receives its typed-array/number args boxed and re-unboxes them on every call.
- A typed-array *element* is raw i32 in memory, but the *access path* re-boxes the result to a
  double before the consumer sees it.
- Numeric locals are even registered as GC roots (`js_shadow_slot_bind`) though a number can never
  be a pointer — wasted root-set work.

**Consequence:** statically-typed code round-trips through the box on every op and every call.
This is why AOT Perry loses to V8's JIT on integer-heavy code (measured on a faithful, unrolled
bcryptjs `_encipher`: **834 ms vs Node 184 ms** for the same work), and why *expression-level*
fast paths don't compose — each is another overlay on a boxed canonical, and they invert on
unrolled code (adding types to the real `_encipher` *regressed* it 834 → 2732 ms, because ~80
per-read guards plus boundary re-materialization dominate). Optimizing individual expressions
cannot fix a representation problem.

## 2. Goal / non-goals

**Goal:** make the native (unboxed) representation the **canonical, first-class** representation of
any value whose type is statically proven — for **every value class where a sound unboxed form
exists** (scalars, strings, objects, typed arrays, closures) — confining NaN-boxing to values that
are provably polymorphic. End-to-end: locals, params, returns, and typed heap slots.

**Non-goals:** unboxing genuinely-polymorphic code (it stays boxed — NaN-boxing remains the
*default* representation, not the *only* one); building a JIT / deopt tier; changing observable
JavaScript semantics in any way.

## 3. Prior art (existence proof)

- **Static Hermes** (Meta) — an AOT compiler for JS/TS that uses static types to emit unboxed native
  code, with no NaN-box for typed values. This RFC is the same architecture applied to Perry.
- **V8 / JavaScriptCore / SpiderMonkey** — do representation selection *dynamically* (type feedback +
  deopt). Perry, being AOT with no deopt tier, must **prove** types statically (conservatively,
  falling back to `Boxed`) rather than speculate. That is AOT's job — and its advantage: no warmup,
  no deopt overhead, so a statically-proven kernel can *beat* the JIT, not merely match it.

## 4. Type-class coverage — what "all types" means concretely

The lattice covers every class; the *win profile* and *constraints* differ per class. Two facts of
Perry's runtime shape the table: (a) under NaN-boxing a **plain number is stored as its own double
bits** (tags live in NaN space), so a proven-`F64` value is already bit-identical unboxed — its win
is eliminating tag-*checks* and dynamic dispatch, not changing bits; (b) Perry's GC **moves objects**
(evacuation with root rewriting), so raw-pointer representations must remain registered, rewritable
roots — their win is *static dispatch and layout*, not root elimination.

| Class | Unboxed rep | Primary win | GC | Notes |
|---|---|---|---|---|
| `I32`/`U32` | `i32` | kill f64↔i32 round-trips; native bitwise/index | not a root | value-range proof required (§4.7) |
| `F64` | `double` (same bits) | eliminate tag-checks + dynamic dispatch on use | not a root | IEEE-754 semantics untouched (§4.8) |
| `F32` | `float` | storage/SIMD only when source-proven (`Math.fround`, Float32Array) | not a root | never introduce precision loss (§4.8) |
| `Bool` | `i1` | branch directly; no TAG_TRUE/FALSE compare | not a root | |
| `String` | `StringHeader*` | skip untag/retag; **direct** string-helper calls (no `js_jsvalue_to_string` dispatch) | rooted + rewritten | short-string (inline payload) values stay by-value |
| `Object(shape S)` | `ObjHeader*` + static shape | **direct field offsets** (no hash lookup), **static method dispatch** | rooted + rewritten | the dominant win for real apps (property access ≫ arithmetic in web workloads); eligibility in §4.6 |
| `TypedArray(kind)` | header ptr (+ hoisted data ptr/len in region) | guard-free element access once kind is proven | rooted + rewritten | data-ptr hoisting invalidated at safepoints if backing can move/detach |
| `Array<number>` (`Ptr<NumArray>`) | `ArrayHeader*`; elements are raw f64 in place (the NaN-box of a number IS its double bits; `TAG_HOLE` marks holes) | SHIPPED (Phase 4a.0-4a.2): inline guarded tiers — header-proof tests instead of out-of-line guard calls, zero runtime calls on the fast path. DEFERRED (Phase 4a.3): guard-free element load/store + bare `.length` under a collector proof; no per-access header tests at all | rooted + rewritten | density lattice `Dense ⊒ HolesOK ⊒ Boxed` (§5.7); a hole-OBSERVING read needs the `TAG_HOLE→undefined` select, hole-DEFAULT consumers (`\|\|0`, `??0`, `\|0`, `>>>0`, numeric `+`) admit the 2-instruction NaN-canonical form under the raw-f64-or-holes proof only; growth re-derives the base after any extend |
| `Closure/Function` | code ptr + env ptr | static call targets (extends existing `FuncRef`) | env rooted | |
| `SmallBigInt` | `i64` | native 64-bit arithmetic | not a root | overflow → boxed BigInt path, exists today |
| `Null/Undefined` | singleton tags | fold checks statically | — | |
| polymorphic / union / escaping-to-`Any` | **Boxed** | — | boxed rules | permanent, by design |

**Sequencing by payoff:** scalars fix the math/crypto class; **objects-with-static-shape is the
class that moves real applications** (gscmaster/Next.js hot paths are property access and string
handling, not bitwise math). The architecture is built once; each class is a lowering + inference
extension on the same lattice, ABI, and boundary rules.

## 5. Architecture

### 5.1 Representation lattice (a first-class IR property)
Every SSA value, local, param, and return — and, in Phase 4, every heap slot — carries a
representation: `Boxed(JsValue) ⊒ { I32, U32, F64, F32, Bool, Str, Ptr<Shape|TA-kind>, ClosureRef,
SmallBigInt, … }`. `Boxed` is top and always sound. This replaces "canonical = Boxed, native =
transient overlay" with "representation is a property of the value."

### 5.2 Static representation inference
Interprocedural flow analysis / abstract interpretation:
- **Seeds:** literals, `new Int32Array(...)`/`new C(...)` and friends, TypeScript annotations, and
  known builtin result types (`Math.*`, `.length`, typed-array element reads, string ops).
- **Propagate** through assignments, phis, calls (interprocedural summaries), and returns; **meet**
  at joins, unifying to `Boxed` on conflict.
- **Conservative:** anything unproven becomes `Boxed`. Soundness beats coverage — no deopt net.

**Soundness barriers (forced `Boxed` or explicit guards).** Dynamic-JavaScript constructs defeat
static proof and MUST demote affected values to `Boxed` (or interpose a checked guard):
property accessors (getters/setters) and `Proxy` on any object whose shape the value's proof
depends on; `eval` / `new Function` / `with` in scope; indirect or unresolved calls (a value passed
to an unknown callee escapes to `Boxed` at that edge); native/FFI boundaries (the existing
manifest-declared reps remain the contract); reflection (`Reflect.*`, `Object.defineProperty`,
`getOwnPropertyDescriptor`) against shape-proven objects; `delete` on a shape-proven object;
prototype mutation (`setPrototypeOf`, `__proto__` writes) reaching a proven shape; and exceptional
control flow (a value's representation at a `catch` join is the meet over all potentially-throwing
paths — in practice `Boxed` unless all throwing paths agree). The inference treats each of these as
a hard meet-to-`Boxed` edge; none of them may be "optimized through."

### 5.3 Representation-selected lowering & operation semantics
Locals and params get native slots per representation; loop phis are typed; ops stay native
end-to-end. **Operation semantics are representation-preserving — lowering may never change an
observable numeric result:**
- **JS number arithmetic is IEEE-754 double.** `+ - * /` on proven numbers lower to native `double`
  ops (bit-identical to today, minus the tag checks). An `I32`-rep value entering ordinary
  arithmetic **promotes to `F64`** (`sitofp`, exact for all i32) unless the operation itself is one
  whose JS semantics are integral:
- **`I32` ops are exact only where ECMAScript already defines integral semantics:** bitwise
  (`& | ^ << >> >>>` — ToInt32/ToUint32 wrapped by spec), `Math.imul`, integer-typed-array element
  load/store, and array indexing. Additive chains may stay in `I32` **only** under a value-range
  proof that no intermediate exceeds i32 (the existing `known_finite_magnitude_bits` composition);
  otherwise they promote to `F64`. Lowering must never introduce integer wrapping the source
  doesn't have.
- **`F32` is storage-only** unless the value is source-proven single-precision (`Math.fround`,
  `Float32Array` elements); computation widens to `double` first (`fpext`), matching JS. No
  double→float demotion is ever inferred.
- **Widening is free and always allowed** (`I32 → F64` exact; `Bool → I32` exact). Narrowing is
  never inferred — it exists only where the spec defines it (ToInt32 contexts) or a range proof
  makes it exact.

### 5.4 Specialized calling convention (bounded monomorphization)
Functions are specialized on the **representation tuple** of (params, return):
- A statically-monomorphic call site (`_encipher(lr, 0, P, S)` with `P = new Int32Array(...)`)
  calls a specialized entry taking raw args (`ptr`, `i32`, …); dispatch is chosen statically.
- **Specialization key** = the representation tuple. Entries are **cached/deduplicated per key** —
  all call sites proving the same tuple share one specialized body.
- **Budgets:** per-function specialization count cap (start: 2 — boxed + one dominant tuple) and a
  module-wide entry cap (`PERRY_SPECIALIZED_ABI_MAX`, default 64). When a budget is exceeded,
  additional call sites route to the **boxed entry** (always present, always correct).
  Cold/polymorphic callers never specialize. Anti-bloat is proven **empirically**: the
  `binary-size` CI job measures the COMPILER binary, not user output, so it cannot see
  monomorphization bloat — instead, a large real corpus is compiled with the flag on vs. off
  (`PERRY_SPECIALIZED_ABI=0`, object cache cleared between arms) and the emitted binaries must be
  within ~1% of each other.
- The boxed entry is the permanent fallback ABI; specialization is purely additive.

### 5.5 Boundary transitions
- **Box (materialize)** only when a native value flows into a `Boxed` context (polymorphic call,
  `Any` store, `console.log`): scalars re-tag (`sitofp`/`uitofp`/tag-or), pointers re-tag with
  their NaN-box tag. Cheap, local, explicit in IR.
- **Unbox (accept)** when a boxed value enters a proven-typed context, under an explicit
  **per-representation acceptance contract** — mismatches take a checked path (throw where the
  spec throws; route to the boxed generic path where the spec coerces), **never silent wrong-value
  coercion**:

| Target rep | Accepts | Everything else |
|---|---|---|
| `F64` | any number (bit-identical) | non-number → boxed path / TypeError per the consuming op's spec |
| `I32`/`U32` | number with exact integral value in range (checked); in a spec ToInt32/ToUint32 context: any number via NaN-safe `toint32_wrap` (never raw `fptosi` — poison on NaN) | non-number → the op's spec behavior via the boxed path |
| `F32` | number proven single-precision-exact | boxed path |
| `Bool` | TAG_TRUE/TAG_FALSE | boxed truthiness path |
| `Str` | STRING/SHORT_STRING tagged | boxed path |
| `Ptr<Shape>` | pointer whose class/shape matches the proof | boxed dynamic dispatch |

  NaN, ±Infinity, −0, fractional values, and out-of-range integers are all **rejected** by the
  integral contracts (they take the checked/boxed path) — exactness is the contract, coercion
  happens only where the spec places it.

### 5.6 GC under unboxed representations (moving collector)
Perry's generational GC **moves objects** (evacuation with reference rewriting), which dictates the
pointer-rep rules:
- **Unboxed scalars** (`I32/U32/F64/F32/Bool/SmallBigInt`) are not roots — their shadow-slot
  bindings are dropped (a structural root-set reduction).
- **Unboxed pointers** (`Str`, `Ptr<…>`, closure envs) remain **precise, rewritable roots**: the
  slot is registered exactly as boxed pointer slots are today, and evacuation rewrites it. The rep
  changes the *tag discipline*, not root discipline.
- **Specialized-ABI pointer params** must be safe across nested calls and exceptions: on entry the
  specialized body spills each raw pointer param to a registered shadow slot (same mechanism as
  today's param binding — it already survives rewriting); all uses reload after any safepoint
  (call, allocation, loop back-edge poll). Derived interior pointers (a hoisted typed-array data
  pointer) are **region-local between safepoints only** and are recomputed from the rewritten
  header pointer after any safepoint — never live across one. No pinning is required; this is the
  same rebase-after-safepoint contract the masked-window region code uses today.
- Unwinding runs the existing frame-pop path; registered slots die with the frame — no new
  exception machinery.

### 5.7 Typed heap (Phase 4)
Unboxed storage extends to heap slots where the *container's* shape is proven and stable:
- **Eligibility:** an object qualifies for unboxed field layout only if its shape is
  closed-world-proven — no dynamic keyed writes of unknown keys, no `delete`, no accessor or
  `Proxy` interposition on it, no reflection against it, no prototype mutation, and **no
  polymorphic alias** (every reference to it carries the same shape proof; a single escaping alias
  to `Boxed` context demotes the layout to boxed). Escape analysis makes this decision; the
  default remains today's boxed field storage.
- **Shape transitions:** adding a proven field transitions between static layouts at compile time;
  any *runtime* shape change reachable at all (only possible when analysis over-approximated
  reachability of a barrier) is prevented by the eligibility rules, not patched at runtime.
- **Observation equivalence:** `for…in`/`Object.keys` order, descriptor reads, and `JSON.stringify`
  must be indistinguishable from boxed layout; codegen materializes boxed views on demand at those
  (rare, already-slow) surfaces.
- **Barriers:** typed pointer fields keep the existing generational write-barrier; typed scalar
  fields need none (another structural win).
- Typed-array element reads stop re-boxing when the consumer is typed (the element is raw in
  memory today; only the access path boxes it).
- **Plain numeric arrays (`Ptr<NumArray>`, Phase 4a).** SHIPPED in Phase 4a.0-4a.2: the
  access path uses inline guarded tiers (per-access header-proof tests on the raw-f64 /
  raw-f64-or-holes bits instead of out-of-line guard calls; zero runtime calls on the fast
  path). DEFERRED to Phase 4a.3 — the following collector contract is a design, NOT yet
  implemented: a local `number[]` whose storage is raw-f64 in place would qualify for
  fully guard-free element access under provenance + containment, exactly like
  `Ptr<Shape>` locals: single-`Let` provenance (`[]` / all-numeric literal /
  `new Array(n)`(`.fill(num)`)), every use a numeric element read/write, `.length`, or numeric
  push/pop, and the module-wide §5.2 barrier kill extended with the array-specific barriers
  (indexed writes to `Array.prototype`/`Object.prototype`, `setPrototypeOf` on arrays,
  `delete arr[i]`, `arr.length = n`, and the reordering mutators `sort`/`reverse`/
  `copyWithin`/`splice`/`shift`/`unshift` on the local). Eligibility carries a **density
  lattice** `Dense ⊒ HolesOK ⊒ Boxed`: `Dense` (no hole can exist — literal provenance with
  proven in-bounds/append-only writes) drops the hole select entirely; `HolesOK` keeps the
  `TAG_HOLE` select for hole-observing reads while hole-default consumers (`||0`-class) use
  the proof-gated 2-instruction canonical-NaN form (this consumer form DID ship in 4a.2,
  inside the guarded tiers); anything that can store a non-numeric value demotes to
  `Boxed`. Hole-vs-undefined observability (`in`, `Object.keys`, `JSON.stringify`) is
  preserved by keeping `TAG_HOLE` in storage and materializing `undefined` only at the
  read edge.

## 6. Phasing (one design; each phase sound on its own)

- **Phase 0** — Representation as a first-class IR property + inference skeleton; `Boxed` default
  everywhere → zero behavior change; scaffolding + tests.
- **Phase 1** — Canonical unboxed **scalar locals** (typed slots + typed loop phis; drop scalar
  shadow roots). In flight: `perf/repsel-p1-canonical-i32-locals` (I32/U32 first; F64 tag-check
  elision and Bool follow the same mechanism).
- **Phase 2** — Specialized **ABI / bounded monomorphization** (§5.4) for statically-typed call
  sites. Unblocks unrolled `_encipher` (raw-typed args; the per-access kind-guard is proven away).
- **Phase 3** — **Strings + pointer locals** (`Str`, `Ptr<Shape>`, `TypedArray`): rooted unboxed
  locals, static string-helper calls, static field offsets and method dispatch on shape-proven
  objects. This is the phase that moves real web workloads.
- **Phase 4** — **Typed heap** fields (§5.7) + typed-array-access unboxing to typed consumers.

## 7. Acceptance criteria (reproducible protocol)

**Benchmark protocol** (all perf claims): quiet dedicated arm64 machine (load < 3 — the project's
M1 test box), Perry built at the tested commit (`perry-dev` profile), `PERRY_NO_AUTO_OPTIMIZE=1`,
object cache cleared between env-flag arms (`rm -rf node_modules/.cache/perry` — env flags are
cache-keyed but arms must not share objects), **min-of-9** wall-clock runs, reported alongside the
machine's load average. Node oracle: the repo-pinned version (`.node-version`, currently 26.5.0),
default flags, same machine, same min-of-9. **Pass threshold "beats V8": Perry min-of-9 <
Node min-of-9 on the same inputs**, with the structural IR proof attached (see below) so the result
is not load-luck.

- **`_encipher` end-state test:** the real (unrolled, untyped-source) bcryptjs `_encipher`
  (extracted verbatim from bcryptjs@3.0.3, 2.1M-call harness, golden outputs
  `lr0=-1623241632 lr1=-1640632493`) compiles to unboxed native i32/f64 end-to-end — structural
  proof: the post-`opt -O3` function contains **zero** `js_dyn_index_get` / `js_dynamic_*` /
  `js_typed_array_*` calls and zero `fptosi`/`sitofp` in the Feistel body — and beats Node per the
  protocol above, byte-exact.
- **Loop-kernel test:** the minimal typed mixer (`slot.ts`) post-`-O3` hot loop is `phi i32` with
  **zero** `fptosi`/`sitofp`.
- **Exactness harness:** the existing byte-for-byte gap corpus (`test-files/test_gap_*.ts`, ~410
  programs, `./scripts/run_gap_tests.sh` against the pinned Node) plus the conformance-smoke
  shards — **zero new failures at every phase**. Polymorphic/dynamic programs must show unchanged
  representations (spot-checked via `--trace llvm`).

## 8. Risks

- **Soundness under no-deopt:** must prove, not speculate; conservative `Boxed` fallback; boundary
  acceptance contracts (§5.5) must throw/route, never coerce. The central correctness burden —
  §5.2's barrier list is the checklist.
- **Compile-time cost** of interprocedural inference (summaries, caching, on-demand analysis).
- **Code size** from monomorphization (bounded by §5.4 budgets; proven by the flag-on/off
  emitted-binary corpus measurement — the `binary-size` CI job only sees the compiler itself).
- **GC complexity:** pointer reps under a moving collector (§5.6) — the rebase-after-safepoint
  contract must be enforced mechanically (a lint/verifier pass over region-local derived pointers).
- **Scope:** Perry's single largest architectural line — HIR, type system, codegen, ABI, GC, object
  model. It replaces the incremental fast-path strategy with a permanent representation model.

## 9. Evidence appendix

Measured during the bcryptjs `_encipher` performance investigation (2026-07-27; protocol of §7):

- `l = phi double` post-`-O3` with per-iteration `fptosi`/`sitofp` that LLVM cannot remove; a
  representation-selected slot would be `phi i32` with zero conversions.
- Real `_encipher` — untyped source vs. hand-typed params, expression-fast-paths on vs. off:
  **834 / 2732 / 6103 ms** (Node 184 ms). Typing the real code *regressed* it: overlays do not
  cross the ABI, and per-read guards multiply on unrolled bodies.
- The expression-level fast paths that motivated this RFC (inline non-BigInt bitwise; inline
  checked-f64 typed-array-param reads; int-valued-local i32 promotion) are correct, byte-exact,
  and help *looped, statically-typed* kernels (3541 → 1005 ms on a typed reproduction), but were
  0% on real (untyped-param) bcryptjs and net-negative on unrolled code — the empirical case for a
  structural, rather than incremental, fix.
