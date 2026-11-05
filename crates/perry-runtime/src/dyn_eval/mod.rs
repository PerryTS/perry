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
//! errors instead of silent miscomputation. #10661: `generate-function`
//! (mysql2's row parsers, and others beyond mysql2) emits a **class
//! expression** as the returned value — `interp::eval_class_expr` supports a
//! deliberately narrow subset of that (constructor + plain methods, no
//! `extends`/decorators/getters/setters/fields/private members/computed
//! keys); see its doc comment for the exact boundary.
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
//! Exception discipline: throws use the runtime's landing-pad transport.
//! Interpreted `try` runs its protected block under a runtime trap
//! (`crate::exception::trap`); a throw that escapes the interpreter entirely
//! unwinds to the caller's handler. The interpreter keeps its own state
//! balanced across that (design §1.1, "dyn_eval"): every entry into
//! interpreted code ([`interp_entry`]) and every interpreted `try` records
//! the rooted-stack length and call depth, and restores them when a throw
//! lands, before it rethrows or handles it. Nothing is recorded by a
//! compiled `try`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};

use perry_parser::swc_ecma_ast as ast;

#[cfg(test)]
mod bench;
mod bridge;
mod env;
mod expr;
pub(crate) mod function_cell;
mod function_owner;
mod interp;
#[cfg(test)]
mod tests;

/// One parsed interpreted function: the (possibly nested) function AST plus
/// the prepass results the interpreter needs at call time. A collecting native
/// cell in closure capture zero owns it.
pub(crate) struct InterpFn {
    /// Parameter patterns (identifiers, destructuring, defaults).
    pub params: Vec<ast::Pat>,
    /// Function body. `Block` for `function`/`function*`-less bodies,
    /// `Expr` for concise arrow bodies.
    pub body: InterpBody,
    /// `var` names hoisted to the function scope (prepass, excludes nested
    /// function bodies).
    pub hoisted_vars: Vec<String>,
    /// Whether assignments in this function use strict PutValue semantics.
    pub strict: bool,
    pub node_functions: RefCell<HashMap<usize, Rc<InterpFn>>>,
}

pub(crate) enum InterpBody {
    Block(Vec<ast::Stmt>),
    Expr(Box<ast::Expr>),
}

thread_local! {
    /// The interpreter's rooted value stack. Every JSValue an interpreter
    /// frame holds across a potential allocation lives here; the GC scanner
    /// below marks and REWRITES the slots, so moving collections can't
    /// invalidate interpreter state. Truncated on frame exit; a throw is
    /// restored by the interpreter entry or interpreted `try` it lands in
    /// ([`interp_entry`]).
    static ROOTS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };

    /// Interpreter call depth (native recursion guard — each interpreted
    /// frame recurses through the Rust tree-walker).
    static CALL_DEPTH: Cell<u32> = const { Cell::new(0) };

    /// Assembled-source → Rust-owned prepared AST (#6693). `new Function` with a
    /// body identical to one already prepared skips the SWC re-parse + subset
    /// scan + hoist prepass entirely — the dominant construction cost — and
    /// reuses the prepared `InterpFn`. Each `new Function` still returns a
    /// FRESH closure over a fresh per-instance root environment, so identity /
    /// expando semantics are unchanged; only the parse work is shared. Fastify
    /// stacks (ajv / fast-json-stringify / find-my-way) and repeated schema
    /// compiles re-`new Function` identical bodies; distinct bodies simply
    /// miss (no slower than before). Bounded so a pathological distinct-source
    /// stream cannot grow it without limit.
    static SOURCE_FN_CACHE: RefCell<HashMap<String, Rc<InterpFn>>> = RefCell::new(HashMap::new());

    /// Aggregate byte size of the source strings currently held in
    /// `SOURCE_FN_CACHE`. The entry-count cap alone doesn't bound memory —
    /// `new Function` bodies are script-controlled and can be large (real
    /// TypeBox validators reach ~58 KB), so 4096 large distinct bodies would
    /// retain hundreds of MB. This tracks the total so we can cap by size too.
    static SOURCE_FN_CACHE_BYTES: Cell<usize> = const { Cell::new(0) };
}

