Use 2 MiB usable Linux GC blocks in 2 MiB-aligned OS regions. Derive census
bitmap capacity from block geometry and advise nursery backing as huge-page
eligible only after in-place promotion commits. Charge the existing block
pool cap for mapped extents, including rounded tails.

This unifies usable block geometry with the OS huge-page unit without adding
a pointer registry or changing object layouts or marking. Preserve whole-arena
GC headroom after small parses by comparing with that arena threshold instead
of the nursery cap; block rounding no longer rearms an already satisfied minor.
