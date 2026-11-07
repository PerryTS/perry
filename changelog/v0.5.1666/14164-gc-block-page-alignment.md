Runtime (wasm32-wasip2, Windows): every GC block now starts and ends on a
4 KiB metadata page, as it already did on Linux and macOS. Before, nursery and
old blocks there came from the allocator at 16-byte alignment, so old-page
defragmentation and idle compaction never saw a block's first page and moved
nothing. A collection that wanted a conservative stack scan the target cannot
run (the allocation-point valves on wasm) now moves no existing object.
