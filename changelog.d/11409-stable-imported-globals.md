Make imported class ShapeId and object-literal producer global declarations
deterministic (#11409). Gather both capability families before emission and
sort/deduplicate their declarations, preserving local-candidate exclusion.
This removes randomized HashMap traversal from the emitted LLVM IR and avoids
spurious changes to IR comparisons and object-cache inputs.

Three codegen unit tests cover class-only, object-only and mixed capabilities,
including duplicate aliases and local candidates. Each asserts live declaration
counts and order, then compares full LLVM IR across fresh randomized maps.
All three fail on the original declaration loops.
