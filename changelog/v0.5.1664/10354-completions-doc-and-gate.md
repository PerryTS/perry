**The A/B's liveness gate was weaker than its own comment claimed.**

`LoopStats::completions` was documented as *"Completions dispatched to a P1 net
subsystem. Zero means turnloop carried no I/O for this process, whatever the
turn count says."* It is not. `record()` does
`stats.completions += self.completions.len()` — the driver's whole turn output,
summed across **every** class (P1 net, P2 process, P3 JS timers, P4 pool)
**before** `dispatch_staged` routes any of it.

That mattered because `scripts/turnloop/server_ab.py` quoted those exact words
as the justification for its "turnloop really carried the I/O" check. A server
that declined its listener to hyper but armed a keep-alive deadline would have
produced a non-zero count and **passed**. And until the idle-timer park landed
in this same PR, an HTTP keep-alive connection produced *two* timer completions
per request — so that was not a hypothetical shape, it was the shape.

Both halves are fixed. The doc comment now says what the counter counts and
points at `turnloop_net::census` for a per-class answer. The harness now reads
the census's `[perry-loop] p1 comp_*` line and requires a non-zero count among
`comp_accept`/`comp_read`/`comp_write`/`comp_connect` — classes that mean a
socket moved bytes — with the aggregate kept only as a fallback for a binary
older than the census, which says so when it uses it.

Self-tested against a synthetic census carrying 40 timer completions and zero
socket completions: the old check accepted it, the new one rejects it.

The lesson is the one CLAUDE.md already records under "a gate must assert its
subject was live": this gate ran, and its subject was *almost* the right
quantity. An instrument is only as good as the counter it reads, and a comment
is not a measurement.
