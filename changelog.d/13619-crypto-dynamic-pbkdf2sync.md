`pbkdf2Sync`, `argon2Sync` and `argon2` imported by name from `node:crypto`
returned `undefined` — or never called back — once any module in the program
imported `node:module`, which routes named `node:crypto` imports through
`js_crypto_native_dispatch` instead of the static call lowering. The dispatcher
had no arms for the three names. It now calls the same runtime helpers as the
direct lowering, validates the PBKDF2 integers and digest there (a missing
digest is an error, not a SHA-256 default), and validates the argon2 algorithm
as a string. `@effect/sql-pg` calls `pbkdf2Sync` by name during SCRAM-SHA-256
authentication, so Postgres password logins failed in every program that also
used `createRequire`.
