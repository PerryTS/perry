Removed the `cron`, `exponential-backoff`, `moment`, and `node-forge` native
bindings (`perry-ext-cron`, `perry-ext-exponential-backoff`,
`perry-ext-moment`, `perry-ext-node-forge`) — real npm source for all four
now compiles cleanly via `perry.compilePackages` and matches
`node --experimental-strip-types` byte-for-byte on a fixture exercising
each package's primary documented use (CronJob scheduling + CronTime
parsing; `backOff` retry/predicate/exhaustion paths; parse/format/
arithmetic/diff/duration; RSA keygen + X.509 build/sign/verify + PEM
round-trips). Each package's hidden `perry-stdlib`-side duplicate
implementation is removed too (`cron.rs`, `exponential_backoff.rs`,
`moment.rs`).

Five other packages probed in the same batch (`cheerio`, `ioredis`,
`nodemailer`, `undici`, `ws`) are **not** removed — each has a concrete,
reproduced blocker (see PR #10795): a callable-object-with-static-properties
invocation bug in cheerio's `$` factory; `ioredis`'s well-known binding key
and internal `Redis`-class dispatch key are shared load-bearing
infrastructure for `iovalkey`/`redis`, which are staying; nodemailer's
`class XOAuth2 extends Stream` against the bare `node:stream` `Stream`
still throws (confirms the #10649 follow-up gap); undici's `request()`
throws before attempting a TCP connect; and ws's `WebSocketServer` throws
right after a correct handshake, before the `'connection'` event fires.