/// Upper bound on distinct cached sources. Codegen sites are few; this only
/// guards against an adversarial stream of unique bodies. On overflow new
/// distinct sources still work — they just aren't memoized.
const SOURCE_FN_CACHE_MAX: usize = 4096;

/// Aggregate byte cap on cached source strings (defense-in-depth alongside the
/// entry-count cap). Past this, new distinct sources still run — just uncached.
const SOURCE_FN_CACHE_MAX_BYTES: usize = 32 * 1024 * 1024;

// ── #6693 runtime A/B toggles ───────────────────────────────────────────────
// Read once per process, then a relaxed atomic load on the hot path. 0 =
// unresolved, 1 = on, 2 = off. Let the SAME compiled binary A/B each win on
// the real bundle without recompiling: `PERRY_DYN_NO_PARSE_CACHE=1` reverts to
// re-parse-every-call (the pre-#6693 parse behavior), and `PERRY_DYN_FAST_SCOPE=1`
// enables the lean plain-scope env accessor (the prototype surgical fix).
static PARSE_CACHE_OFF: AtomicU8 = AtomicU8::new(0);
static FAST_SCOPE_ON: AtomicU8 = AtomicU8::new(0);

fn env_toggle(slot: &AtomicU8, var: &str) -> bool {
    match slot.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let on = std::env::var_os(var)
                .map(|v| v != "0" && !v.is_empty())
                .unwrap_or(false);
            slot.store(if on { 1 } else { 2 }, Ordering::Relaxed);
            on
        }
    }
}

/// Whether the source→`InterpFn` parse cache is active (default on; disabled by
/// `PERRY_DYN_NO_PARSE_CACHE=1` to A/B its effect on the real grind).
fn parse_cache_enabled() -> bool {
    !env_toggle(&PARSE_CACHE_OFF, "PERRY_DYN_NO_PARSE_CACHE")
}

/// Whether the lean plain-scope env accessor is active (default off; enabled by
/// `PERRY_DYN_FAST_SCOPE=1`). #6693 prototype.
pub(crate) fn fast_scope_enabled() -> bool {
    env_toggle(&FAST_SCOPE_ON, "PERRY_DYN_FAST_SCOPE")
}

/// Cap on interpreter recursion. Each interpreted call consumes native stack
/// via the recursive tree-walker, so the guard must fire well before the OS
/// stack does. ajv's recursive meta-schema validation nests ~10 deep; 256
/// leaves generous headroom while converting runaway recursion into a
/// catchable RangeError instead of a native stack overflow.
const MAX_INTERP_CALL_DEPTH: u32 = 256;

pub(crate) fn register_fn(f: InterpFn) -> f64 {
    function_cell::new(Rc::new(f))
}
pub(crate) fn lookup_fn(id: f64) -> Option<Rc<InterpFn>> {
    function_cell::get(id)
}

// ── GC safepoints ──────────────────────────────────────────────────────────

