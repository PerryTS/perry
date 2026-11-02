The fast x29-chain stack-map walker computed slot addresses from
`add x29, sp, #imm` alone, on the premise that establishing the frame pointer is
a function's last stack adjustment. It is not: LLVM emits a further
`sub sp, sp, #N` after the `add` when a function has a large or separately
laid-out local area.

    stp x29, x30, [sp, #0x90]
    add x29, sp, #0x90        <- fp established
    sub sp, sp, #0x170        <- body SP drops a further 368 bytes

Every slot in such a frame was therefore read 368 bytes high. `verify` reports
the fast walk and the unwinder visiting six slots each with five differing by
exactly that, and forcing `PERRY_STACKMAP_WALKER=unwind` raised objects copied
from 23 to 110 on the same probe — the wrong addresses were causing live objects
to be missed.

This is a silent wrong answer rather than a crash, and `verify` is not the
default, so an ordinary run has nothing to disagree with the fast walker.

The decoder now accumulates the contiguous run of `sub sp, sp, #imm` that follows
the `add`. A `sub sp` separated from that run is a body operation (a dynamic
alloca, a call-argument area) whose effect the stack map's own slot offsets
already carry, and is deliberately not folded in.
