Add whole-program invariance facts (wpfacts), slice 1: the analysis and all
its checks, with no check removed from generated code yet. A new crate,
`perry-wpfacts`, runs one inclusion-based value-flow pass over the final HIR
of every module after the link-time shape ids. Anything it does not model
flows to one TOP node, and the `Expr` match has no wildcard arm. For every
class, object-literal shape and constructor-function body it computes field
writers and readonly-after-construction fields, definite birth keys, field
value kinds, typed-array stability, escape (E1-E25, B1-B4), prefix-stable and
fixed layout, receiver class sets and single method targets.

Codegen receives only a per-module digest of the facts it can observe, which
goes into the object-cache key; generated code is unchanged.
`PERRY_PROGRAM_FACTS=0` turns the pass off, `PERRY_FACTS_DUMP=<file>` writes
the facts (plus constraint relations with `PERRY_FACTS_RELATIONS=1`), and
`PERRY_FACTS_REPORT=1` prints the pass statistics and wall time.

A contract build (`PERRY_CHECK_CONTRACTS=1`) carries each module's claims in
a read-only `.perry_facts` section. It links perry-runtime's new
`contract-checks` feature (never in `default`), which aborts with the fact's
name when a HARD operation, a generic store, or a runtime writer contradicts
a claim. `test-files/wpfacts` holds one adversarial program per escape source
plus a positive control. `scripts/wpfacts_corpus.py` checks node parity, the
fact assertions, the contract build and the sabotage controls.
`scripts/wpfacts_recompute.py` re-solves the constraints independently, and
`scripts/wpfacts_oracle.py` checks the claims against node's observed
behaviour.