/// Offer a GC safepoint at an interpreter step, behind
/// `PERRY_GC_INTERP_SAFEPOINTS=1` (#7803 tooling).
///
/// # The structural gap this closes
///
/// Compiled code offers the collector cooperative safepoints at loop
/// back-edges (`PERRY_GC_MOVING_LOOP_POLLS`, default on since #7721). The
/// interpreter offers NONE. A collection can therefore only reach interpreted
/// execution at an *allocation* point — and the alloc-point arm forces a
/// conservative stack scan, which finds Rust locals and makes the copying
/// minor ineligible. The consequence is not that the interpreter is safe; it
/// is that the interpreter is **untestable**:
///
///  * `PERRY_GC_SCHEDULE_RATE` forces collection at safepoints, and there are
///    none here;
///  * `PERRY_GC_SCHEDULE_SEED` selects safepoints, and there are none here;
///  * `gc_root_dominance_check.py` reads emitted LLVM IR, and there is none
///    here.
///
/// So the one rooting domain with no static checker also has no dynamic one.
/// That is the finding #7803 turned up, independent of what its own root cause
/// turns out to be: `dyn_eval/mod.rs` claims "interpreter frames hold every
/// live JSValue in a rooted thread-local value stack", and nothing in the tree
/// can currently falsify that sentence.
///
/// This gives the existing instruments a handle. It routes through
/// `js_gc_loop_safepoint`, deliberately, rather than collecting directly:
/// every entry guard (in-alloc, root-lock, unsafe-FFI-zone, budgeted-cycle)
/// and the seeded-schedule ordinal apply exactly as they do to a compiled
/// back-edge, so an interpreter safepoint is the *same* safepoint, not a
/// second kind.
///
/// # Why it is opt-in rather than on
///
/// Turning it on lets the precise moving collector run at points where
/// interpreted frames are live. If the interpreter's rooting is complete that
/// is simply better — the copying minor becomes eligible where only a
/// conservative sweep could run before. If it is NOT complete, this converts a
/// latent hole into a live crash for exactly the workloads `dyn_eval` exists
/// to serve (ajv, fast-json-stringify, find-my-way, fastify). Shipping that
/// flip before the rooting is verified would be trading a quiet bug for a loud
/// one in someone else's server.
///
/// So it lands as an instrument, and the flip to default-on is a separate,
/// evidence-gated decision — the same sequencing `PERRY_GC_MOVING_LOOP_POLLS`
/// had between #7161 and #7721.
///
/// Parsed BY VALUE (`1`/`on`/`true`), never by presence — see #7993.
#[inline]
pub(crate) fn interp_safepoint() {
    if !interp_safepoints_enabled() {
        return;
    }
    crate::gc::js_gc_loop_safepoint();
}

fn interp_safepoints_enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *crate::once_init::get_or_init(&ENABLED, || {
        matches!(
            std::env::var("PERRY_GC_INTERP_SAFEPOINTS").ok().as_deref(),
            Some("1") | Some("on") | Some("true")
        )
    })
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

#[inline]
pub(crate) fn roots_len() -> usize {
    ROOTS.with(|r| r.borrow().len())
}

pub(crate) fn roots_truncate(len: usize) {
    function_owner::release_function_pins(len);
    ROOTS.with(|r| {
        let mut v = r.borrow_mut();
        if v.len() > len {
            v.truncate(len);
        }
    });
}

// ── exception-machinery integration ────────────────────────────────────────

/// The interpreter's state at an entry or an interpreted `try`: the roots
/// length and the call depth, packed. A throw skips the epilogues of the
/// interpreter Rust frames it abandons, so the entry (or `try`) that catches
/// it puts both back.
#[inline]
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

/// Run interpreted code entered from outside the interpreter (an
/// interpreted function call, a script, a direct eval). A throw that leaves
/// `f` restores the interpreter's rooted stack and call depth to the entry
/// state and is rethrown to the caller's handler.
pub(crate) fn interp_entry<R>(f: impl FnOnce() -> R) -> R {
    let saved = interp_savepoint();
    crate::dyn_eval_hooks::interp_active_enter();
    let r = crate::exception::catch_js_throw(f);
    crate::dyn_eval_hooks::interp_active_leave();
    match r {
        Ok(r) => r,
        Err(err) => {
            // The exception value is a plain f64 here; nothing below
            // allocates before `js_throw` roots it again.
            interp_restore(saved);
            crate::exception::js_throw(err)
        }
    }
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
    env::scan_env_key_cache_mut(visitor);
    bridge::scan_member_key_cache_mut(visitor);
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
    // Interpreted code reaches `process`/`console` by name.
    crate::object::js_install_global_value_surfaces();
    let fn_id = prepare_function_args(args);
    // Keep the prepared AST alive across environment/closure allocations.
    let prepared = function_owner::pin_function(fn_id).expect("just prepared function");
    let function_length = prepared
        .params
        .iter()
        .take_while(|pat| !matches!(pat, ast::Pat::Assign(_) | ast::Pat::Rest(_)))
        .count();
    // Preserve Function-constructor semantics: each instance owns a private
    // sloppy-assignment root, while universal globals resolve in this realm.
    let base = roots_len();
    // Root the global BEFORE env_new_root() allocates. A copying minor
    // triggered by that allocation evacuates the global singleton —
    // THREAD_GLOBAL_THIS (a registered root) is rewritten, but a raw local
    // is not. The stale pointer then flows into the closure's capture
    // slots 3 (global) and 4 (intrinsics), so every call reads a stale
    // global. On the `Function("return this")()` path the sloppy-mode
    // `this = global` returns undefined (the evacuated from-space object's
    // fields read back as undefined), and downstream
    // `Object.getPrototypeOf(undefined)` throws TypeError. The other
    // dyn_eval entry points already root global_this before any
    // allocation; this one missed it.
    let global = crate::object::js_get_global_this();
    let global_idx = root_push(global);
    let root_env = env::env_new_root();
    let root_idx = root_push(root_env);
    let closure = interp::alloc_interp_closure(
        fn_id,
        root_get(root_idx),
        None,
        root_get(global_idx),
        root_get(global_idx),
        true,
        true,
    );
    let closure_idx = root_push(closure);
    let closure_ptr = crate::value::js_nanbox_get_pointer(root_get(closure_idx))
        as *mut crate::closure::ClosureHeader;
    if !closure_ptr.is_null() {
        crate::object::set_bound_native_closure_name(closure_ptr, "anonymous");
        let closure_ptr = crate::value::js_nanbox_get_pointer(root_get(closure_idx))
            as *mut crate::closure::ClosureHeader;
        crate::object::set_builtin_closure_length(closure_ptr as usize, function_length as u32);
    }
    let closure = root_get(closure_idx);
    roots_truncate(base);
    closure
}

