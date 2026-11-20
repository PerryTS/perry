### Fixed

- A stream pipeline waiting on a pending promise no longer reads that promise from its old location after the jobs it runs trigger a moving collection (a `SIGSEGV` under from-space protection in `Readable.from(...).pipe(...)` drains).
- A promise the runtime marks handled on the user's behalf (WHATWG stream `closed` / `ready` promises, `Readable.toWeb` reads) keeps that mark when a collection moves it, so a caught rejection is no longer reported as `Uncaught (in promise)`. The mark is now a byte on the promise; the address set it replaced is gone.
