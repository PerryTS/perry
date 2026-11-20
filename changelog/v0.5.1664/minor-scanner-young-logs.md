Copying-minor scans of shape descriptors and captured-variable boxes now walk
only entries that can still expose non-old GC pointers. This removes the two
largest table-size-dependent root-scan costs, while full collections retain
their authoritative whole-table walks. `PERRY_GC_DIAG=1` also reports the
whole copying-minor pause and its scanner share on each completed-minor line.
