### Fixed

- **gc:** a keys-array family's descriptor list no longer memmoves its whole
  tail on every removal, and no longer scans linearly to find an id.

  `IdList::remove` was `Vec::remove(pos)`, which shifts everything past the
  removed position. Measured on the compiled claude-code TUI, one 3300-char
  reply, ten draws across two hosts: the removals sit at position **~0.31** of
  the list — i.e. essentially always the front — and the longest list reaches
  **514,030** entries, so the same ~3.7 M removals memmove up to **848 GB** in
  a single turn. The removals come from the dead-owner prune
  (`prune_dead_owner_side_tables_post_trace`).

  No claim is made that this explains the turn's bimodal CPU: one draw moved
  335 GB and was as fast as one that moved 16 GB, so bytes moved is necessary
  but not sufficient for the slow mode. What is removed here is unambiguously
  wasted work; how much time that is worth is for the A/B to say.

  A spilled list now carries an `id -> index` map, built once it passes 32
  entries, and `families` removes through a swap-remove that moves one element
  regardless of position. `by_facts` keeps the order-preserving removal it
  needs (its first entry is the canonical answer for exact-facts interning) and
  is unaffected — measured at max length **1**, so it never builds an index.

  The same index also removes the linear membership scan in
  `family_push_back`, previously **6.2 %** of main-thread leaf samples.

  This does not address why one family reaches half a million descriptors,
  which is a separate defect and a separate change.
