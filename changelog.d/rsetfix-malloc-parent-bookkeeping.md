Reuse the malloc registry's exact ownership lookup when repairing remembered
coverage after a collection, removing the per-owner scan of every malloc
allocation. Unmarked, unswept malloc parents still retain their young edges;
the repair witness includes an uncovered capture page and a stale entry.
