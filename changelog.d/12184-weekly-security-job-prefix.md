### Complete lowercase job names for weekly security

Use the stable `security-weekly` prefix for all four weekly-security jobs,
fixing the routing-contract assertion exposed by PR #12183. Apply suite-prefix
validation to every inlined job and suite result, including aliases outside
the workflow catalog. Execution settings and parallelism are preserved.

Validation: Actions topology and documentation checks passed. Hosted CI runs
on the PR.
