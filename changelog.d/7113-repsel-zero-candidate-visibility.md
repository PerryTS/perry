### `--opt-report` / repsel census: "no analysis ran" is now a visible, gated state

The promotion census (#7104) recorded that **8 of its 18 real workloads produced
zero representation-selection candidates** — no analysis considered a single
value in them. That is a stronger and more basic statement than "promotion is
low": a denial names a rule you can argue with, while zero candidates names
nothing, and both render as the identical census table.

It is **one root cause, not eight**. `codegen/entry.rs` excludes module-init and
program-entry bodies from canonical (i32/u32/Str) selection wholesale, and that
decision is taken at `FnCtx` construction — before any per-value rule runs — so
a top-level local recorded neither a selection nor a denial. Nine of the
seventeen `benchmarks/suite` workloads put their entire hot loop at module top
level. The control is `16_matrix_multiply`: its 3 canonical-i32 promotions are
`matmul::i/j/k`, inside the function; its two *top-level* loops over the same
arrays promote nothing.

Changes, none of which alter which values get promoted:

- `FnCtx::repsel_context_denial` carries *why* a context forbids canonical
  selection (`module_init_context`, `async_body`, `generator_body`,
  `was_plain_async_body`), and the `Stmt::Let` site records it as a denial.
  Deliberately `None` when only the `PERRY_CANONICAL_*` env gate is off — those
  are bisection knobs, and their arms must not grow entries the default build
  lacks.
- Every proven-integer local that stayed boxed now names the first failing rule
  from an ordered table (`CanonicalI32Denial::verdict`), so a local with two
  problems names the more actionable one. `02_loop_overhead`'s `i` and `sum`
  report `not_index_used_or_bounded`; its `ITERATIONS` reports
  `module_init_context`.
- The census gains `check_analysis_reach`: a corpus workload whose candidate
  total is zero across every analysis fails, unless named in
  `ZERO_CANDIDATE_ALLOWLIST` — held in the script, not the regenerable
  baseline, so `--update` cannot widen it. Only `suite_01_startup` is
  allowlisted (a lone `console.log`, no bindings).

Workloads reached by no analysis: **8 → 1**. Canonical-slot candidates across
the corpus: **15 → 71**. Promotion floors are unchanged.

Verified byte-neutral: baseline vs patched compiler emit identical object files
on 15 suite workloads with the report off and 6 with it on. Verified falsifiable:
the census exits 1 against the pre-change compiler (7 workloads flagged
`UNREACHED BY EVERY ANALYSIS`) and 0 after, and the shipped-baseline unit test
fails against the pre-fix baseline. The existing `PERRY_PTR_SHAPE_LOCALS=0` CI
sabotage arm still goes red for its own reason, not the new one.

Per-workload findings filed separately: #7109 (module-init exclusion — the real
promotion gap, `08_string_concat`'s `result` and `11_prime_sieve`'s `i`/`j` are
fully eligible and waiting on it), #7110 (canonical-i32 needs index-use or a
proven i32 bound, so a bare accumulator never promotes anywhere), #7111 (a
function whose call sites were all inlined away never reaches the spec-ABI
decision loop), #7112 (`Ptr<Shape>` / `Ptr<NumArray>` candidate pre-filters
record nothing).

Not a regression in any of them: `01_startup` genuinely has nothing to promote,
and `11_prime_sieve`'s `boolean[]` sieve is correctly refused by
`Ptr<NumArray>`'s numeric-element-type rule.
