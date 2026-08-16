Fixed the GC root-dominance moving-only audits so boxed string concatenation is
classified as poll-capable when it delegates non-string operands through
coercion and user code.

Issue #7872 exposed the missing edge: `js_string_concat_box` can call
`js_dynamic_string_or_number_add`, which can reach `js_number_coerce` and run
user code. The runtime call-graph classifier omitted the boxed-concat wrapper,
so moving-only dominance audits could incorrectly treat its callers as unable
to poll. An exact two-hop self-test now keeps that reachability visible.

Validation ran all four static GC audits, their self-tests and gate wiring,
plus the shadow and native root-dominance corpora. The shadow corpus passed
139/139 programs; the native corpus emitted 35,246 statepoints and 21,290 live
root bundles with zero dominance or unrooted-allocation hazards. Curated
before/after classification found no newly exposed stale window.
