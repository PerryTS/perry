Run the full-workspace release runtime test arm serially, matching the runtime's
shared-state requirement and the assertion-enabled debug arm. Concurrent arena,
hook and teardown tests could invalidate a guest endpoint test's actual GC
relocation witness.
