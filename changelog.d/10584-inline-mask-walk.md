Walk an inline slot mask directly instead of re-entering the slot iterator once
per slot. `visit_gc_layout_slot_descriptors` called
`HeapChildSlotIterator::next` for every payload slot; for the common case — a
`Masked` selection whose mask is `LayoutSlotMask::Inline` — each of those calls
re-dispatched the selection, re-decoded the mask's niche and rebuilt the limit
and cursor masks, for about eight instructions of work.

The mask's set bits are the slot indices, in ascending order, so the arm takes
the word once and walks it with `trailing_zeros` and `word &= word - 1`. Every
other selection, including a `Heap` mask (more than 64 payload slots), keeps the
iterator. The helper carries the iterator's two side conditions with it: the
one-shot raw-numeric accounting that `next`'s first call performs, and the
cursor, left at the end so a later `next` yields nothing. The prefix and meta
edges belong to the caller, which takes them before the payload; the helper
asserts they are gone rather than arguing it.

Measured on a control whose pointer fields target DISTINCT objects, because the
older shared-child control let the collector's one-entry address memo answer
83.3% of its classifications against 0.0% on the real fixtures, and so hid the
cost of everything downstream of that memo. On it, `next` costs 75.8 of the
417.1 instructions a pointer-slot visit costs, and the walk removes 69.7 of
them. On the same control the shared-child version reports 75.9 — the iterator's
own cost is what the blind control did NOT distort.

The descriptor walk serves the copying minor, the full mark and the
remembered-set rebuild. Inclusive instructions for the walk on gc3, exact, by
caller: copying minor -7.95%, full mark -16.62%, remembered-set rebuild -17.59%,
dirty scan unchanged. On `oldyoung`, whose masked population is mostly one
`Heap` mask, the remembered-set rebuild and the dirty-coverage restore each pay
one failed `take_inline_mask_word` dispatch per visit: +0.25% and +0.33%, about
two instructions per object visit, against -4.49% on that fixture's copying
minor and -1.07% on the program.

Whole program, instructions:u, min of 5: gc3 -6.75%, w20000 -5.86%, w5000
-4.90%, w1000 -2.36%, oldyoung -1.06%, and an allocation-only fixture flat to
298 instructions in 320 million. No fixture regresses in instructions, peak RSS
or max GC pause.

The equivalence between the walk and the iterator is a property, and is tested
as one: identical index sequences for every mask word (empty, one bit at each
end, full width, both alternations, and 64 pseudo-random words) crossed with
every live slot count from 0 to 128, with a sabotaged twin that drops the mask's
top bit and must be caught, plus a real collection whose only young child hangs
off the highest masked slot and its own sabotaged twin.
