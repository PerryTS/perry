### Performance

**The default-off `PERRY_GC_THIS_SET_CHECK` diagnostic no longer taxes every
dynamic call.** `js_implicit_this_set` is the save/restore boundary for the
implicit receiver around dynamically-dispatched calls. The diagnostic added
for the `#7803` investigation wrapped each TLS replacement with two
`OnceLock::get_or_init` mode checks, so an unset environment variable still
paid two atomic fast paths and branches per setter invocation. On the
closure-heavy `pipeline` workload that setter runs about 2.9 million times.

The investigation in `#8243` isolates this as the entire previously
unattributed `#8084` regression: with one compiler and per-arm runtime/stdlib
archives, `PERRY_NO_AUTO_OPTIMIZE=1`, and min-of-5 instructions retired,
`pipeline` falls from 3,868,625,816 to 3,718,178,431 instructions (**-3.89%**).
All ten runs produce byte-identical output, and the fixed arm spans only 0.08%
around its minimum. This accounts for the reported +3.95% regression; neither
the callee-rooting hunk nor the stack-map walker caused it.

The expired diagnostic and its now-unused GC header-coherence wrapper are
removed, restoring `js_implicit_this_set` to one TLS replacement. The setter's
contract now records that default-off diagnostics must stay off this hot path.
