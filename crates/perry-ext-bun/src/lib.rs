//! Perry's native `Bun.Transpiler` and in-memory `Bun.build` (#9602).
//!
//! Both need the same pinned SWC generation the compiler parses with, and
//! living here keeps the bundler out of `perry-runtime`, so apps that never
//! ask for runtime compilation do not carry it. Codegen links this crate
//! (well-known key `perry/bun-transpiler`) whenever a program emits a
//! `js_bun_transpiler_*` / `js_bun_build` call.
//!
//! The crate used to be `perry-ext-typescript` and also served a native
//! subset of npm `typescript` (`transpileModule` and
//! `flattenDiagnosticMessageText`). That binding was removed: npm
//! `typescript` compiles from source.

mod bun;
