**GC: unreachable Proxies no longer retain their target graphs for the life of the thread.**

Proxy values are id-band handles rather than heap objects, so the collector previously had no
death signal for them: `PROXIES` grew append-only and its root scanner kept every target and
handler strongly alive. Full traces now observe proxy-band payloads through mutable roots,
registered runtime roots, heap fields (including pointer-free layout ranges), and incremental
mark barriers. Observed proxies recursively retain their target and handler; once a complete
full trace reaches sweep, unobserved registry entries become collected tombstones. Minor
collections keep the registry strong because they do not enumerate the whole heap.

Native proxy operations pin their receiver with a `RuntimeHandleScope` across trap calls that
can run user code and collect. Using a tombstoned handle raises a named collected-proxy
`TypeError` instead of silently losing proxy semantics or handing its payload to object code.
With `PERRY_GC_DIAG=1`, every completed full trace reports live entries, tombstones, entries
reclaimed in the pass, and a monotone reclaimed total. Five focused GC fixtures cover full
reclamation, minor strength, exact shadow roots, pointer-free heap ranges, and nested proxies.

Closes #8230.
