GC: an old-generation reclaim that comes due at an allocation now runs at the
next precise safepoint (a loop back-edge poll or the microtask-pump boundary)
instead of at the allocation behind a conservative scan of the whole native
stack. The allocation point still collects, with the scan, when old pressure
grows another growth band without reaching a safepoint, or when loop polls are
off. On tsc this removes all 244-251 conservative scans and cuts instructions
by about 1.6% and peak RSS by about 5%. With `PERRY_GC_INCREMENTAL=0` the
full-collection count is the same on every run (#11873).
