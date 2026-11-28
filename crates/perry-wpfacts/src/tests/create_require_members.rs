//! Members of the `require` that `module.createRequire` returns.
use super::*;

/// `require.resolve` (and its `paths`) is a function of the loader the
/// realm does not declare: the solve models it as a native function.
#[test]
fn create_require_resolve_is_a_native_function() {
    facts(
        r#"
        import { createRequire } from "node:module";
        const req: any = createRequire(import.meta.url);
        console.log(typeof req.resolve, typeof req.resolve.paths, req.resolve("./x"));
    "#,
    );
}

