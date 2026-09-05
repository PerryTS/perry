Gave the full-outline generic property get (`js_object_get_field_ic`, #5391
path 3) the monomorphic inline-cache hit the inline diamond has. The outlined
helper observed typed feedback and then called `js_object_get_field_ic_miss`
unconditionally on every read of every heap receiver, so on a module past the
full-outline threshold — every minified bundle — the per-site cache was written
by every property read and consulted by nobody. Measured on the compiled
claude-code TUI, one 400-character reply: entries to the miss handler
2,725,376 → 649,216 and primes 2,180,102 → 114,732 over the same ~12,330 sites.
Guards mirror the emitted diamond's one for one; anything the hit path declines
still reaches the handler. `PERRY_IC_OUTLINE_FASTPATH=0` restores the previous
behaviour for measurement.

Added `PERRY_IC_DIAG`'s prime split: every `pic_prime_get` is classified as
re-priming the token the site's MRU entry already held, priming a token that
was already in one of the four ways, or priming a genuinely new shape, with a
`PIC_WAY_STATE` census at prime time — globally and per site.
