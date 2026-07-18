//! #6559: runtime dynamic-code evaluation for `new Function(p1, …, body)`.
//!
//! Perry is AOT-compiled, so a `new Function` body constructed from RUNTIME
//! data has no compiled form. Historically the runtime threw a clean
//! TypeError (the honest signal feature-probing libraries like zod rely on).
//! But the schema-codegen ecosystem — **ajv** compiled validators,
//! **fast-json-stringify** serializers, **find-my-way** router matchers, i.e.
//! every fastify-based app — performs mandatory runtime codegen with NO
//! non-codegen fallback: if `new Function` can't evaluate generated source,
//! route registration throws and the server can't boot.
//!
//! This module makes those sites work: the generated source is parsed with
//! the compiler's own parser (perry-parser → SWC; no second parser) and run
//! by a scoped tree-walking interpreter over the SWC AST. The interpreter
//! covers the pragmatic subset those code generators emit (see `interp.rs` /
//! `expr.rs`); anything outside the subset throws a diagnostic TypeError
//! naming the unsupported construct, so real-world gaps surface as clear
//! errors instead of silent miscomputation.
//!
//! Bridging is the crux and it is bidirectional:
//!  * interpreted code calls REAL runtime values (schema refs, format
//!    validators, serializer helpers, `Math`/`JSON`/`String` builtins,
//!    RegExp objects, host classes via `new`) through the same generic
//!    dispatch helpers compiled code uses;
//!  * the callable returned by `new Function` is a first-class runtime
//!    closure (usable as a property value, bound, called with any `this`,
//!    carrying expando properties like ajv's `validate.errors`).
//!
//! GC discipline: interpreter frames hold every live JSValue in a rooted
//! thread-local value stack (`roots`) that a registered mutable root scanner
//! marks AND rewrites on moving collections — the same pattern as
//! `node_vm`'s script tables. Environments are ordinary runtime objects
//! (null-proto, chained through a non-identifier key), so closure captures
//! keep whole scope chains alive through the normal object graph.
//!
//! Exception discipline: throws use the runtime's setjmp/longjmp machinery.
//! Interpreted `try` installs a Rust-side landing pad with the same
//! `crate::ffi::setjmp` idiom the microtask pump uses; a throw that escapes
//! the interpreter entirely unwinds to the caller's compiled `try`. The
//! roots stack is restored via a per-try-depth savepoint recorded by
//! `js_try_push` (see `exception.rs`), mirroring the shadow-stack savepoint.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use perry_parser::swc_ecma_ast as ast;

mod bridge;
mod env;
mod expr;
mod interp;
#[cfg(test)]
mod tests;

/// One parsed interpreted function: the (possibly nested) function AST plus
/// the prepass results the interpreter needs at call time. Registered in the
/// thread-local `FN_REGISTRY`; closures reference it by id (capture slot 0).
pub(crate) struct InterpFn {
    /// Parameter patterns (identifiers, destructuring, defaults).
    pub params: Vec<ast::Pat>,
    /// Function body. `Block` for `function`/`function*`-less bodies,
    /// `Expr` for concise arrow bodies.
    pub body: InterpBody,
    /// `var` names hoisted to the function scope (prepass, excludes nested
    /// function bodies).
    pub hoisted_vars: Vec<String>,
}

pub(crate) enum InterpBody {
    Block(Vec<ast::Stmt>),
    Expr(Box<ast::Expr>),
}

thread_local! {
    /// id → parsed function. Entries live for the program's lifetime (one per
    /// distinct nested function per `new Function` call — bounded by the
    /// number of codegen sites, not by request volume).
    static FN_REGISTRY: RefCell<HashMap<u32, Rc<InterpFn>>> =
        RefCell::new(HashMap::new());
    static NEXT_FN_ID: Cell<u32> = const { Cell::new(1) };

    /// The interpreter's rooted value stack. Every JSValue an interpreter
    /// frame holds across a potential allocation lives here; the GC scanner
    /// below marks and REWRITES the slots, so moving collections can't
    /// invalidate interpreter state. Truncated on frame exit and restored
    /// from the per-try-depth savepoint on caught throws.
    static ROOTS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };

    /// Interpreter call depth (native recursion guard — each interpreted
    /// frame recurses through the Rust tree-walker).
    static CALL_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Cap on interpreter recursion. Each interpreted call consumes native stack
/// via the recursive tree-walker, so the guard must fire well before the OS
/// stack does. ajv's recursive meta-schema validation nests ~10 deep; 256
/// leaves generous headroom while converting runaway recursion into a
/// catchable RangeError instead of a native stack overflow.
const MAX_INTERP_CALL_DEPTH: u32 = 256;

pub(crate) fn register_fn(f: InterpFn) -> u32 {
    let id = NEXT_FN_ID.with(|c| {
        let id = c.get();
        c.set(id + 1);
        id
    });
    FN_REGISTRY.with(|r| r.borrow_mut().insert(id, Rc::new(f)));
    id
}

pub(crate) fn lookup_fn(id: u32) -> Option<Rc<InterpFn>> {
    FN_REGISTRY.with(|r| r.borrow().get(&id).cloned())
}

// ── rooted value stack ─────────────────────────────────────────────────────

/// Push a value onto the rooted stack; returns its index. The index stays
/// valid until the owning frame truncates back past it.
pub(crate) fn root_push(value: f64) -> usize {
    ROOTS.with(|r| {
        let mut v = r.borrow_mut();
        v.push(value.to_bits());
        v.len() - 1
    })
}

/// Re-read a rooted value (the GC scanner may have rewritten the bits).
pub(crate) fn root_get(idx: usize) -> f64 {
    ROOTS.with(|r| f64::from_bits(r.borrow()[idx]))
}

