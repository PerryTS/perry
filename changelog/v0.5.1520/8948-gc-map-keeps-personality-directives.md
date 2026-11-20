Fixed a Linux crash on the first caught `throw` in any program whose
`try`/`catch` spans more than one module or codegen unit: the process died
with a general-protection fault inside `_Unwind_RaiseException` (`call *%rax`
with a garbage `rax`) during module init. Coop's Next.js dylib hit it on
every start on x86-64 Linux; a two-module `.ts` program with one `try` in
each module reproduces it as a plain executable.

Root cause is in the compact GC-map rewrite (`gc_map.rs`), not in the linker
or the personality routine. `compact_stack_map_asm` treats every line from
the `.llvm_stackmaps` section switch up to the next section switch as the
stack map and replaces it. LLVM's `AsmPrinter` finalization prints the ELF
personality slot's attributes — `.hidden DW.ref.perry_eh_personality` and
`.weak DW.ref.perry_eh_personality` — right after the stack map and *before*
`.section .data.DW.ref.perry_eh_personality,"awG",…,comdat`, so both lines
fell inside the replaced range and were dropped. The assembler then defined
the COMDAT slot as a LOCAL symbol (`readelf -Ws` on any cached `.o`:
`OBJECT LOCAL DEFAULT DW.ref.perry_eh_personality`, where clang on the same
IR gives `OBJECT WEAK HIDDEN`).

That is fatal at every multi-object link. GNU ld keeps one COMDAT group per
program (also in the `ld -r` merge of split codegen units) and, because a
reference to a symbol in a discarded group is only redirected for *global*
symbols, the other objects' CIE personality relocations resolve to nothing —
silently, since `.eh_frame` is exempt from the "defined in discarded section"
diagnostic. `readelf --debug-dump=frames` on the linked image shows one CIE
with a real `DW_EH_PE_indirect|pcrel|sdata4` personality and every other
`zPLR` CIE carrying junk (`9b 18 00 00 00 …`, `9b 00 00 00 00 …`). The
unwinder decodes that junk as the personality pointer for any frame owned by
those objects and calls it. Mach-O never had the problem (no `DW.ref` slot,
no COMDAT), which is why the macOS arms and single-file gap tests stayed
green.

The rewrite now carries every zero-width line inside the block that does not
name `__LLVM_StackMaps` through verbatim, in its original position: symbol
attributes LLVM printed ahead of a section switch, and the `-O3`
absolute-symbol assignments (`perry_null_guard_zero = …`) that were parsed
as zero bytes and then lost the same way. Unit tests pin the x86-64 ELF
shape (attributes re-emitted exactly once, before the slot's section; the
map label's own attributes still dropped; assignments re-emitted).

Verified on Ubuntu 24.04 / x86-64 / LLVM 22.1.8 / binutils 2.42: the
two-module reproducer segfaulted before and prints its result after; the
rebuilt objects carry `WEAK HIDDEN DW.ref.perry_eh_personality`; Coop's
75 MB Next.js App Route dylib (split codegen units merged with `ld -r`)
initialises and serves `200 OK` in the daemon, one app and three apps.