/// Build a persistent script lexical environment over a live global/object
/// environment chain. The last `object_envs` entry has highest precedence.
pub(crate) fn script_environment(global: f64, object_envs: &[f64]) -> f64 {
    // Interpreted code reaches `process`/`console` by name.
    crate::object::js_install_global_value_surfaces();
    let chain = object_environment_chain(global, object_envs);
    let chain_idx = root_push(chain);
    let lexical = env::env_new(root_get(chain_idx));
    roots_truncate(chain_idx);
    lexical
}

pub(crate) fn script_binding(lexical_env: f64, name: &str) -> f64 {
    env::lookup(lexical_env, name).unwrap_or_else(bridge::undefined)
}

/// Compile a Function-constructor body against a selected global and live
/// context-extension objects. Parameter/local bindings still win; then
/// object_envs are searched from last to first; the global is last.
#[cfg(test)]
pub(crate) fn function_from_strings_in(
    args: &[String],
    global_this: f64,
    intrinsics: f64,
    object_envs: &[f64],
) -> f64 {
    // Interpreted code reaches `process`/`console` by name.
    crate::object::js_install_global_value_surfaces();
    function_from_strings_in_with_codegen(args, global_this, intrinsics, object_envs, true, true)
}

pub(crate) fn function_from_strings_in_with_codegen(
    args: &[String],
    global_this: f64,
    intrinsics: f64,
    object_envs: &[f64],
    strings_allowed: bool,
    wasm_allowed: bool,
) -> f64 {
    // Interpreted code reaches `process`/`console` by name.
    crate::object::js_install_global_value_surfaces();
    let base = roots_len();
    let global_idx = root_push(global_this);
    let intrinsics_idx = root_push(intrinsics);
    let object_env_idxs = object_envs
        .iter()
        .copied()
        .map(root_push)
        .collect::<Vec<_>>();
    let fn_id = prepare_function_args(args);
    let _prepared = function_owner::pin_function(fn_id).expect("just prepared function");
    let rooted_object_envs = object_env_idxs
        .iter()
        .map(|&idx| root_get(idx))
        .collect::<Vec<_>>();
    let chain = object_environment_chain(root_get(global_idx), &rooted_object_envs);
    let chain_idx = root_push(chain);
    let closure = interp::alloc_interp_closure(
        fn_id,
        root_get(chain_idx),
        None,
        root_get(global_idx),
        root_get(intrinsics_idx),
        strings_allowed,
        wasm_allowed,
    );
    roots_truncate(base);
    closure
}

