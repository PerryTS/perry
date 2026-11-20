Fix long GC pauses on large graphs of objects with the same shape. Shared keys
and prototype words now use their existing shape root scanners for remembering,
while receivers continue to mark and rewrite them. This removes redundant
per-receiver external entries and their quadratic replay without changing
ordinary external-buffer coverage. GC traces now report remembered-set restore
time and include it in full-cycle reclaim time.
