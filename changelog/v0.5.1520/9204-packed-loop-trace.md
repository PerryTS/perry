`match_packed_f64_versioned_loop`'s admission decision is a chain of independent
conditions, and when a loop unexpectedly stayed on the generic path there was no
way to tell which one declined it without reading the code and guessing — three
separate attempts on #9151 each found a different gate that way.
`PERRY_PACKED_LOOP_TRACE=1` names the rejection reason instead.

`packed_loop_reject` prints and returns `None` either way, so the emitted code
is identical with the flag on and off. It is registered in
`BUILD_CACHE_ENV_EXCLUSIONS` rather than `BUILD_CACHE_ENV_VARS` for that
reason: making it a cache input would cost every trace run a rebuild and buy
nothing.