/// Parse and execute script/global code with a selected global and persistent
/// lexical environment. Syntax errors use the shared SWC parser diagnostic;
/// unsupported runtime constructs keep dyn_eval's precise TypeError path.
pub(crate) fn eval_script_in(
    source: &str,
    global_this: f64,
    intrinsics: f64,
    lexical_env: f64,
) -> f64 {
    // Interpreted code reaches `process`/`console` by name.
    crate::object::js_install_global_value_surfaces();
    eval_script_in_with_codegen(source, global_this, intrinsics, lexical_env, true, true)
}

pub(crate) fn eval_script_in_with_codegen(
    source: &str,
    global_this: f64,
    intrinsics: f64,
    lexical_env: f64,
    strings_allowed: bool,
    wasm_allowed: bool,
) -> f64 {
    // Interpreted code reaches `process`/`console` by name.
    crate::object::js_install_global_value_surfaces();
    interp_entry(|| {
        eval_script_in_entered(
            source,
            global_this,
            intrinsics,
            lexical_env,
            strings_allowed,
            wasm_allowed,
        )
    })
}

fn eval_script_in_entered(
    source: &str,
    global_this: f64,
    intrinsics: f64,
    lexical_env: f64,
    strings_allowed: bool,
    wasm_allowed: bool,
) -> f64 {
    let base = roots_len();
    let global_idx = root_push(global_this);
    let intrinsics_idx = root_push(intrinsics);
    let env_idx = root_push(lexical_env);
    let (script_id, script) = register_script(parse_script_statements(source));
    let statements = script_statements(&script);
    let variable_env_idx = root_push(env::variable_environment(root_get(env_idx)));
    let ret_idx = root_push(bridge::undefined());
    let ctx = interp::Ctx {
        this_idx: global_idx,
        ret_idx,
        global_idx,
        intrinsics_idx,
        variable_env_idx,
        strict: script.strict,
        strings_allowed,
        wasm_allowed,
        owner_fn: script_id,
    };
    let _ = interp::exec_script_stmts(&ctx, statements, env_idx);
    let result = root_get(ret_idx);
    roots_truncate(base);
    result
}

pub(crate) fn eval_direct_in(
    source: &str,
    global_this: f64,
    intrinsics: f64,
    caller_env: f64,
    caller_variable_env: f64,
    strings_allowed: bool,
    wasm_allowed: bool,
) -> f64 {
    eval_direct_in_context(
        source,
        global_this,
        intrinsics,
        caller_env,
        caller_variable_env,
        strings_allowed,
        wasm_allowed,
        false,
        global_this,
    )
}

