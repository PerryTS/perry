Removed the native `uuid` binding: it is missing `parse`/`stringify`
entirely (a roundtrip throws — `parse` returns `undefined`), and `NIL`
reads as `undefined` (`js_uuid_nil` existed in the deleted source but was
never wired into either the `NativeModSig` dispatch table or the API
manifest). `import { v4, parse, stringify, NIL } from "uuid"` (no
`perry.compilePackages` entry) now compiles the real npm package from
source, matching Node byte-for-byte.

Deleted both duplicate hand-written implementations
(`crates/perry-ext-uuid` and `crates/perry-stdlib/src/uuid.rs`, which
independently exported the same `js_uuid_*` symbols — #10678).

`crypto.randomUUID()`/`randomUUID({ v7: true })` and nodemailer's
message-id generation both call the `uuid` Cargo crate directly and
unconditionally, with no feature gate — the dependency in
`perry-stdlib/Cargo.toml` was declared `optional = true` behind the
now-removed `bundled-uuid` npm-binding feature but was never actually
optional in practice. Made it a required dependency so those two call
sites keep compiling once `bundled-uuid` is gone; verified
`crypto.randomUUID()` still works.
