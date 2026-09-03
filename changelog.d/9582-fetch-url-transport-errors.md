### Fixed

- **Global `fetch` now accepts WHATWG `URL` instances and reports Node-shaped
  transport failures (#9536).** Passing `new URL(...)` previously rejected
  immediately with `Error: Invalid URL`, which prevented Claude Code's MCP HTTP
  transport from reaching the network. URL inputs now contribute their `href`
  while native `Request` handles retain their existing dispatch path.

  Failed requests now reject with `TypeError: fetch failed` and carry the
  underlying failure as `cause`. DNS failures expose Node-compatible
  `ENOTFOUND`, `errno`, `syscall`, and `hostname` diagnostics. Error objects are
  constructed on the main thread and the inner cause stays rooted while the
  outer error allocates. A five-shape gap fixture covers string, URL, Headers,
  Request, and AbortSignal inputs byte-for-byte against Node 26.5.1.
