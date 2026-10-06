RegExp literal evaluations share immutable compiled matcher data through the existing per-site word while allocating a fresh ordinary object with its own lastIndex. Workers use their per-thread compile cache. Site words use existing mutable global roots and the root-store barrier, so moving GC rewrites their data edge.

Remove the literal/factory test wrappers, factory identity plumbing, active factory stack and site header table. Factory calls and direct literal test calls use ordinary method sites. Add fresh identity, independent state, compile isolation, worker and moving-root witnesses, including both required sabotage controls.

Grow catch snapshots with the thread's actual nesting depth while keeping jump buffers fixed. This avoids an extra mimalloc segment after removing the factory savepoint; the existing scanner continues to visit the active initialized prefix.

Ordinary accessor read sites refresh after their weak receiver ShapeId retires instead of counting the expired shape as polymorphic churn. This keeps flags Gets on short-lived literal instances cached across collection; holder and accessor-lane validation remain authoritative.
