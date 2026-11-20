**Idle heaps now return arena capacity left behind by a burst.** General-arena
blocks deliberately need two full-GC observations before their mappings can be
released, but the idle reducer excluded its own collections from its activity
clock. A quiet heap therefore got one full and could stop forever with every
empty block only halfway through the page-return protocol (#9709).

Two consecutive post-collection samples at or below 50% utilization, above a
32 MiB capacity floor, now open a bounded arena right-size episode. The existing
idle-reclaim and page-return paths supply only the full observations still
needed, stop early once live data reaches 60% of capacity, and remain subject to
the reducer's quiet-time, rate, wake, and work-budget gates. A completed episode
stays disarmed until utilization reaches 70% or capacity grows materially
(at least 25% and 8 MiB), so a retained low live set cannot turn the idle timer
into a periodic full-GC loop.

On the compiled Claude Code 2.1.112 workload from the report, arena capacity
fell from 96.5 MB before the episode to 36.7 MB, then 35.7 MB and 35.7 MB
across a five-minute idle soak with about 23 MB live. RSS fell from 401 MB at
the first census to 132 MB at the last, and exactly one idle full was attributed
to arena right-sizing.
