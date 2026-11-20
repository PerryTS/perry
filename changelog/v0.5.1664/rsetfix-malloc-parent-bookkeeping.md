Reuse the malloc registry's exact ownership lookup when repairing remembered
coverage after a collection, removing the per-owner scan of every malloc
allocation. Unmarked, unswept malloc parents still retain their young edges;
the repair witness includes an uncovered capture page and a stale entry.

Decide dirty-slot custody at the parent/descriptor boundary and reuse the dirty
scan's old-or-malloc and strong-slot admission in the copier. This removes
per-slot parent and generation lookups, keeps old side buffers associated with
their owner, and batches old-page slot accounting across a parent's descriptors.
Malloc slots no longer probe the old-page metadata table, where they have no
entry. A negative control verifies that generation alone loses old side-buffer
custody. Exact malloc-registry activation was already presized on main.

Remove the old-page walk's per-object membership set by visiting selected pages
in order: each spanning object belongs to its first selected overlapping page.
The copying dirty scan reuses that uniqueness instead of building a second set
for old owners. Exact deduplication remains for external and fallback entries.
A spanning-object witness includes omitted first pages and clean gaps, with a
negative control that duplicates owners by visiting each page independently.

Presize the copying scan's coverage set from its current owner snapshot and
remove the global previous-cycle size estimate. Test/debug builds assert that
the table never grows while scanning. Reservation is bounded by the owners in
this cycle, with no retained peak or new collector state.
