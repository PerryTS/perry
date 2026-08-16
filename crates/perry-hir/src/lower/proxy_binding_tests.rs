//! Verdict tests for [`super::LoweringContext::is_proxy_local`] — WHICH
//! lowering a member read / write / `in` check got, per function.
//!
//! These have to be verdict tests rather than output tests. `js_proxy_get` on a
//! non-proxy answers `undefined` instead of throwing, so the pre-#7775 bug was
//! silent: a program could keep printing plausible numbers while every read
//! through a mis-keyed receiver returned nothing. Byte-comparing output against
//! node covers the behaviour (`test-files/test_gap_proxy_local_name_collision_7775.ts`);
//! this file pins the *routing*, so a future change that quietly stops keying on
//! the resolved binding fails here even if some other path happens to paper over
//! the result.
//!
//! Every test carries a positive control in the same module — a function whose
//! read MUST still route to `ProxyGet`. Without it a test would pass if
//! `ProxyGet` stopped being emitted anywhere at all, which is CLAUDE.md's fourth
//! way a gate can be unable to fail.

#![cfg(test)]

use crate::Module;
use perry_diagnostics::SourceCache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed =
                perry_parser::parse_typescript_with_cache(&src, "proxy_binding.ts", &mut cache)
                    .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", "proxy_binding.ts")
                .expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

/// Debug rendering of one lowered function's body, by name.
fn body_of(module: &Module, name: &str) -> String {
    let function = module
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| {
            panic!(
                "function `{name}` is lowered (have: {:?})",
                module
                    .functions
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
            )
        });
    format!("{:?}", function.body)
}

/// True when this function's body routes any receiver through a proxy-only
/// operation — `ProxyGet` (`p.k`) or `ProxyHas` (`k in p`).
///
/// The write side is deliberately absent: proxy assignment lowers to
/// `Expr::PutValueSet`, which the generic assignment path also emits, so it is
/// not a proxy tell. Writes are covered byte-for-byte against node in
/// `test-files/test_gap_proxy_local_name_collision_7775.ts` instead
/// (`writesProperty`, which read back `undefined` pre-fix).
fn routes_to_proxy(body: &str) -> bool {
    body.contains("ProxyGet") || body.contains("ProxyHas")
}

/// The core of the bug: a name bound to `new Proxy(...)` in ONE function made
/// EVERY function's `<name>.prop` a proxy read. The proxy's own function never
/// had to run — only the spelling had to match.
#[test]
fn a_same_named_plain_object_in_another_function_is_not_proxified() {
    let module = lower(
        r#"
        function neverCalled(): number {
          const a: any = new Proxy({ v: 1 }, {});
          return a.v;
        }
        function readObj(): number {
          const a = { v: 42 };
          return a.v;
        }
        console.log(readObj());
        "#,
    );
    // Positive control: the genuine proxy still takes the proxy path, so a
    // green verdict below means "keyed correctly", not "feature switched off".
    assert!(
        routes_to_proxy(&body_of(&module, "neverCalled")),
        "the genuine proxy binding must still route to a proxy operation"
    );
    assert!(
        !routes_to_proxy(&body_of(&module, "readObj")),
        "a plain object that merely shares the proxy's spelling must not be \
         routed through js_proxy_get — it answers undefined, silently"
    );
}

/// `record_var` in the pre-scan only matches `ast::Pat::Ident` declarators, so
/// a parameter, a `for...of` head and a destructured binding could never be
/// poisoned no matter what they held. Those are exactly the shapes here.
#[test]
fn parameters_loop_heads_and_destructured_bindings_are_not_proxified() {
    let module = lower(
        r#"
        function neverCalled(): number {
          const a: any = new Proxy({ v: 1 }, {});
          return a.v;
        }
        function fromParam(a: number[]): number {
          return a.length;
        }
        function fromLoopHead(): number {
          let total = 0;
          for (const a of [{ v: 1 }]) total += a.v;
          return total;
        }
        function fromDestructuring(): number {
          const box = { a: { v: 42 } };
          const { a } = box;
          return a.v;
        }
        function writesAndChecks(): boolean {
          const a = { v: 1 };
          a.v = 5;
          return "v" in a;
        }
        console.log(fromParam([1]), fromLoopHead(), fromDestructuring(), writesAndChecks());
        "#,
    );
    assert!(
        routes_to_proxy(&body_of(&module, "neverCalled")),
        "the genuine proxy binding must still route to a proxy operation"
    );
    for victim in [
        "fromParam",
        "fromLoopHead",
        "fromDestructuring",
        "writesAndChecks",
    ] {
        assert!(
            !routes_to_proxy(&body_of(&module, victim)),
            "`{victim}` holds no proxy — it must not be routed through one"
        );
    }
}

/// The same bug pointing the other way. The pre-scan's `walk_stmt` descends into
/// function DECLARATIONS only, so a proxy declared inside a class method or an
/// arrow body was never registered and its traps never took the fast path.
/// Keying on the resolved binding registers it at its declarator, wherever that
/// declarator sits.
#[test]
fn a_proxy_declared_in_a_class_method_or_arrow_body_is_registered() {
    let class_module = lower(
        r#"
        class Holder {
          run(): number {
            const m: any = new Proxy({ v: 1 }, { get: () => 7 });
            m.w = 5;
            return m.v;
          }
        }
        console.log(new Holder().run());
        "#,
    );
    assert!(
        format!("{class_module:?}").contains("ProxyGet"),
        "a proxy declared in a class method must reach the proxy path"
    );

    let arrow_module = lower(
        r#"
        const inArrow = (): number => {
          const r: any = new Proxy({ v: 1 }, { get: () => 8 });
          return r.v;
        };
        console.log(inArrow());
        "#,
    );
    assert!(
        format!("{arrow_module:?}").contains("ProxyGet"),
        "a proxy declared in an arrow body must reach the proxy path"
    );
}

/// A module-level proxy read from a function lowered BEFORE its declarator. The
/// module-var pre-registration pass hands that function a resolved `LocalId`,
/// so `is_proxy_local` consults `proxy_local_ids` rather than the name fallback
/// — the id has to be seeded there at pre-registration or the forward reference
/// silently loses the proxy path.
#[test]
fn a_forward_referenced_module_level_proxy_keeps_the_proxy_path() {
    let module = lower(
        r#"
        function readsLater(): number {
          return modProxy.v;
        }
        const modProxy: any = new Proxy({ v: 1 }, { get: () => 64 });
        console.log(readsLater());
        "#,
    );
    assert!(
        routes_to_proxy(&body_of(&module, "readsLater")),
        "a module-level proxy referenced from an earlier-lowered function body \
         must still route to a proxy operation"
    );
}