pub(crate) fn root_set(idx: usize, value: f64) {
    ROOTS.with(|r| r.borrow_mut()[idx] = value.to_bits());
}

pub(crate) fn roots_len() -> usize {
    ROOTS.with(|r| r.borrow().len())
}

pub(crate) fn roots_truncate(len: usize) {
    ROOTS.with(|r| {
        let mut v = r.borrow_mut();
        if v.len() > len {
            v.truncate(len);
        }
    });
}

// ── exception-machinery integration ────────────────────────────────────────

/// Savepoint recorded by `js_try_push` for every `try` block (compiled OR
/// interpreted): packs the roots length and the interpreter call depth. A
/// throw `longjmp`s past interpreter Rust frames without running their
/// epilogues, so `js_throw` restores both from the savepoint of the catching
/// `try` — exactly like the shadow-stack savepoint (#1830) and the
/// method-depth savepoint (#5591).
pub(crate) fn interp_savepoint() -> u64 {
    let len = roots_len() as u64;
    let depth = CALL_DEPTH.with(|c| c.get()) as u64;
    (depth << 40) | len
}

pub(crate) fn interp_restore(savepoint: u64) {
    let len = (savepoint & 0xFF_FFFF_FFFF) as usize;
    let depth = (savepoint >> 40) as u32;
    roots_truncate(len);
    CALL_DEPTH.with(|c| c.set(depth));
}

pub(crate) fn call_depth_enter() -> Result<(), ()> {
    CALL_DEPTH.with(|c| {
        let d = c.get();
        if d >= MAX_INTERP_CALL_DEPTH {
            Err(())
        } else {
            c.set(d + 1);
            Ok(())
        }
    })
}

pub(crate) fn call_depth_leave() {
    CALL_DEPTH.with(|c| c.set(c.get().saturating_sub(1)));
}

// ── GC root scanner ────────────────────────────────────────────────────────

/// Mark + rewrite every value on the interpreter's rooted stack. Registered
/// from `gc::init` alongside the other runtime mutable-root scanners.
pub fn scan_dyn_eval_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    ROOTS.with(|r| {
        let mut v = r.borrow_mut();
        for slot in v.iter_mut() {
            visitor.visit_nanbox_u64_slot(slot);
        }
    });
}

// ── entry point ────────────────────────────────────────────────────────────

/// `new Function(p1, …, pN, body)` with a runtime-constructed body.
/// `args` are the already-decoded string arguments (parameter-name lists
/// first, body last — V8 semantics). Returns a first-class runtime closure
/// or throws:
///   * SyntaxError when the assembled source does not parse (matches Node —
///     e.g. the #5206 fixture's `return (not json)`),
///   * TypeError naming the construct when the source parses but uses
///     something outside the interpreter subset.
pub fn dyn_function_from_strings(args: &[String]) -> f64 {
    let (params, body) = match args.split_last() {
        Some((body, params)) => (params.join(","), body.as_str()),
        None => (String::new(), ""),
    };
    // V8's exact assembly shape: the wrapper turns the body into a function
    // expression so top-level `return` (which every ajv/fjs/fmw body uses)
    // parses, and the parameter text is validated by the same parse.
    let source = format!("(function anonymous({params}\n) {{\n{body}\n}})");
    // `.cjs` pins script (sloppy, non-module) parsing: generated bodies rely
    // on sloppy semantics (find-my-way assigns the undeclared `value`), and
    // module auto-detection must not kick in on `import(`-looking substrings.
    let mut cache = perry_diagnostics_cache();
    let parsed =
        match perry_parser::parse_typescript_with_cache(&source, "perry-dyn-fn.cjs", &mut cache) {
            Ok(p) => p,
            Err(e) => bridge::throw_syntax_error(&format!(
                "invalid or unsupported source in runtime `new Function` body: {e}"
            )),
        };
    let func = match extract_wrapper_fn(parsed.module) {
        Some(f) => f,
        None => bridge::throw_syntax_error(
            "runtime `new Function` source did not parse to a single function",
        ),
    };
    // Eager subset scan: reject statically-known-unsupported constructs at
    // construction time (like a SyntaxError would surface in Node), so
    // feature-probing callers take their fallback immediately instead of
    // failing on first invocation.
    interp::scan_function_supported(&func);
    let interp_fn = interp::build_interp_fn(
        func.params.into_iter().map(|p| p.pat).collect(),
        InterpBody::Block(func.body.map(|b| b.stmts).unwrap_or_default()),
    );
    let fn_id = register_fn(interp_fn);
    // The instance's root environment: undeclared-assignment target (sloppy
    // implicit "globals" scoped to this Function instance) and the parent of
    // every call scope.
    let root_env = env::env_new_root();
    let root_idx = root_push(root_env);
    let closure = interp::alloc_interp_closure(fn_id, root_get(root_idx), None);
    roots_truncate(root_idx);
    closure
}

fn perry_diagnostics_cache() -> perry_diagnostics::SourceCache {
    perry_diagnostics::SourceCache::new()
}

/// Unwrap `(function anonymous(…) {…})` from the parsed module.
fn extract_wrapper_fn(module: ast::Module) -> Option<ast::Function> {
    let mut body = module.body;
    if body.len() != 1 {
        return None;
    }
    let stmt = match body.pop()? {
        ast::ModuleItem::Stmt(s) => s,
        ast::ModuleItem::ModuleDecl(_) => return None,
    };
    let expr_stmt = match stmt {
        ast::Stmt::Expr(e) => e,
        _ => return None,
    };
    let mut expr = *expr_stmt.expr;
    loop {
        match expr {
            ast::Expr::Paren(p) => expr = *p.expr,
            ast::Expr::Fn(fn_expr) => return Some(*fn_expr.function),
            _ => return None,
        }
    }
}
