//! Pre-scan for every `class X { … }` DECLARATION name in the module, at any
//! nesting depth.
//!
//! #8882: `lower_new`'s unresolved-constructor guard (#8643) decides at
//! lowering time whether `new X()` can bind at all. Its lookups only see
//! bindings registered so far, but a class declared inside a function body
//! that is lowered LATER is still a legitimate late-bound target — JS
//! resolves the constructor reference when the `new` executes, not when the
//! enclosing method is compiled. The CJS wrap makes this shape common: it
//! hoists most top-level classes out of the module IIFE but leaves some
//! inside (a class it did not recognise textually, or one that reads an
//! IIFE-local), so a hoisted class's constructor can `new` a sibling that is
//! now nested in the `__perry_cjs_factory` closure and registered only when
//! that closure body is lowered. Next's `server/lib/lru-cache.js` has exactly
//! this: `LRUCache` (hoisted) constructs `SentinelNode` (left in the IIFE
//! because its doc comment closes on the `class` line).
//!
//! The guard consults this set: a name declared as a class anywhere in the
//! module keeps the by-name `Expr::New` lowering that codegen resolves through
//! the module class table (the pre-#8643 behaviour); anything else is a
//! runtime `globalThis` lookup that throws the spec `ReferenceError: X is not
//! defined`. Only DECLARATIONS count — a named class EXPRESSION's name binds
//! inside its own body alone.

use swc_ecma_ast as ast;
use swc_ecma_visit::{Visit, VisitWith};

use crate::lower::*;

pub(crate) fn pre_scan_class_decl_names(ast_module: &ast::Module, ctx: &mut LoweringContext) {
    struct Collector<'a> {
        names: &'a mut std::collections::HashSet<String>,
    }
    impl Visit for Collector<'_> {
        fn visit_class_decl(&mut self, class_decl: &ast::ClassDecl) {
            self.names.insert(class_decl.ident.sym.to_string());
            class_decl.visit_children_with(self);
        }
    }
    let mut collector = Collector {
        names: &mut ctx.class_decl_names_any_depth,
    };
    ast_module.visit_with(&mut collector);
}
