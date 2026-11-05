//! Forward-captured class declarations.
//!
//! A class declared AFTER a closure that reads it is a lexical binding that
//! closure must see at call time. A function-nested `class C extends Array {
//! … }` (any heritage the compiler cannot resolve to a known class, private
//! elements, computed field keys, or captured enclosing bindings) is lowered to
//! `let C = ClassExprFresh { … }`: the binding holds THIS evaluation's class
//! object, and the static template `ClassRef(C)` is a different object with its
//! own prototype. A closure created EARLIER in the body than the declaration
//! (`var go = () => new C(n)` above `class C`, the UMD/webpack factory shape of
//! fuzzysort's `KeysResult`) used to resolve `C` to the template, so its
//! instances missed every method of the evaluated class and failed `instanceof
//! C`.
//!
//! Whether a class is evaluated is decided while it is lowered, from its
//! heritage, its members and the bindings they capture, so it is not predicted
//! here. Every class declaration that an earlier closure reads is
//! pre-registered as a boxed TDZ local exactly like a forward-captured `let`
//! (`pre_register_forward_captured_lets`), and its declaration initializes the
//! binding (`body_stmt.rs`): with `ClassExprFresh` when the class is evaluated,
//! with the template `ClassRef` (the same value) when it is not. A plain class
//! costs the earlier closure one captured box.
//!
//! Only closures read that local, and expressions that run when a class is
//! defined (computed keys, `extends`, static initializers) like any other code
//! of the function. A class member that names the later class keeps naming it
//! statically while the binding is pending
//! (`with_pending_forward_class_bindings_hidden`), so the scan that finds the
//! earlier closures leaves those member bodies out
//! (`closure_ident_scan::class_binding_reads_stmt`).

use std::collections::HashSet;

use swc_ecma_ast as ast;

use crate::lower::LoweringContext;
use crate::types::LocalId;

/// The declaration site of a pre-registered class binding: take it out of
/// name resolution while the class itself is lowered, so the class body sees
/// exactly what it saw without the forward binding (its own name inside its
/// members is the class's inner binding). Returns the binding id, which the
/// caller then initializes.
pub(crate) fn take_forward_class_binding(
    ctx: &mut LoweringContext,
    class_decl: &ast::ClassDecl,
) -> Option<LocalId> {
    let id = ctx
        .lexical_forward_decls
        .remove(&class_decl.ident.span.lo.0)?;
    ctx.pending_forward_class_bindings.remove(&id);
    let name = class_decl.ident.sym.as_ref();
    if let Some(index) = ctx.lookup_local_index(name) {
        if ctx.locals[index].1 == id {
            ctx.locals.remove(index);
        }
    }
    Some(id)
}

/// What a hidden pending binding is called in `ctx.locals`: it starts with a
/// NUL, so no identifier names it.
const HIDDEN_PENDING_PREFIX: &str = "\0pending forward class ";

fn hidden_name(name: &str) -> String {
    format!("{HIDDEN_PENDING_PREFIX}{name}")
}

/// Is this `ctx.locals` entry a pending class binding that
/// [`with_pending_forward_class_bindings_hidden`] has hidden? It is no name
/// the program can use, and no source text can spell it.
pub(crate) fn is_hidden_pending_binding(name: &str) -> bool {
    name.starts_with(HIDDEN_PENDING_PREFIX)
}

/// Lower a deferred class member (a method, accessor, constructor, static
/// block or field initializer) with the class bindings of the enclosing
/// function that are still pending hidden from name resolution.
///
/// A class snapshots the bindings its members capture when it is evaluated. A
/// binding that is pending is in its dead zone until its own declaration runs,
/// so the snapshot would hold `undefined`, and nothing reaches the snapshot
/// afterwards: not an older evaluation of the class that a loop already handed
/// out, not an instance a per-instance capture array already copied the
/// `undefined` into, not a capture array whose other bindings left scope with
/// their block. A member naming the later class therefore keeps resolving it
/// the way it did before the binding was pre-registered, to the class's static
/// template (`ClassRef`), which is the class itself for the first evaluation.
///
/// What runs when the class is DEFINED is not a member body and is not hidden:
/// computed keys, the `extends` expression, decorators, static field
/// initializers. The class captures nothing for them (it reads no snapshot
/// there), so a closure created in one holds the binding's box and reads it
/// live, like any other closure. Closures and classes in a function nested in
/// the one that owns the binding are unaffected too: they run when that
/// function is called, which is after the declaration initialized the binding.
///
/// Which bindings are hidden: the pending ones the function being lowered
/// owns (`current_fn_scope`; a catch clause is part of its function), and every
/// pending binding of another function that shares a name with one of them,
/// because hiding the inner one would otherwise expose the outer one to the
/// member. Entries are found by id, so two pending bindings with the same name
/// in one function (an inner block's class shadowing an outer one) are both
/// hidden.
///
/// "Hidden" renames the binding in `ctx.locals` to a name no identifier can
/// spell ([`hidden_name`]), in place, so nothing moves and every scope mark
/// stays valid. Code that lists `ctx.locals` by name and spells the names back
/// into source (a direct eval's bindings object) skips such an entry with
/// [`is_hidden_pending_binding`]. The
/// caller invokes this before the member's own `enter_scope`, so the function
/// being lowered is the one that owns the class. The class's own binding was
/// already taken out by `take_forward_class_binding`.
pub(crate) fn with_pending_forward_class_bindings_hidden<T>(
    ctx: &mut LoweringContext,
    lower: impl FnOnce(&mut LoweringContext) -> T,
) -> T {
    if ctx.pending_forward_class_bindings.is_empty() {
        return lower(ctx);
    }
    // Pending bindings that are in scope and not already hidden by an
    // enclosing member (a hidden one no longer answers to its name).
    let in_scope: Vec<(usize, LocalId, u32, String)> = ctx
        .pending_forward_class_bindings
        .iter()
        .filter_map(|(id, (owner, name))| {
            let (pos, _) = ctx
                .locals
                .iter_named(name)
                .find(|(_, entry)| entry.1 == *id)?;
            Some((pos, *id, *owner, name.clone()))
        })
        .collect();
    let owned_names: HashSet<&str> = in_scope
        .iter()
        .filter(|(_, _, owner, _)| *owner == ctx.current_fn_scope)
        .map(|(_, _, _, name)| name.as_str())
        .collect();
    let hidden: Vec<(usize, LocalId, String)> = in_scope
        .iter()
        .filter(|(_, _, _, name)| owned_names.contains(name.as_str()))
        .map(|(pos, id, _, name)| (*pos, *id, name.clone()))
        .collect();
    for (pos, _, name) in &hidden {
        ctx.locals.rename_at(*pos, hidden_name(name));
    }
    let result = lower(ctx);
    for (pos, id, name) in hidden {
        // Nothing the member's lowering does moves a binding below its own
        // marks; the lookup by id is the fallback if one ever did.
        let pos = if ctx.locals.get(pos).is_some_and(|entry| entry.1 == id) {
            Some(pos)
        } else {
            ctx.locals.iter().position(|entry| entry.1 == id)
        };
        if let Some(pos) = pos {
            ctx.locals.rename_at(pos, name);
        }
    }
    result
}
