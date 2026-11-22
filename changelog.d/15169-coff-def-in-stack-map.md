Windows: a module whose stack map was followed by a COFF symbol definition
(`.def sym; .scl 2; .type 32; .endef`, which LLVM prints for the next function
ahead of its section switch) was refused with "unrecognised directive `.def`
inside the stack-map block". `.def`, `.scl` and `.endef` are now zero-width
and carried through the compact-map rewrite.
