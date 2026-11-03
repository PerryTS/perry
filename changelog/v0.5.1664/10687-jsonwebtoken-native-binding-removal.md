Removed the native `jsonwebtoken` binding (#10683): `verify()` returned
`null` instead of throwing on every forgery case (tampered payload, wrong
secret, `alg:none`, garbage token, tampered signature, expired token), and
`sign(..., { expiresIn: "1h" })` silently dropped the expiry. `import jwt
from "jsonwebtoken"` (no `perry.compilePackages` entry) now compiles the
real npm package from source, matching Node exactly including all six
thrown error names/messages.

Deleted both duplicate hand-written implementations (`crates/perry-ext-jsonwebtoken`
and `crates/perry-stdlib/src/jsonwebtoken.rs`, which independently exported
the same `js_jwt_*` symbols — #10678) plus the dedicated codegen lowering
path in `crates/perry-codegen/src/lower_call/native/jsonwebtoken.rs` that
bypassed the well-known-binding registry entirely. Re-wired `dep:rsa`/
`dep:spki` directly onto perry-stdlib's `crypto` feature, since WebCrypto's
`key_object.rs`/`keys.rs` need them unconditionally and were only reachable
through the now-deleted `bundled-jsonwebtoken` feature by historical
accident.
