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
