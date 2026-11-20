**`--opt-report`: the `Ptr<Shape>` allocation-site buckets now mean what they
are read as (#7170 R0).** Report-only — emitted LLVM IR is identical across the
**29 dependency modules that emit IR** out of 30 sampled (the 30th fails to
compile identically in both arms), each with a same-compiler control run, both
arms run *under* `--opt-report` — the only mode in which any of this code
executes — and both built from one pinned commit. Four defects, each of which
made a scheduler-facing number say something other than what it looked like:

- **De-duplication was masking rows, by a factor of 2.2.** Every anonymous
  object literal renders as `object literal { ... }` and carries
  `byte_offset: 0`, so every unbound literal in a function collapsed onto one
  `Entry::dedup_key`. Over 196 real `__esModule` dependency modules the
  `ptr-shape` allocation-site denial count goes **837 → 1842**. A new
  `alloc_ordinal` (the site's index in the region's deterministic walk)
  discriminates them; a function lowered twice walks the same body, produces
  the same ordinals, and still collapses. What the drain *did* collapse is now
  reported as `summary.masked_by_dedup` instead of left implicit.
- **`constructor argument` conflated two mechanisms.** A closed-shape literal
  lowers to `new __AnonShape_N(v0, …)` whose constructor arguments *are* its
  property values, so `{a: {b: 1}}` filed its inner literal as a constructor
  argument. Split into `constructor argument` (5 on the corpus) and
  `object literal property value` (418) — the old label was 98.8 % the latter,
  and the two need different fixes: a nested literal is a field value of a
  parent allocation that is itself unbound, so proving its shape licenses
  nothing on its own.
- **The `return` bucket counted syntactic sites, not opportunities.**
  `collectors/ptr_shape_returns.rs` (#7107) already admits a bare
  `return new C(...)` as a producer, but `deny_alloc_site` fires before any
  seeding. Those sites now carry their own rule string and a new `Tier::Served`
  so they leave the rule-1 bucket rather than inflating it.
- **`return` was a *label propagated down a subtree*, not a position.** It was
  set once at `Stmt::Return` and inherited by everything nested inside the
  returned expression, so `return cond ? {a} : {b}`, `return flag && {a}`,
  `return await {a}` and `return {a}.x` all counted as return positions.
  `scan_return` is now the only producer of a return position, servedness reads
  `NewSite::is_return_position` rather than comparing a label (so renaming a
  bucket cannot silently disable it), and nested allocations get their own
  bucket, `returned expression operand`.

The buckets are computed by the compiler (`summary.by_rule`,
`summary.by_alloc_context`) instead of being reconstructed downstream, which is
what made the second and third defects invisible: a `jq` reduction over
`entries[]` cannot see that one label means two mechanisms.

**The re-derived dependency-JS rule-1 wall is 1838 unserved allocation sites of
2028 `Ptr<Shape>` candidates (90.6 %)**, replacing #7152's `506 / 746` and
#7170's `963 / 1352`. The correction #7170 §5.2 expected to be material is not
— served returns are **4** sites — because only 62 of 1842 unbound allocations
are in a `function` region at all; 1711 (92.9 %) are in closures, which cannot
carry a return-shape fact. The instrument's real distortion was the dedup, and
it ran the other way: the wall is larger than reported, not smaller.

Census: `alloc_buckets` per workload keyed by the full `(analysis, allocation
position, denial rule)` tuple — the same identity the renderer uses, so the two
cannot describe different things — recorded as context and floored in code via
`ALLOC_BUCKET_FLOORS`, with `fixture_alloc_buckets.ts` written to land one
allocation in each bucket. The served row is the only layer that can catch the
wiring dying: it lives in `codegen/function.rs`, and every compiler unit test
sets the report scope by hand. No existing census floor moved and no existing
workload's candidate count moved: the distortion is invisible on hand-written
benchmarks and doubles the row count on real dependency JS, which is why it
survived.

Still open, measured but deliberately not fixed here: `statement` is the
largest bucket (455 of 1842, 24.7 %) and is a **catch-all** — the report walk's
default context, holding property stores, reassignments, conditional branches
and default parameters indistinguishably.
