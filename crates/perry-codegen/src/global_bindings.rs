//! Which lazily born global bindings a module can observe (RB6N26).
//!
//! perry-runtime births a global function or namespace row (populate.rs `GLOBAL_BINDING_ROWS`, derived from the realm declaration)
//! only once `js_install_global_<name>` armed it, so a program that cannot observe the binding links
//! none of its code. This pass is the observability proof. Every global binding read resolves in HIR
//! to one of these forms, whatever codegen path later lowers it (value read, call, spread call,
//! typeof, destructuring, `in`, method read):
//!
//! - `PropertyGet { GlobalGet, name }`: the binding `name` (a global identifier read);
//! - `IndexGet { GlobalGet, String(k) }`: the binding `k`;
//! - `Call { ExternFuncRef("js_global_get_optional" | "js_global_get_or_throw_unresolved"), [String(k)] }`:
//!   the binding `k` (`typeof x` / an unresolved identifier);
//!
//! and these hold %Global% itself as a value, so they can observe ANY binding (computed key,
//! enumeration, `in`, getOwnPropertyDescriptor, escape to unknown code) and arm all rows:
//! `GlobalThisExpr`, a read of `globalThis` / `global` / `self` / `window` / `frames`, an
//! `IndexGet` on `GlobalGet` with a non-literal key, a global-get extern call with a non-literal key.
//!
//! The calls are emitted once per module, in `__perry_prepare_literals_<module>`, which runs before
//! any code of the module can (eager modules: from the entry before every body; deferred modules and
//! workers: from their own init).
use perry_hir::{Expr, Module};

/// The runtime's `GLOBAL_BINDING_ROWS` names, in declaration order: both derived from the
/// %Global% declaration's `observed(X)` members (perry-runtime-manifest realm_global_bindings.rs).
use perry_realm_table::LAZY_GLOBAL_BINDINGS;

/// Names whose read yields %Global% itself.
const GLOBAL_OBJECT_NAMES: &[&str] = &["globalThis", "global", "self", "window", "frames"];

#[derive(Default)]
struct Observed {
    all: bool,
    names: [bool; LAZY_GLOBAL_BINDINGS.len()],
}

impl Observed {
    fn name(&mut self, name: &str) {
        if GLOBAL_OBJECT_NAMES.contains(&name) {
            self.all = true;
        } else if let Some(i) = LAZY_GLOBAL_BINDINGS.iter().position(|n| *n == name) {
            self.names[i] = true;
        }
    }

    fn visit(&mut self, expr: &Expr) {
        match expr {
            Expr::GlobalThisExpr => self.all = true,
            // A bare builtin global function identifier (`parseInt`, `setTimeout`) lowers to an
            // `ExternFuncRef` named after it; codegen reads its value from the global object.
            Expr::ExternFuncRef { name, .. } => self.name(name),
            Expr::PropertyGet {
                object, property, ..
            } if matches!(**object, Expr::GlobalGet(_)) => self.name(property),
            Expr::IndexGet { object, index, .. } if matches!(**object, Expr::GlobalGet(_)) => {
                match &**index {
                    Expr::String(key) => self.name(key),
                    _ => self.all = true,
                }
            }
            Expr::Call { callee, args, .. } => {
                if let Expr::ExternFuncRef { name, .. } = &**callee {
                    if name == "js_global_get_optional"
                        || name == "js_global_get_or_throw_unresolved"
                    {
                        match args.as_slice() {
                            [Expr::String(key)] => self.name(key),
                            _ => self.all = true,
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// The installer symbols module `module` must call, in row order.
pub(crate) fn install_symbols(module: &Module) -> Vec<String> {
    let mut seen = Observed::default();
    perry_hir::tdz_check::for_each_module_expr(module, &mut |e| seen.visit(e));
    for global in &module.globals {
        if let Some(init) = &global.init {
            perry_hir::tdz_check::for_each_expr_in(init, &mut |e| seen.visit(e));
        }
    }
    LAZY_GLOBAL_BINDINGS
        .iter()
        .zip(seen.names)
        .filter(|(_, hit)| seen.all || *hit)
        .map(|(name, _)| format!("js_install_global_{name}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn global_read(name: &str) -> Expr {
        Expr::PropertyGet {
            object: Box::new(Expr::GlobalGet(0)),
            property: name.to_string(),
            byte_offset: 0,
            byte_end: 0,
        }
    }

    fn observed(e: &Expr) -> Vec<&'static str> {
        let mut seen = Observed::default();
        perry_hir::tdz_check::for_each_expr_in(e, &mut |x| seen.visit(x));
        LAZY_GLOBAL_BINDINGS
            .iter()
            .zip(seen.names)
            .filter(|(_, hit)| seen.all || *hit)
            .map(|(n, _)| *n)
            .collect()
    }

    #[test]
    fn a_named_read_arms_only_that_binding() {
        assert_eq!(observed(&global_read("parseInt")), vec!["parseInt"]);
        assert!(observed(&global_read("Math")).is_empty());
    }

    #[test]
    fn the_global_object_as_a_value_arms_every_binding() {
        assert_eq!(
            observed(&global_read("globalThis")).len(),
            LAZY_GLOBAL_BINDINGS.len()
        );
        assert_eq!(
            observed(&Expr::GlobalThisExpr).len(),
            LAZY_GLOBAL_BINDINGS.len()
        );
    }
}
