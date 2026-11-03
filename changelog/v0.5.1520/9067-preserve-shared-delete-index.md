Deleting a property no longer strands shape-index accelerators on sibling
objects that share the same keys array.

The delete path now clones a shared source's validated slot index onto the
deleting object's private, compacted keys array while leaving the original
index in place for siblings. Owned sources retain the cheaper move behavior.
This avoids forcing every untouched sibling to decode and hash the full key
set again on its next indexed lookup.

On the new 500-key shared-sibling benchmark, 15 interleaved A/B pairs pinned
to one CPU reduced the median measured loop from 171 to 70 ms (**−59.1%**);
the minimum improved from 111 to 43 ms (−61.3%). Every arm produced the same
checksum.
