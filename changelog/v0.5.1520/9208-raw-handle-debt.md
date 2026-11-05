The raw-handle debt ratchet now counts empty
`RuntimeHandle::across_{mut,const,nanbox}(|| ())` wrappers as debt. Those
wrappers refreshed a handle across no work, so they were equivalent to a bare
pointer read while still receiving credit as a conversion.

All 17 existing no-op wrappers now use scoped handle access or put the real
allocation-capable operation inside `across_*`. The total baseline and every
per-module ceiling remain unchanged.
