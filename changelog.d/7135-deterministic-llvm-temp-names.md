**codegen:** temp LLVM IR filenames are content-addressed, so two identical
compiles produce byte-identical objects on Linux ELF (#7131).

clang records the `.ll` source path into the emitted object. The previous
name mixed `pid` + wall-clock nanos + a counter, so every compile of the
same IR wrote a different path into the object (visible as ~10-byte diffs
on aarch64 Linux; Mach-O kept the noise in the debug map only).

The `.ll` basename is now `perry_llvm_<fnv1a64(ir)>.ll` — a pure function of
the IR bytes. The `.o` basename still carries a per-call counter so concurrent
rayon workers never race the clang output file (#509). Writes go through a
unique `.tmp` + rename so concurrent same-content producers never leave a
partial file.

This restores object-hash A/B on Linux (`repsel_census`, `census-knob-isolation
--require-emission`, and every "did emission change" claim).
