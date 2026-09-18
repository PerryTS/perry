### Faster

- Regular expression calls spend 305 to 332 fewer instructions each, 4.3% to 12.5% depending on the pattern. A `test` or `exec` call used to acquire the engine's program and subject views twice — once to learn the shape of the search and again to run it — and move the search between them; it now does both in one entry, and a search that its first quantum decides is never built as a resumable one at all (#10166).
