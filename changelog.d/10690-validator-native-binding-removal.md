Removed the native `validator` binding: 9+ methods threw "not implemented"
(`trim`/`contains`/`equals`/`isAlpha`/`escape`/`isMobilePhone`/etc.), `trim()`
silently returned `undefined`, and the 5 implemented checks
(`isEmail`/`isURL`/`isUUID`/`isJSON`/`isEmpty`) returned `0`/`1` instead of
real booleans. `import validator from "validator"` (no
`perry.compilePackages` entry) now compiles the real npm package from
source, matching Node for all 50 checks.

Deleted both duplicate hand-written implementations
(`crates/perry-ext-validator` and `crates/perry-stdlib/src/validator.rs`,
which independently exported the same `js_validator_*` symbols — #10678)
plus `crates/perry-validation`, a shared grammar-helper crate consumed only
by the two duplicates. Also fixed a standalone-workspace release fixture
(`tests/release/packages/next-app-route/provider/stdlib/Cargo.toml`) that
referenced the now-deleted `validation` feature — it has its own
`Cargo.lock` and isn't a member of the main workspace, so `cargo check
--workspace` never covers it.
