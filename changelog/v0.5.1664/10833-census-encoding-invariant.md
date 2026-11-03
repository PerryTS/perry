Retargeted `scripts/shape_descriptor_census.py`'s generic-read-PIC emptiness
proof from an **instruction** to the **invariant** behind it.

The census required `icmp_ne(I64, &packed_word, "0")` to survive in the generic
read body — the "empty compact-cache rejection". #10833 removes that line, and
the census refused the tree.

**It was right to refuse, and the PR is right to remove it.** The unprimed
compact word used to be `0`, so `packed != 0` *was* the emptiness test. This
change moves the sentinel to `PACKED_GET_EMPTY = 0xFFFF_FFFF`, which lies
outside the valid ShapeId range `[SHAPE_ID_BASE, SHAPE_ID_END)` =
`[0x8000_0000, 0xC000_0000)`. An unprimed word's stamp therefore cannot equal
any valid `pcid`, so the token compare already rejects it and the separate test
is redundant. Under the new encoding, asserting `packed != 0` asserts nothing.

So the census now asserts the range relationship directly:

    PACKED_GET_EMPTY must lie OUTSIDE [SHAPE_ID_BASE, SHAPE_ID_END)

which is strictly stronger than the line it replaces: it survives the next
encoding change and it names the hazard — an unprimed site read as a hit —
rather than one spelling of the fix for it. A new `require_match` helper reads
the constants' values, because reasoning about a magnitude is what makes the
check encoding-independent.

The census's **sabotage self-test** was retargeted with it. It previously
planted `icmp_ne(..., "-1")`; it now plants `PACKED_GET_EMPTY = 0x9000_0000`,
i.e. moves the sentinel *into* the valid range — the exact mistake that would
let an unprimed site read as a hit. Verified independently of the self-test:
sabotaging the real constant gives

    empty sentinel 0x90000000 is INSIDE the valid ShapeId range
    [0x80000000, 0xc0000000) -- an unprimed site could be read as a hit

and the clean tree passes. The assertion was not weakened to accommodate the
change; it was moved onto the property the change preserves.
