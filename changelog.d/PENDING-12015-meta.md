Use seeded ConstFn shapes as the authority for completed method-record
publication. Reuse their key prefix and immutable body metadata, while checking
the receiver's current closures and retaining bootstrap validation for unseeded
reconstruction. Compiled-body ABI facts avoid redundant native admission probes.

Read ordered key and slot bounds from borrowed shape records. Internal key
reads honor shape prefixes, consumed fronts and holes without JS array probes.
Object.keys uses shape-owned non-enumerable metadata; effective class descriptor
resolution and values/entries snapshot checks remain intact.

Five interleaved trials against a separately built current-main arm reduce
instructions on all seven real-program drivers. Qs stringify improves 4.02%;
the method-factory micro improves 39.92%, for-in 20.76%, and keys 3.38%.
An isolated ConstFn-only arm attributes 4.01% of the QS improvement to the first
fix. All program and gap outputs match Node 26.5.1. Runtime, stdlib and codegen
have no new failures against main; five new unit tests have observed red
sabotage variants. Test counters are isolated with per_test_global! and excluded
from production builds.
Native, shadow and dependency GC dominance checks pass their existing CI floors
and budgets; all configured planted violations are caught. Owned build targets
were removed after retaining the measurements and verification logs.

RSS medians have small increases on four drivers (up to 1 MiB) and a 1.34 MiB
reduction on QS stringify, so the literal all-negative RSS/symmetric-window goal
is not fully satisfied. Descriptor reflection's micro rises 0.19%.
Full attribution, before/after/Node micro rows, program medians and baseline
failure lists: `docs/perf-12015-attribution.md`. Versions are unchanged.
