# RFC: Type-Driven Representation Selection — unbox-by-default for statically-typed values

**Status:** Draft / RFC (2026-07-27)
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

**Consequence:** statically-typed numeric code round-trips through the box on every op and every
call. This is why AOT Perry loses to V8's JIT on integer-heavy code (measured on a faithful,
unrolled bcryptjs `_encipher`: **834 ms vs Node 184 ms** for the same work), and why
*expression-level* fast paths don't compose — each is another overlay on a boxed canonical, and they
invert on unrolled code (adding types to the real `_encipher` *regressed* it 834 → 2732 ms, because
~80 per-read guards plus boundary re-materialization dominate). Optimizing individual expressions
cannot fix a representation problem.

## 2. Goal / non-goals

**Goal:** make the native (unboxed) representation the **canonical, first-class** representation of
any value whose type is statically proven; confine NaN-boxing to values that are provably
polymorphic. Unbox *as much as static proof allows*, end-to-end — locals, params, returns, and
(later) typed heap slots.

**Non-goals:** unboxing genuinely-polymorphic code (it stays boxed — NaN-boxing remains the
*default* representation, not the *only* one); building a JIT / deopt tier.

## 3. Prior art (existence proof)

- **Static Hermes** (Meta) — an AOT compiler for JS/TS that uses static types to emit unboxed native
  code, with no NaN-box for typed values. This RFC is the same architecture applied to Perry.
- **V8 / JavaScriptCore / SpiderMonkey** — do representation selection *dynamically* (type feedback +
  deopt). Perry, being AOT with no deopt tier, must **prove** types statically (conservatively,
  falling back to `Boxed`) rather than speculate. That is AOT's job — and its advantage: no warmup,
  no deopt overhead, so a statically-proven kernel can *beat* the JIT, not merely match it.

## 4. Architecture

### 4.1 Representation lattice (a first-class IR property)
Every SSA value, local, param, and return — and, in Phase 3, every heap slot — carries a
representation from a lattice: `Boxed(JsValue) ⊒ { I32, U32, F64, F32, Bool, Ptr<Int32Array|…>,
Str, SmallBigInt, … }`. `Boxed` is top and always sound. This replaces the current model
("canonical = Boxed, native = transient overlay") with "representation is a property of the value."

### 4.2 Static representation inference (the AOT analog of type feedback)
An interprocedural flow analysis / abstract interpretation:
- **Seeds:** literals, `new Int32Array(...)` and friends, TypeScript annotations, and known builtin
  result types (`Math.*`, `.length`, integer-typed-array element reads → I32/…).
- **Propagate** through assignments, phis, calls (via interprocedural summaries), and returns;
  **meet** at control-flow joins, unifying to `Boxed` on conflict.
- **Conservative:** anything unproven becomes `Boxed`. Soundness beats coverage — there is no deopt
  safety net.

### 4.3 Representation-selected lowering
Locals and params get native slots according to their representation (an `i32` slot, not a
`double`); loop phis are typed; operations stay native end-to-end. This subsumes and *completes* the
existing expression-level i32-chain work (which makes the ops native but leaves storage boxed).

### 4.4 Specialized calling convention (monomorphization)
Functions are specialized on the representations of their call sites. A statically-monomorphic call
site (e.g. `_encipher(lr, 0, P, S)` where `P = new Int32Array(...)`) calls a specialized entry that
takes **raw** args (`ptr`, `i32`); polymorphic callers keep the boxed entry. Dispatch is chosen
statically. This is the piece that lets unboxing cross the ABI — the boundary the current overlay
model cannot span.

### 4.5 Boundary transitions and GC
- **Box** (materialize) only when a native value flows into a `Boxed` context (a polymorphic call,
  an `Any` store, `console.log`).
- **Unbox** when a boxed value enters a proven-typed context — with a runtime type-check that
  throws / handles a mismatch (no deopt), confined to boundaries and rare under sound inference.
- **GC:** unboxed scalars are not roots; only boxed values and heap pointers are. The shadow-slot
  binding for numbers disappears — a structural root-set reduction that falls out for free.

### 4.6 Typed heap (Phase 3)
Typed object fields stored unboxed; typed-array element access stays unboxed when the consumer is
typed (stop re-boxing the element on the way out).

## 5. Phasing (one design; each phase sound on its own)

- **Phase 0** — Representation as a first-class IR property + inference skeleton, with `Boxed` as the
  default everywhere → zero behavior change. Scaffolding and tests.
- **Phase 1** — Canonical unboxed **locals** (typed slots + typed loop phis; drop numeric shadow
  roots). The biggest single win for numeric loops. Completes the existing i32-chain work.
- **Phase 2** — Specialized **ABI / monomorphization** for statically-typed call sites. Unblocks
  unrolled `_encipher` (raw-typed args; the per-access kind-guard is proven away by specialization).
- **Phase 3** — Typed **heap** fields + typed-array-access unboxing to typed consumers.

## 6. Acceptance criteria

- Real (unrolled, untyped-source) bcryptjs `_encipher` compiles to unboxed native i32/f64
  end-to-end, **beats Node/V8**, byte-exact.
- A looped numeric kernel: **zero** `fptosi`/`sitofp` in the hot loop.
- Polymorphic code: unchanged representation, no regressions; the full gap and conformance suites
  remain byte-exact against pinned Node 26.5.0.

## 7. Risks

- **Soundness under no-deopt:** must prove, not speculate; conservative `Boxed` fallback; boundary
  unbox must guard-or-throw. This is the central correctness burden.
- **Compile-time cost** of interprocedural inference (mitigate with summaries, caching, on-demand
  analysis).
- **ABI + GC complexity** (two entries per specialized function; representation-aware root scanning).
- **Scope:** this is Perry's single largest architectural line — it touches HIR, the type system,
  codegen, the ABI, GC, and the object model. It replaces the incremental fast-path strategy for
  numeric hot code with a permanent representation model.

## 8. Evidence appendix

Measured during the bcryptjs `_encipher` performance investigation (2026-07-27):

- `l = phi double` post-`-O3` with per-iteration `fptosi`/`sitofp` that LLVM cannot remove; a
  representation-selected slot would be `phi i32` with zero conversions.
- Real `_encipher` — untyped source vs. hand-typed params, campaign fast paths on vs. off:
  **834 / 2732 / 6103 ms** (Node 184 ms). Typing the real code *regressed* it, because overlays do
  not cross the ABI and per-read guards multiply on unrolled bodies.
- The expression-level fast paths that motivated this RFC are correct and byte-exact and help
  *looped, statically-typed* kernels, but were 0% on real (untyped-param) bcryptjs and net-negative
  on unrolled code — the empirical case for a structural, rather than incremental, fix.
