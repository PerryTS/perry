### perf(objects): settle eight-shape static write caches (#6812)

Static existing-field writes now retain four inline receiver-shape entries and
use a compact outlined helper for four more. Once all eight entries are full,
the cache stays settled instead of continuously evicting its fourth shape;
ninth and later shapes continue through full `[[Set]]` semantics without
unbounded code growth.

On the new 60-million-write eight-shape matrix cell, 15 alternating runs reduce
Perry's median from 2,359 ms to 768 ms (67.4%, 3.07×) with identical Node/Perry
checksums. Monomorphic timing is unchanged, four-shape timing stays within 2%,
and both linked matrix executables have the same file size.
