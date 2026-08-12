### Emitted LLVM IR is run-to-run deterministic again (#7622)

Compiling one source twice with the same `perry` binary produced different IR.
Not a miscompile — but it defeats byte-level IR A/B, which is the primary
evidence the #7615 rooting-migration slices offer (three of nine apparent diffs
in #7620 were this, not the change), and it is a latent hazard on the object
cache. Both shapes were `std::collections` hash-map iteration order reaching the
emitter; Rust's default hasher is seeded per `RandomState`, so the order was a
fresh permutation every process. Six runs of each of the issue's two named
sources differed 5/5 from run 1 before the fix and 0/5 after.

**Rayon was not the cause.** #7303 suspected module-codegen completion order;
`ctx.native_modules` is a `BTreeMap<PathBuf, HirModule>`, and parallel codegen
is untouched here. Nothing else varied either — no register renumbering, no
block reordering, no metadata churn, no `Instant`- or address-derived value.

**Function-name registration** (`codegen/artifacts.rs`). The
`hir.closure_display_names` walk. Every entry mints a rodata constant through
`add_string_constant`, whose `@.str.N` counter numbers in first-use order, and
emits one `js_register_function_name` call into `__perry_init_strings_*`, so the
map's order set both. This is the same defect #7038 fixed 36 lines below it, in
the `closure_source_text` loop, and left standing here. Now sorted by `FuncId`.

**The dispatch towers** (`lower_call/property_get/dynamic_dispatch.rs`). The
`ctx.class_ids` walk that builds `implementors` — each surviving entry is one
`icmp`-guarded case block, so map order *was* arm order, and consecutive call
sites named different `perry_method_*` callees run to run. The virtual-override
tower (`vdispatch.*`) reads the same map the same way. Both now walk
`(class_id, class_name)` order, which is total because the name is the map key.

`max_explicit_arity`'s scan stopped at the **first** name carrying a class id,
and ids are not unique over `class_ids` keys — a class-expression self-binding
alias (`var X = class _X`) and an imported class registered under both its own
and its local-alias name each map two names to one id. So it was a hash-order
tie-break whose loser decides how many `TAG_UNDEFINED` padding args every
emitted call in the tower carries. It now takes the max over all names sharing
the id: what the variable already means, and the safe direction (under-padding
is the #235 garbage-argument bug; over-padding just lets a default-param
desugaring fire).

**The method-symbol registry** (`codegen/method_registry.rs`). The
`class_table.values()` walk writes into the `(class, method) -> symbol` map both
`insert` (last writer wins) and `entry().or_insert_with` (first writer wins), and
`class_table` mixes local classes, their alias keys and imported stubs — so two
distinct `&Class` can contend for one entry, in the table every call site
consults to pick its callee. Now walked by sorted key.

**The object cache was implicated.** `compute_object_cache_key` is a function of
`CompileOptions`, the post-transform HIR fingerprint, the perry version, a hash
of the perry binary and the codegen env vars — all deterministic — while the
emitted IR was not. Identical inputs therefore produced an identical key over
different `.o` bytes: a cache hit and a cold rebuild could hold different code.
For the shapes observed that difference is semantically neutral (the name
registry keys on distinct function pointers; tower arms key on distinct class
ids), but not by construction — the tower's `seen_pairs` dedups on
`(class_id, fname)`, which admits two arms sharing one class id with different
symbols, and there emission order *is* the behaviour. Same for the registry's
two tie-breaks. That hazard is now closed.

**Tests** (`codegen/emission_order_tests.rs`, `--lib`, so they run on every PR
touching perry-codegen rather than only in the tag-gated integration tier). Four
cases over the two shapes with a reproducing fixture. Each builds its `Module`
fresh per compile — the offending maps live in the HIR, so double-compiling one
long-lived `Module` would re-iterate the same `RandomState` and pass
unconditionally — and each asserts its subject was live (registration count,
tower arm count) before judging order. Sabotage-verified in both directions:
reverting either sort alone turns exactly that shape's two tests red and leaves
the other shape's green. The virtual-override tower and the `method_registry`
walk are sorted with **no** test and labelled untested in the module prose:
`vdispatch` blocks appear in zero of 41 sampled `test_gap_*` programs (every
receiver-typed call measured is claimed earlier by `method_override.rs`'s
`method_direct` shape guard), and a green test over a fixture that emits no arms
asserts nothing.

Validated locally: a 42-program sweep compiled 3x each is 0 nondeterministic /
41 compared; `cargo test -p perry-codegen --lib` 695 passed; the
gc-root-dominance corpus is green in both gated modes over 149 files with 40/40
seeded violations caught.
