### Docs: #7510 measured out; `_tlv_get_addr` is the top remaining lever

All three of #7510's items are now answered by measurement rather than
argument. Item 1 shipped (#7535), item 2 shipped (#7525), and **item 3
collapsed on its own**: `layout_note_slot` costs **0.03%** on `churn_alloc`
(2 of 5,869 leaf samples), codegen emits **zero** `js_gc_note_slot_layout`
sites for that workload, and stubbing the function out entirely is worth
1.016x. The type-propagation work (#7550/#7552) and declaration-at-allocation
(#7501/#7532) removed the calls before anyone optimised them.

The plan's next lever is now `_tlv_get_addr` at **30.5%** of `churn_alloc` —
#7469's structural half. It has grown as a *share* because everything around
it shrank, not because it regressed.

Also records the campaign's most repeated failure mode: three times a ticket's
headline number was stale by the time it was worked (33.6% -> 11%,
14.5% -> 3.0% -> 1.7%, 7.5% -> 0.03%). Re-measure before scoping.
