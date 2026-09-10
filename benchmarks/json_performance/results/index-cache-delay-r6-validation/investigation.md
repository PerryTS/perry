# R6 hypothesis, not yet measured

R5 ARM64 ordinary-array branches execute the Array-subclass inline-cache slot
load and safe-sentinel selection before receiver/index/brand guards. Only the
shape-carried Object tier uses that pointer. Move the load and select into that
tier; the fallback helper still receives the static cache slot address.
No runtime, GC, representation, or benchmark changes. Compare exact R5 runtime
archives and source hashes. Inspect final ARM64 to establish actual sinking;
then interleave main/R5/R6 access and focus CPU/RSS. All other rows remain in
scope. Rejected R5 remains available on its pushed branch.
