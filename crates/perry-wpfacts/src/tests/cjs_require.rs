//! `require("<literal>")` in a CommonJS-wrapped module takes the one module
//! the wrapper resolved it to, not every arm of the synthetic `require` plus
//! the computed-specifier registry behind them.

use super::*;

// The shape `cjs_wrap` emits: the `node:module` import that marks a wrapped
// module, one arm per statically resolved specifier naming the target's
// absolute path, and the registry fallback for computed specifiers.
const WRAPPED: &str = r#"
    import { createRequire as __perry_cjs_create_require, isBuiltin as __perry_cjs_is_builtin } from 'node:module';
    class Left { x = 1; }
    class Right { x = "right"; }
    __perry_register_path_module("/cjsq-left.js", { exports: new Left(), __perry_cjs_record: true });
    __perry_register_path_module("/cjsq-right.js", { exports: new Right(), __perry_cjs_record: true });
    function __perry_cjs_refresh_path(path, local) { return __perry_require_path_module(path); }
    function __perry_cjs_factory() {
        function require(specifier) {
            if (specifier === './left') { const c = require.cache["/cjsq-left.js"]; if (c !== undefined) return c.exports; return __perry_cjs_refresh_path("/cjsq-left.js", undefined); }
            if (specifier === './right') { const c = require.cache["/cjsq-right.js"]; if (c !== undefined) return c.exports; return __perry_cjs_refresh_path("/cjsq-right.js", undefined); }
            return __perry_require_path_module(specifier);
        }
"#;

/// The module body runs inside the factory, after the helper.
fn wrapped(body: &str) -> String {
    format!("{WRAPPED}\n{body}\n}}\n__perry_cjs_factory();")
}

#[test]
fn static_require_takes_one_module() {
    let pf = facts(&wrapped("const left = require('./left'); console.log(left.x);"));
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "x").collect();
    assert_eq!(sites.len(), 1, "{}", pf.dump_text());
    assert_eq!(sites[0].verdict, Verdict::T0Mono, "{}", pf.dump_text());
    assert!(
        sites[0].receivers.iter().all(|r| r.ends_with(":Left")),
        "{:?}\n{}",
        sites[0].receivers,
        pf.dump_text()
    );
}

#[test]
fn computed_require_keeps_the_registry() {
    // A specifier the wrapper could not resolve still sees every record.
    let pf = facts(&wrapped("function pick(n) { return n ? './left' : './right'; } const any = require(pick(1)); console.log(any.x);"));
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == "x").collect();
    assert_eq!(sites.len(), 1, "{}", pf.dump_text());
    assert_ne!(sites[0].verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

