Scoped direct-method descriptor invalidation to the receiver and relevant
prototype mutations, then inlined complete monomorphic ShapeId guards. On a
10,000-entity callback-heavy query with an unrelated descriptor, read-only time
falls from 3.7599 to 0.3381 ms/op and accumulation from 3.6702 to 0.2829 ms/op;
inlining contributes a further 7.4% and 8.1% paired reduction respectively.
