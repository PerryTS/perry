Fold same-named executable literal images and weak static atoms as whole COMDAT sections on ELF and COFF. Preserve weak identity across modules and codegen units, section-based runtime discovery, and distinct mutable local-symbol atoms. Keep COFF iteration-only atoms live under /OPT:REF; Mach-O keeps its existing weak subsection coalescing.

Function-name metadata uses direct weak-image references on ELF and COFF, so its relative pointers resolve to the kept COMDAT.
