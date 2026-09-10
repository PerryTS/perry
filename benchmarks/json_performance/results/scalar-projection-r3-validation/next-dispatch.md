# Next dispatch investigation (not implemented)

R3 qualified access window has separated +5.59% 20MiB repeat and +0.97% 20MiB mixed-field CPU regressions versus main. This input exceeds the 16MiB lazy cap, so these rows use ordinary parsed Arrays. Main/R3 frozen access objects were disassembled with llvm-objdump --disassemble-symbols and relocations, with baseline object hash verified against its original provenance.

Concrete machine-code observation at repeat-loop brand dispatch: main compares Object (2) then Array (1), and loads the dense slot. R3 compares LazyArray (9) BEFORE Object (2) and Array (1), even though source emitter order puts JSON after ordinary brands. R3 also moves the selected array register before the shared Array guard. See main-access-run-assembly.txt addresses 0x2ef0..0x2f44 and r3-access-run-assembly.txt 0x3360..0x33c0. LLVM reordered the source branch chain. This disproves an assertion that unchanged source branch order guarantees unchanged ordinary machine dispatch.

Next candidate should investigate preserving an ordinary-type-first branch in optimized IR/machine code (existing branch weight infrastructure or a guarded dispatch shape), and then remeasure the SAME 20MiB controls alongside smaller JSON wins. Do not claim this explains the entire timing delta without a new controlled measurement. Do not change GC policy. Full-process instructions/cycles include parse/startup outside timed region, so they do not independently establish per-iteration instruction count.

The actual main native compiler reproduction remains a separate finding: two unsuppressed, inherited fixture hazards; worker native roots are clean. Large-input ForceOn diagnostics remain a later experiment with full/mixed-consumption controls, not justification to raise the cap.
