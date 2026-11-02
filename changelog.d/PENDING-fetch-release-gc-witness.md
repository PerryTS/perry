Verify fetch method-cache GC relocation through the cached closure's actual
pointer change in every test profile. The plain release runtime dependency
intentionally compiles diagnostic counters out; reading that counter as the
liveness witness made the stdlib test fail before checking the closure itself.
