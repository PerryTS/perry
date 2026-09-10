Apply after returning from temporary main checkout:

R3 must preserve resolve_materialized_array's cached_length refresh. Array push
cleans the lazy receiver before mutation, then mutates the backing Array; an
alias may retain the original lazy header. The existing lazy element lookup
refreshes cached_length on every read. In the new materialized branch, after
GC_TYPE_ARRAY/no-forwarding proof and before entering shared Array guards,
load the backing array length and store it to the lazy header's first u32.
This is pointer-free and noncollecting. Keep a separate admitted block so no
header mutation occurs when the materialized brand/forwarding proof fails.

Strengthen the compiled growth test with grownAlias=grown BEFORE pushes,
read grownAlias through id() after each push, and report its length after the
read. This ensures the test retains the lazy wrapper even when the push's
compiler path rewrites the primary local to the returned ordinary array.
Keep the same alias for prototype-hole and shrink checks.

Potential later large-input investigation (not implemented): main's 16MiB lazy
cap was chosen because iterate-all/materialize workloads favored DirectParser.
R2 scalar projection changes scalar-scan cost, but simply raising the cap risks
adding a tape pass before full-object scans. Measure ForceOn as a diagnostic
with BOTH scalar and mixed-field/full-consumption controls before considering
any policy change; don't extrapolate from scalar scans or parse-only results.
