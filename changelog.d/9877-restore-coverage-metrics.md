Correct the `[gc-restore-coverage]` diagnostic to report the actual old-page input (`dirty_old_pages`) separately from raw external entries and the covered-object skip set. The former `dirty_pages` field included external pages that the old-arena walk did not traverse.

Add admitted parent visits, enumerated slots, and strong slots whose children still require tracking. Slot productivity counts edges even when their page was already dirty; it is separate from `pages_added`. These counters compile out of the diagnostics-off walk, and collection/remembered-set behavior is unchanged.

A subprocess regression exercises diagnostics on and off, unequal old/external page inputs, duplicate stale external owners, skipped parents, mixed primitive/old/young slots, and repeated repair of an already-dirty page.
