**repsel: one Pass-3 safety gate and one write walker for both numeric proofs (#7788, follow-up to #7770/#7774).**

No behavior change. `prove_group_numeric_fields` had grown an independent copy
of the `'cand` loop's obligation set (`ctor_chain_safe`, `prototype_is_stable`,
field/method ambiguity, per-method `method_safe`); that verdict licenses a bare
unchecked `load double`, so two copies drifting is a miscompile rather than a
missed optimization. Extracted as `chain_this_flow_verdict`, the single
implementation both callers share. Likewise
`collect_numeric_by_construction_locals`'s hand-rolled write collector is gone:
`not_bigint_locals::collect_writes` now records a no-init `Let` as `None` (fine
for the non-BigInt fixpoint, fatal for the numeric one) and serves both.

Adds the coverage gap the review found: the `super(...)` parameter-resolution
path became reachable through the group MEET in #7770 and had no group-scope
test (every fixture was `extends: None`), so a wrong index there would have
granted an unsound claim silently. `super_chain_params_resolve_under_the_group_meet`
covers both directions. Also skips the group proof's this-flow walk when no
chain field is raw-f64-declared, and computes `group_members()` once per region
instead of twice.
