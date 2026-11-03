`pbkdf2Sync`, `argon2Sync` and `argon2` reached as values — a CommonJS
`const { pbkdf2Sync } = require("node:crypto")`, or any program where
`require("node:module")` enables the CJS interop path — returned `undefined`,
or never called back: they resolve through `js_crypto_native_dispatch` instead
of the static call lowering, and the dispatcher had no arms for the three
names. It now calls the same runtime helpers as the direct lowering, derives
PBKDF2 from the validated iteration count and key length (an INT32-tagged
argument silently derived with zero rounds before), and validates the argon2
algorithm as a string. Both paths now also reject an omitted `digest` with
`ERR_INVALID_ARG_TYPE` as Node does, instead of deriving a SHA-256 key.
`@effect/sql-pg` calls `pbkdf2Sync` by name during SCRAM-SHA-256
authentication, so Postgres password logins failed in a compiled CommonJS
dependency of the Supabase CLI.
