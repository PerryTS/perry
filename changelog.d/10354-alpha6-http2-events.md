**Fix the branch: alpha.6's two new HTTP/2 events, and a duplicated state machine deleted.**

The turnloop 0.1.0-alpha.6 bump landed on `turnloop/integration` **red**.
`perry-ext-http` did not compile, and it was not checked: the bump was verified
against `perry-runtime`, `perry-stdlib` and `perry-ffi`, and not against the
heaviest consumer of `turnloop-http`. A bump that changes a protocol crate has
to be checked against the crates that use that protocol crate.

alpha.6 added two things to `http2::Event`:

**`HeadersKind` on `Event::Headers`** — `Head`, `Informational`, `Trailers`,
with the connection enforcing the distinction so that, in its own words, "a host
does not have to track `received_head` itself". Perry was doing exactly that, in
two hand-rolled ways: sniffing `:status` for a leading `1` to spot a 1xx, and
keeping its own `head_received` flag to spot trailers. Both are duplicated state
that can drift from the connection's, and both are now deleted in favour of the
field. `head_received` had exactly one reader — the branch it existed to drive —
so nothing else depended on it.

**`Event::Unprocessed`** — a stream the peer opened after a graceful GOAWAY,
above the id that GOAWAY named, with nothing sent for it. RFC 9113 §6.8 calls it
"not processed" and expects a retry on a new connection.

The handling is to do **nothing**, and that is the interesting part. Node emits
no frame at all here — no RST_STREAM, no GOAWAY, the session stays alive and the
request never reaches the application, measured against Node 26.5.1 with a raw
peer. turnloop-http deliberately declines to answer on the host's behalf, since
a frame it emitted could not be un-emitted.

It is worth recording that the opposite was believed here first. A
`RST_STREAM(REFUSED_STREAM)` reply was proposed on the assumption that Node
sends one; measuring against a raw peer showed it does not. The event is now
carried explicitly through the `Owned` mirror rather than swallowed by a
wildcard, so the decision to send nothing is visible in the match instead of
being an absence.
