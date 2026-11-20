### Fixed

- Make direct in-process LLVM construction apply Perry's finalized precise-root lowering before RS4GC in both single and split codegen units, restoring byte-identical text/native objects without weakening the differential oracle (#8052).
