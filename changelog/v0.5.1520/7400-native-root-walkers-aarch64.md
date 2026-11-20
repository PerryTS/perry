**Fixed** two aarch64 native-root walker defects that together crashed the
`PERRY_RS4GC` probe matrix on aarch64-Linux — `02_survivor_promotion` took a
SIGSEGV under forced evacuation (#7392). Neither was in statepoint lowering.

The x29 chain walk demanded a 16-byte-aligned frame record. AAPCS64 fixes what a
frame record *contains* and leaves where it sits in the frame unspecified; only
SP must be 16-byte aligned. LLVM's AArch64 **ELF** frame lowering puts the
`x29,x30` pair below the other callee-saved GPRs, so an odd number of those
lands the record 8 mod 16 — measured in the runtime frame that tripped it, which
saves x19..x23 and v8 and whose CFI reads `CFA = x29+56`. Darwin pins the record
to the top of the frame, which is why this could never fire on macOS. The walk
read a legal record as a corrupt chain and abandoned the stack mid-walk.

It abandoned into the platform unwinder, which resolved SP-relative roots
against `CFA - stack_size`. That follows DWARF's definition of a CFA and is
wrong for what `_Unwind_GetCFA` returns: inside an `_Unwind_Backtrace` callback
the CFA already *is* the stack pointer of the frame whose return address
`_Unwind_GetIP` reports, so every such root landed one whole frame too low.
Measured in-process on the failing probe: at the CFA the slot holds a NaN-boxed
pointer (`0x7ffd…`); 240 bytes lower — that frame's `stack_size` — it holds a
stack address. So the frame's real roots were never rewritten after an
evacuation, and the mutator dereferenced a stale from-space pointer.

`CFA_RETURN_ADDRESS_BYTES` is deleted: a standalone probe recording each frame's
real stack pointer and matching it against a live walk gives the same answer on
aarch64 Linux (libgcc), aarch64 macOS (Apple libunwind) and x86-64 Linux, so the
return-address convention never entered into it. That probe ships as
`unwind_cfa_is_the_frames_stack_pointer`, in `cargo-test` on every host.

`PERRY_STACKMAP_WALKER` had no arm anywhere in the tree, while the knob ledger
in `gc-native-roots.yml` claimed one — which is how both non-default walkers
carried bugs indefinitely, `verify` being the only check that can catch a wrong
root base at all (nothing downstream knows what a root slot should contain).
The workflow now runs every probe under `unwind`, and under `verify` on the
aarch64 arms, each byte-diffed against the pinned Node oracle. Measured on
aarch64-Linux: 2 of 11 probes passed all three walkers before this change,
11 of 11 after.
