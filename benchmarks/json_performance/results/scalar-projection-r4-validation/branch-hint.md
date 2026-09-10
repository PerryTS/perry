# R4 branch hint

Implementation 479cea228477675bcaf2213da6b93d569bd523b4 adds llvm.expect.i1 to the existing LazyArray comparison and declares the intrinsic. It expects false only as optimization guidance; both actual branch outcomes retain the existing checks and runtime fallback. LLVM handles llvm.* as noncollecting in Perry's existing classifier. No runtime, GC policy, layout, cache or output representation change relative to R3.

LLVM documents lowering expect into branch weight metadata, including propagation through branches/switches. Primary source consulted: https://llvm.org/doxygen/LowerExpectIntrinsic_8cpp_source.html . Installed compiler backend is LLVM 22.1.4; compiled machine-code inspection and qualified measurements must prove this candidate works on that backend, not an inference from the documentation.

R3 machine code reordered the JSON comparison before ordinary Array/Object. The R4 validation must check the optimized ordinary loop, exercise lazy true and false outcomes in the existing functional fixture, retain native/shadow root checks, then compare all targeted parse/stringify/access/RSS controls. A source hint alone proves no speedup.

A separate tiny-stringify crossover links hash-verified main worker.o with frozen R3 runtime.a under identical cc flags. No existing runtime declaration changed between main and R3 (only the new scalar helper was added); this tiny diagnostic uses eager Object/Array layouts that remain unchanged. It is not a coherent R4 product build. Normal/scheduled/full-GC outputs and full checksum match Node. The scheduled arm protects 106 retired sets and moves 12720 objects. If tiny stringify still regresses, compare this diagnostic to main/R3/R4 before attributing the cost to generated code or runtime.