fn eval_direct_in_context(
    source: &str,
    global_this: f64,
    intrinsics: f64,
    caller_env: f64,
    caller_variable_env: f64,
    strings_allowed: bool,
    wasm_allowed: bool,
    inherited_strict: bool,
    this_value: f64,
) -> f64 {
    // Interpreted code reaches `process`/`console` by name.
    crate::object::js_install_global_value_surfaces();
    interp_entry(|| {
        eval_direct_in_entered(
            source,
            global_this,
            intrinsics,
            caller_env,
            caller_variable_env,
            strings_allowed,
            wasm_allowed,
            inherited_strict,
            this_value,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn eval_direct_in_entered(
    source: &str,
    global_this: f64,
    intrinsics: f64,
    caller_env: f64,
    caller_variable_env: f64,
    strings_allowed: bool,
    wasm_allowed: bool,
    inherited_strict: bool,
    this_value: f64,
) -> f64 {
    let base = roots_len();
    let this_idx = root_push(this_value);
    let global_idx = root_push(global_this);
    let intrinsics_idx = root_push(intrinsics);
    let caller_env_idx = root_push(caller_env);
    let caller_variable_env_idx = root_push(caller_variable_env);
    let strict_source;
    let source = if inherited_strict {
        strict_source = format!("\"use strict\";\n{source}");
        strict_source.as_str()
    } else {
        source
    };
    let (script_id, script) = register_script(parse_script_statements(source));
    let statements = script_statements(&script);
    let strict = inherited_strict || script.strict;
    let lexical_env_idx = root_push(env::env_new(root_get(caller_env_idx)));
    let ret_idx = root_push(bridge::undefined());
    let variable_env_idx = if strict {
        lexical_env_idx
    } else {
        caller_variable_env_idx
    };
    let ctx = interp::Ctx {
        this_idx,
        ret_idx,
        global_idx,
        intrinsics_idx,
        variable_env_idx,
        strict,
        strings_allowed,
        wasm_allowed,
        owner_fn: script_id,
    };
    let _ = interp::exec_direct_eval_stmts(&ctx, statements, lexical_env_idx, variable_env_idx);
    let result = root_get(ret_idx);
    roots_truncate(base);
    result
}

/// Register a parsed script/eval body as a pinned, parameterless `InterpFn`.
/// Nested functions are cached by AST-node address, so the statements must
/// stay allocated for as long as those cache entries exist: owning them in
/// its own node memo dies with the AST. The cell pin is released when the
/// script's roots are truncated, including by
/// a throw). A local `Vec` freed on return would let the next script's parse
/// reuse an address and resolve to this script's function.
fn register_script(statements: Vec<ast::Stmt>) -> (f64, function_owner::FunctionPin) {
    let strict = interp::has_use_strict_directive(&statements);
    let id = register_fn(InterpFn {
        params: Vec::new(),
        body: InterpBody::Block(statements),
        hoisted_vars: Vec::new(),
        strict,
        node_functions: RefCell::new(HashMap::new()),
    });
    let pin = function_owner::pin_function(id).expect("just registered script");
    (id, pin)
}

fn script_statements(script: &InterpFn) -> &[ast::Stmt] {
    match &script.body {
        InterpBody::Block(statements) => statements,
        InterpBody::Expr(_) => unreachable!("scripts register a block body"),
    }
}

fn parse_script_statements(source: &str) -> Vec<ast::Stmt> {
    let mut cache = perry_diagnostics_cache();
    let parsed =
        perry_parser::parse_typescript_with_cache(source, "perry-vm-script.cjs", &mut cache)
            .unwrap_or_else(|e| {
                bridge::throw_syntax_error(&format!("invalid node:vm script source: {e}"))
            });
    parsed
        .module
        .body
        .into_iter()
        .map(|item| match item {
            ast::ModuleItem::Stmt(stmt) => stmt,
            ast::ModuleItem::ModuleDecl(_) => {
                bridge::throw_syntax_error("module syntax is not valid in a vm.Script")
            }
        })
        .collect()
}

pub(crate) fn validate_script_source(source: &str) -> f64 {
    let _ = parse_script_statements(source);
    bridge::undefined()
}

fn object_environment_chain(global: f64, object_envs: &[f64]) -> f64 {
    let base = roots_len();
    let global_idx = root_push(global);
    let object_idxs = object_envs
        .iter()
        .copied()
        .map(root_push)
        .collect::<Vec<_>>();
    let root = env::env_new_object(None, root_get(global_idx));
    let env_idx = root_push(root);
    for &object_idx in &object_idxs {
        let next = env::env_new_object(Some(root_get(env_idx)), root_get(object_idx));
        root_set(env_idx, next);
    }
    let result = root_get(env_idx);
    roots_truncate(base);
    result
}

fn prepare_function_args(args: &[String]) -> f64 {
    let (params, body) = match args.split_last() {
        Some((body, params)) => (params.join(","), body.as_str()),
        None => (String::new(), ""),
    };
    // V8's exact assembly shape: the wrapper turns the body into a function
    // expression so top-level `return` (which every ajv/fjs/fmw body uses)
    // parses, and the parameter text is validated by the same parse.
    let source = format!("(function anonymous({params}\n) {{\n{body}\n}})");
    // Parse cache: an identical assembled source reuses the already-prepared
    // `InterpFn` (same native AST, same stable AST-node addresses that
    // the nested-function cache keys on) — skipping SWC parse + subset scan +
    // hoist prepass. A cache hit still builds a fresh root env + closure below.
    let fn_id = if !parse_cache_enabled() {
        prepare_source(&source)
    } else {
        match SOURCE_FN_CACHE.with(|c| c.borrow().get(&source).cloned()) {
            Some(function) => function_cell::new(function),
            None => {
                let id = prepare_source(&source);
                SOURCE_FN_CACHE.with(|c| {
                    let mut c = c.borrow_mut();
                    let bytes = SOURCE_FN_CACHE_BYTES.with(|b| b.get());
                    if c.len() < SOURCE_FN_CACHE_MAX
                        && bytes + source.len() <= SOURCE_FN_CACHE_MAX_BYTES
                    {
                        SOURCE_FN_CACHE_BYTES.with(|b| b.set(bytes + source.len()));
                        c.insert(source, lookup_fn(id).expect("just prepared AST"));
                    }
                });
                id
            }
        }
    };
    fn_id
}

/// Parse an assembled `(function anonymous(…){…})` source, reject
/// out-of-subset constructs eagerly, run the hoist prepass, and register the
/// resulting `InterpFn`. Returns its collecting cell. Throws SyntaxError on a
/// parse failure and TypeError on an unsupported construct — the same
/// diagnostics as before the parse cache existed; only a cache MISS runs this.
fn prepare_source(source: &str) -> f64 {
    // `.cjs` pins script (sloppy, non-module) parsing: generated bodies rely
    // on sloppy semantics (find-my-way assigns the undeclared `value`), and
    // module auto-detection must not kick in on `import(`-looking substrings.
    let mut cache = perry_diagnostics_cache();
    let parsed =
        match perry_parser::parse_typescript_with_cache(source, "perry-dyn-fn.cjs", &mut cache) {
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
        perry_parser::into_function_parameter_patterns(func.this_param, func.params).collect(),
        InterpBody::Block(func.body.map(|b| b.stmts).unwrap_or_default()),
        false,
    );
    register_fn(interp_fn)
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

/// Direct eval from compiled code. The binding objects hold native
/// getter/setter closures, so reads, writes and retained eval closures share
/// live cells. `scope` is the calling function's eval scope object when it has
/// a dynamic scope (`perry_hir::lower::dynamic_eval`): the eval's `var`s and
/// functions are declared there, between the function's own bindings and the
/// enclosing ones. Otherwise it is undefined and `outer` is unused.
#[no_mangle]
pub extern "C" fn js_eval_in_scope(
    source: f64,
    own: f64,
    outer: f64,
    scope: f64,
    this_value: f64,
    strict: f64,
) -> f64 {
    let value = crate::value::JSValue::from_bits(source.to_bits());
    if !value.is_string() {
        return source;
    }
    let roots = crate::gc::RuntimeHandleScope::new();
    let source = roots.root_nanbox_f64(source);
    let own = roots.root_nanbox_f64(own);
    let outer = roots.root_nanbox_f64(outer);
    let scope = roots.root_nanbox_f64(scope);
    let this_value = roots.root_nanbox_f64(this_value);
    let text = bridge::read_string(source.get_nanbox_f64()).expect("string source");
    let global = roots.root_nanbox_f64(crate::object::js_get_global_this());
    let has_scope =
        !crate::value::JSValue::from_bits(scope.get_nanbox_f64().to_bits()).is_undefined();
    let (env, variable_env) = if has_scope {
        let env = roots.root_nanbox_f64(script_environment(
            global.get_nanbox_f64(),
            &[
                outer.get_nanbox_f64(),
                scope.get_nanbox_f64(),
                own.get_nanbox_f64(),
            ],
        ));
        // lexical → own bindings → the scope object's environment.
        let own_env = env::env_parent(env.get_nanbox_f64()).expect("own bindings env");
        let scope_env = env::env_parent(own_env).expect("eval scope env");
        let scope_env = roots.root_nanbox_f64(scope_env);
        (env, scope_env)
    } else {
        let env = roots.root_nanbox_f64(script_environment(
            global.get_nanbox_f64(),
            &[own.get_nanbox_f64()],
        ));
        let variable_env = roots.root_nanbox_f64(env::variable_environment(env.get_nanbox_f64()));
        (env, variable_env)
    };
    eval_direct_in_context(
        &text,
        global.get_nanbox_f64(),
        global.get_nanbox_f64(),
        env.get_nanbox_f64(),
        variable_env.get_nanbox_f64(),
        true,
        true,
        crate::value::js_is_truthy(strict) != 0,
        this_value.get_nanbox_f64(),
    )
}

pub(crate) fn install_function_owner_storage() {
    function_owner::install_storage_resolvers();
}
