Numeric loops whose reassigned locals are proven to remain Numbers no longer
emit moving-GC back-edge polls. The loop-purity predicate now consumes the
existing whole-write `number_by_construction_locals` proof, preserving the
call-free counted-loop optimization without trusting erased TypeScript
annotations. The `loop_safepoint_purity` integration suite is restored to the
per-PR codegen map and its temporary #8263 exclusion is removed.
