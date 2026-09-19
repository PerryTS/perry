Removed the native `nanoid` binding: `customAlphabet(alphabet, size)` is
documented to return a generator function, but the native implementation
returned the generated id string directly, so the only documented usage
(`const gen = customAlphabet(...); gen();`) crashed with `TypeError: value
is not a function`. `import { nanoid, customAlphabet } from "nanoid"` (no
`perry.compilePackages` entry) now compiles the real npm package from
source, matching Node exactly.

Deleted both duplicate hand-written implementations
(`crates/perry-ext-nanoid` and `crates/perry-stdlib/src/nanoid.rs`, which
independently exported the same `js_nanoid_*` symbols — #10678).
`customAlphabet` turned out to have no call-site wiring anywhere in
`perry-codegen` at all — only plain `nanoid(size)` had a dispatch row,
consistent with the reported crash.
