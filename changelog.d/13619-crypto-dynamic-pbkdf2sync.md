`pbkdf2Sync`, `argon2Sync` and `argon2` reached as values — a CommonJS
`const { pbkdf2Sync } = require("node:crypto")`, or any program where
`require("node:module")` enables the CJS interop path — returned `undefined`,
or never called back: they resolve through `js_crypto_native_dispatch` instead
of the static call lowering, and the dispatcher had no arms for the three
names. It now calls the same runtime helpers as the direct lowering, derives
PBKDF2 from the validated iteration count and key length (an INT32-tagged
argument silently derived with zero rounds before), and validates the argon2
algorithm as a string.

PBKDF2's digest argument is now validated on both paths the way Node validates
it: an omitted digest is `ERR_INVALID_ARG_TYPE` instead of a SHA-256 key, and a
name Perry does not implement is `ERR_CRYPTO_INVALID_DIGEST` instead of
silently deriving SHA-256. `md5` is implemented, since Node accepts it and
`crypto.getHashes()` already advertises it. Node's remaining digest set
(sha3-*, shake*, blake2*, ripemd160, …) reports the same error rather than a
wrong key.

`@effect/sql-pg` calls `pbkdf2Sync` by name during SCRAM-SHA-256
authentication, so Postgres password logins failed in a compiled CommonJS
dependency of the Supabase CLI.
