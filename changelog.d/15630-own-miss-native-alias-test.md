Tests pin the own-miss record-kind check: an object initialised by
`http.Server.call(this)` forwards a read that its own keys and prototype
chain prove absent to the native server, so the probe's own miss declines for
that kind instead of answering from the chain.
