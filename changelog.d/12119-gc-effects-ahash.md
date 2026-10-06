The GC call-effects classifier exempts aHash's random-source initialisation,
which made about 740 runtime symbols read as `Reenters`, `js_arena_alloc` among
them. The call-effects tables are regenerated.
