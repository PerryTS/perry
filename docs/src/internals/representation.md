# Whole-program representation

How Perry chooses the machine representation of each value: wpfacts decides it for the whole
program, and codegen obeys. This is the current design. It replaces the per-site selection of the
[representation-selection RFC](https://github.com/PerryTS/perry/blob/main/docs/representation-selection-rfc.md) for scalars, the internal ABI and heap slots: it
overrides RFC §5.2 (profitability term), RFC §5.4 (tuple specialization and budgets) and RFC §6
(the per-phase, per-local selection). RFC §5.3 operation semantics, the RFC §5.5 acceptance
contracts, RFC §5.6 GC rules and the RFC §5.7 Phase 4b rejection stay in force. Section numbers
below (§1–§9) are the ones code comments cite. The facts themselves are described in
[Program facts](program-facts.md).

## 1. Why

The chess beat-Bun round (Perry #14262, #14294, #14295) measured what per-site selection
costs. Every attempt to add i32 values *locally* (int32 params, int32 call results, signed
index params, a relaxed index split) moved the conversions to the neighbouring boundary and
changed the total by −1 % to −6 %, sometimes by nothing. The remaining gap is spread over the
whole program, not concentrated in a single loop:

| workload (macOS arm64, instructions retired) | Perry | Bun AOT | ratio |
|---|---:|---:|---:|
| search 250000 | 41.36 G | 12.71 G | 3.3× |
| perft 4 | 1.055 G | 0.375 G | 2.8× |
| evaluate 100000 | 3.538 G | 1.433 G | 2.5× |
| legal 10000 | 1.465 G | 0.436 G | 3.4× |

The cause is structural. The ABI is boxed, numbers have two encodings (raw double and
INT32-tagged), and codegen picks a representation per function, per local and per site. So
one value is boxed for a call, unboxed by the callee, tag-tested on every read for the INT32
case, and re-boxed at the return. Each local decision is correct. The sum is the gap.

## 2. The rule

1. **wpfacts is the only representation authority.** Every value-holding node gets exactly one
   representation from the whole-program facts: bindings, temporaries, parameters, returns,
   call results, object fields and array elements. Codegen reads it and obeys it. Codegen
   never picks a representation itself: no per-site heuristics, no profitability rule, no
   local promotion. This is the target design. While P5 is partial (§8), codegen still chooses
   per site in these places: the `$idx$i32` index-split clones, the `spec_abi` tuples, and the per-function collectors
   (`facts_int32_locals`) for a local the facts do not cover.
2. **Representation-typed signatures are the internal ABI.** A function's parameters and
   return are passed in their representations. A boxed adapter exists only for a function
   that escapes to code the analysis cannot see.
3. **A proven Number is a canonical double.** It is never INT32-boxed, at rest or in flight,
   so no reader of a proven Number tests the INT32 tag.

Conversions exist only at **boundaries**: native/FFI calls, the dynamic constructs listed in
RFC §5.2 (soundness barriers), and ⊤ nodes, meaning nodes whose facts are unknown. Inside proven
code a value keeps the representation it was given from production to last use, except for the
one exact widening §3 allows: an I32 value copied or joined into an F64 node takes a `sitofp`.

A **conversion** here means re-encoding one value because two pieces of code disagree on its
representation: box, unbox, tag test, retag. An *operation* whose JavaScript semantics compute
in another domain is not a conversion. `a / b` on two I32 operands is a double division
(RFC §5.3), and the `sitofp` it emits belongs to the operation, the same as `x | 0` emitting
ToInt32.

## 3. Representations and how wpfacts assigns them

The scalar lattice is `I32 ⊑ F64 ⊑ Boxed` (Bool and the pointer representations of RFC §5.1
follow the same rule in later phases). `Boxed` is ⊤ and always sound.

- **F64**: the node's values are all Numbers (the `prims` fact is exactly `{number}`).
- **I32**: F64 holds, *and* the interval fact proves every value integral and inside
  `[-2^31, 2^31)`, *and* the node carries **int demand** (below). The interval may admit `-0`
  only when no use of the value can observe the sign of a zero (a division, `**`, `Object.is`,
  a native operand and every use the facts do not classify observe it); otherwise `-0` must be
  impossible.
- **Boxed**: everything else, including every ⊤ node.

**Int demand** replaces the RFC §5.2 profitability term with a fact. It is a backward property
computed by the solver: a node has int demand when an int consumer is reachable from it along
copy edges. Int consumers are array and typed-array indices, bitwise operands, `Math.imul`,
int typed-array stores, and switch/compare chains against int constants. A counter that feeds
only double arithmetic (the `15_mandelbrot` case from #7128) has no int demand, so it stays F64
and pays no conversion. A chess square feeds indices on every path, so it is I32 everywhere it
flows. Because demand is a whole-program fact, it gives the same answer in the caller, the
callee, the field and the array element. That shared answer is what removes the churn.

Demand also flows from an integer rule's result back to its operands (`+ - * %`, unary `-`,
`++`/`--`, a loop test's clip, `Math.abs`/`min`/`max` and the integral roundings), and
through filters.

**The ring rule.** `+` and `-` on I32 operands have a result that is exact in a double (below
2^33). When the interval fact proves that result inside i32, the node is I32 and lowers to a
native `add`/`sub` with no check. When every consumer of the result is a ToInt32 context, the
operation lowers to a wrapping i32 `add`/`sub`, which equals ToInt32 of the exact double. `*`
qualifies only when the interval proves `|a·b| < 2^53`, because `(a*b)|0` is not
`Math.imul(a, b)` above that bound. No other operation is inferred as integral (RFC §5.3 still
holds).

**Joins.** A copy edge from an I32 node into an F64 node is a widening (`sitofp`, exact). It
happens where the program merges an integer with a non-integer, and where an int-demanded value
is copied into a node with no int demand (one whose consumers are all double arithmetic). Both
are exact and test no tag; they are the one representation change inside proven code. A copy edge into
`Boxed` is a boundary (§6).

## 4. Representation-typed ABI

- Each function has **one signature**: the representation of each parameter node and of the
  return node. Parameter nodes already join every caller's arguments, so one signature serves
  every resolved call. That replaces the RFC §5.4 specialization tuples, the per-function and
  module budgets, `PERRY_SPECIALIZED_ABI_MAX` and the boxed-plus-one-tuple scheme: there is
  nothing to bound, because there are no clones.
- A resolved call (the call graph edge is known, so the callee set is exactly known)
  passes and receives raw values. Main serves a typed signature only to a call whose callee
  is exactly one body at every copy of the call, with no spread; any other call, including one
  that resolves to several callees, takes the boxed entry. Calling several differently typed
  callees through the join of their signatures is open P2 work.
- **Boxed adapters** are emitted only for escaping functions: a function whose value reaches a
  ⊤ node, a native callback, a dynamic call site, a module export consumed outside the program,
  or a reflective surface. The adapter unboxes under the RFC §5.5 contracts and calls the typed
  body. Non-escaping functions have no boxed entry at all. This is open P2 work: today every
  function keeps its boxed entry.
- Methods follow the same rule (receiver plus parameters). This absorbs the approved "Phase 2
  for methods" work: `spec_abi` on methods becomes the general ABI, not a special case.
- GC: raw pointer parameters keep the RFC §5.6 spill-to-shadow-slot rule. Raw scalars are not roots.

## 5. Canonical doubles

- Proven code never creates an INT32-boxed Number. When an F64 or I32 value is boxed at a
  boundary, it boxes as a raw double.
- No Number is INT32-boxed. The runtime has no API that builds one, and compiled code never
  tests for the INT32 tag on a Number, proven or not.
- Heap storage is already canonical on the proven paths: number fields are raw IEEE bits
  (RFC §5.7 4b, finding 1), `runtime_store_jsvalue_slot` canonicalizes INT32 stores (4b.2), and
  `canonicalize_array_numeric_store_bits` does the same for arrays.

## 6. Boundaries

Boundaries are exactly these:

- native/FFI calls, with the manifest-declared representations as the contract
- RFC §5.2's soundness barriers (eval, `with`, Proxy, accessors on proof-relevant objects,
  reflection, `delete`, prototype mutation, catch joins)
- ⊤ nodes
- escaping-function adapters (§4)

Every boundary is explicit in the IR, and a per-module dump lists them with their counts
(§8, P0). A conversion anywhere else, other than the §3 I32-to-F64 widening, is a bug in the facts or a codegen site that is not yet
following them; it is never a tuning choice.

The chess benchmark is closed by design: none of these boundaries occurs in its hot paths.
That makes it the end-to-end test:
**any boundary in a chess hot function is a defect to fix, not a cost to accept.**

## 7. Fields and array elements

A field node (per class field per allocation token) and an element node (per array token)
carry a representation like any other node. Their layouts do not change:

- **F64 fields** are already raw double slots. The fact removes the remaining read-side and
  store-side checks on every receiver the facts cover, not only `Ptr<Shape>`-proven locals.
- **`number[]` elements** are already raw f64. The facts replace the local
  provenance-and-containment collector (`collectors/ptr_numarray.rs`) as the eligibility source,
  so arrays that are passed to callees qualify as well.
- **Int32Array and other int typed-array elements** are I32 at rest already.

**I32 at rest.** An I32 field is an I32 lane of its shape and an I32 element an I32 word
(`INTERNAL_WORD_TAG | (u32)v`); both are decoded and encoded only through the layout accessor
(perry-runtime `layout_slot.rs`). Proven code reads the facts, never the header bit. This
layout applies only to templates wpfacts admits (`rest_elems`, `rest_fields`): an Array no
generic reader (a builtin, a protocol, a spread, native code) can reach, and a class without
heritage whose instances no generic reader such as JSON can reach. Every other template keeps
the ordinary JSValue layout, and a lane-key site whose receiver the facts do not prove goes
through `js_object_rest_get` / `js_object_rest_set`.

## 8. Phasing

Each phase is sound on its own and ships with probes, parity tests, an env switch that restores
the previous behaviour (`PERRY_WP_REP_P1`, `PERRY_WP_REP_P2`, `PERRY_WP_REP_TEMPS`,
`PERRY_FACTS_AT_REST`, `PERRY_FACTS_INT_DEMAND`), a changelog fragment, rustfmt and the
file-size allowlist. P3 has none: the INT32 encoding it removed no longer exists. Per the structural-only rule, no phase adds a cache, a side table or a
memo: the representation is a field of the node's facts.

- **P0: export** (landed; the boundary census is open). wpfacts computes `rep` and the int-demand fact for every node and exports it
  to codegen (bindings, params, returns, call results, fields, elements), with a dump and a
  boundary census per module. No codegen change. Acceptance: chess and the tssrc corpus dumps
  show which nodes are I32, F64 and Boxed, and tssrc compile RSS stays within +5 %.
- **P1: locals and temporaries obey** (landed). Binding slots, loop phis and expression temporaries
  take the fact's representation, and the ring rule applies to `+ - *`. This subsumes
  `facts_int32_locals` and the Phase 1 canonical-i32-locals collector.
- **P2: typed signatures** (partial). §4 for functions and methods, with boxed adapters only
  for escaping functions. This retires `spec_abi_sites`, the tuple keys and the budgets. Landed
  (#14328): wpfacts computes the signatures, a parameter the facts give `I32` takes the
  canonical-i32 slot at entry, and a call whose every callee returns only Numbers is a Number.
  The calling convention itself, the boxed adapters and the retirement are the remaining work.
- **P3: canonical Numbers** (landed). §5. No Number is INT32-boxed, and no reader tests the
  INT32 tag.
- **P4: fields and elements** (landed for admitted templates). §7, with I32 at rest for the
  fields and elements wpfacts admits.
- **P5: retire the heuristics** (partial: the RFC §5.2 profitability term is gone; int
  demand decides). Delete the `$idx$i32` index-split
  clones, `spec_abi` tuples, `facts_int32_locals` and the remaining per-site representation
  choices in codegen, so that codegen has exactly one source for representation.

## 9. Acceptance

- **Chess, macOS instructions retired:** Perry is below Bun AOT on all four workloads (search
  250000, perft 4, legal 10000, evaluate 100000). Peak RSS is no worse than today. The chess
  source is not changed.
- **Structural proof on chess:** in the post-`-O3` hot functions (move generation, make/unmake,
  attack tests, evaluation, search) there are **zero** boxing or unboxing runtime calls,
  **zero** INT32 tag tests, and no `sitofp`/`fptosi` except those that belong to an operation
  (§2). The P0 boundary census for those functions is empty.
- **Exactness:** the RFC §7 gap corpus, conformance smoke and the RFC §5.6.1 GC matrix show zero new
  failures at every phase. Any new representation file is registered in the matrix corpus.
- **Compile cost:** tssrc compile time and RSS are measured at every phase, with a budget of
  +5 % RSS.
