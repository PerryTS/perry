macOS: the exception walker recognizes Perry's own handlers in binaries linked
by `ld64.lld`. For a DWARF-mode function, `ld64.lld` keeps the personality (in
the CIE) and the LSDA (in the FDE) only in `__eh_frame` and leaves the compact
unwind word's personality index and LSDA flag zero; Apple `ld` also copies them
into the word. The walker read only the word, so with `ld64.lld` a throw into
`perry_trap_call` / `perry_trap_call_base` (and other DWARF-mode functions with
Perry's personality) was not taken by the single-phase transport and fell back
to the system unwinder. It now reads them from the FDE when the word carries
none, the same path ELF targets use.
