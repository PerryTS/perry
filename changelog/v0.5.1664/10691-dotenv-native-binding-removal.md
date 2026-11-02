Removed the native `dotenv` binding: `dotenv.parse(Buffer)` (the idiomatic
`dotenv.parse(fs.readFileSync(...))`) returned 0 keys, and `config()`
reported no error but populated neither `result.parsed` nor `process.env` —
a silent total no-op. `import dotenv from "dotenv"` (no
`perry.compilePackages` entry) now compiles the real npm package from
source, matching Node exactly.

Deleted both duplicate hand-written implementations
(`crates/perry-ext-dotenv` and `crates/perry-stdlib/src/dotenv.rs`, which
independently exported the same `js_dotenv_*` symbols — #10678) and removed
`"dotenv"` from `PERRY_NATIVE_EXTENSION_PACKAGES` so the real package's
source (including the `dotenv/config` auto-load subpath) reaches the module
walker instead of being skipped as "handled by native stdlib". Also fixed a
standalone-workspace release fixture
(`tests/release/packages/next-app-route/provider/stdlib/Cargo.toml`) that
referenced the now-deleted `bundled-dotenv` feature.
