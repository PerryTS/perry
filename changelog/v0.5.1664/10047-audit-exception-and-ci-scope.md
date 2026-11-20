Preserve exception-handler edges for cold property-cache miss calls. Only
audited `nounwind` attribute groups may suppress an `invoke`; a profitability
hint such as `cold` is not a no-throw promise. Add predicate and emitted-IR
regressions, retaining the existing throwing-getter, delegated-generator and
revoked-Proxy native cases as end-to-end checks.

Use complete, expected-head-pinned paginated PR file lists for the CI planner,
crate scope and native integration scope. Reject partial/duplicate listings,
API failures and concurrent PR changes instead of treating the first100
benchmark artifacts as the complete diff and silently skipping source tests.
