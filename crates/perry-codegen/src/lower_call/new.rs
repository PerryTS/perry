//! `new ClassName(args…)` lowering.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. Holds `lower_new` (Phase C.1 constructor inlining).
//! The `FieldInitMode` enum and `apply_field_initializers_recursive` live
//! in the sibling `field_init` module.

use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::Expr;

use super::field_init::{apply_field_initializers_recursive, FieldInitMode};
use super::lower_builtin_new;
use super::new_ctor_args::{
    bind_inline_constructor_params, call_local_constructor_symbol, lower_constructor_arg,
    marshal_imported_ctor_args, restore_inline_constructor_scope, CaptureFill,
};
use super::new_helpers::{
    collect_decl_local_ids, ctor_body_calls_super, ctor_body_closure_calls_super,
    ctor_body_has_value_return, ctor_body_uses_this, ctor_chain_uses_new_target,
    default_ctor_dynamic_parent_owner, emit_promise_subclass_init, local_constructor_symbol_exists,
    node_stream_parent_kind,
};
use crate::expr::{lower_expr, lower_js_args_array, nanbox_pointer_inline, FnCtx};
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::rooting::{self, open_rooted_group, EmittedValue, Repr, RootedGroup};
use crate::types::{DOUBLE, I16, I32, I64, I8, PTR};

/// Does `new <class_name>(…)` run user code — an own or inherited constructor
/// body, or field initializers — between the instance allocation and the value
/// the `new` expression yields?
///
/// That is the window #7154 is about: user code allocates, a back-edge poll
/// inside it drives an evacuating minor, and the instance moves while the
/// caller holds it only in an SSA register. A class with none of these has no
/// window at all (`js_gc_init_typed_shape_layout` is the only thing emitted in
/// between, and it does not allocate), so it keeps its pre-#7154 IR exactly.
///
/// An unresolvable class name is `false` on purpose, not conservatively `true`:
/// the instance root is pushed only on paths that resolved the class out of
/// `ctx.classes`, so a name this returns `false` for never reaches the push and
/// would leave the scope marker as pure overhead.
fn construction_runs_user_code(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    // #7207: an IMPORTED constructor runs user code while leaving no trace in
    // the local class table — `ctx.classes[class_name].constructor` is `None`
    // for it. A class that also declares no fields and no heritage therefore
    // answered `false` here while `lower_new_impl_inner` went on to dispatch
    // `ctx.imported_class_ctors[class_name]` (its `has_imported_ctor` arm, and
    // the `Stmt::Return` writer at the tail of this file). That left BOTH
    // consumers of this predicate unprotected across a real constructor body:
    // #7192's instance root, and the `this`-slot bind added for #7202.
    //
    // Keeping it ONE predicate rather than two is the point — the consumers
    // have to agree by construction, which is what stops the divergence
    // #7114's pair of predicates produced.
    if ctx.imported_class_ctors.contains_key(class_name) {
        return true;
    }
    ctx.classes.get(class_name).is_some_and(|class| {
        class.constructor.is_some()
            || !class.fields.is_empty()
            // #8809: a class whose only private elements are METHODS or
            // ACCESSORS declares no fields, no constructor and no heritage, and
            // answered `false` here — while `emit_field_inits` still emits
            // `js_private_brand_add` for it (#8643 added that call, keyed on
            // `has_private_instance_elements`, and its `continue` guard lets a
            // fieldless class through precisely so the brand can be installed).
            // That helper allocates the marker key and calls
            // `js_object_set_field_by_name`; its own body says "the marker-key
            // allocation can evacuate both the receiver and any live value" and
            // opens a `RuntimeHandleScope` for exactly that reason. So the
            // window this predicate claims cannot collect does, and the
            // instance was crossing it in a bare register: `new
            // WithPrivateMethod()` fed a stale handle to
            // `js_gc_init_typed_shape_layout` and then published it into the
            // caller's root slot.
            //
            // One predicate, one place — the temp root, the `this`-slot bind
            // and `reload_instance` all read this, which is what stops them
            // disagreeing the way #7114's pair did.
            || class.has_private_instance_elements()
            || class.extends.is_some()
            || class.extends_name.is_some()
            || class.native_extends.is_some()
            || class.extends_expr.is_some()
    })
}

/// Re-read the freshly-constructed instance from the temp-root slot that
/// carried it across the constructor body (#7154).
///
/// Returns `(obj_handle, obj_box)` — the bare handle and its NaN-boxed form.
/// When no root was pushed (nothing between the allocation and here can
/// collect) the original registers are handed straight back, so those sites
/// keep their old IR byte for byte.
fn reload_instance(
    ctx: &mut FnCtx<'_>,
    group: &RootedGroup<'_>,
    instance: &Instance,
    obj_handle: &str,
    obj_box: &str,
) -> (String, String) {
    if !instance.protected {
        return (obj_handle.to_string(), obj_box.to_string());
    }
    let handle = group.reread_emitted(ctx, instance.root);
    let boxed = nanbox_pointer_inline(ctx.block(), &handle);
    (handle, boxed)
}

/// The freshly-allocated instance's place in the `new` scope.
///
/// `protected` is `construction_runs_user_code`, taken ONCE. It gates three
/// things that must agree — the temp root, the `this`-slot bind (#7202), and
/// whether [`reload_instance`] re-reads at all — and the reason it is a field
/// rather than three calls is the same reason `construction_runs_user_code` is
/// one predicate rather than two: a fork here is how #7114's pair diverged.
struct Instance {
    root: EmittedValue,
    protected: bool,
}

pub(crate) use super::capture_writeback::emit_class_capture_writeback;
use super::typed_shape_init::{emit_typed_shape_layout_declare, emit_typed_shape_layout_init};

/// Lower `new ClassName(args…)` — Phase C.1.
///
/// Strategy: allocate an anonymous object via `js_object_alloc(0, N)`
/// where N is the field count, NaN-box the pointer, then inline the
/// constructor body with:
/// - a fresh local-id-keyed alloca slot for each constructor parameter
///   (pre-populated with the lowered argument value)
/// - a `this_stack` entry pointing at a slot holding the new object
///
/// `Expr::This` then loads from the top of `this_stack`. `this.x = v`
/// goes through the existing `Expr::PropertySet` path which targets
/// `js_object_set_field_by_name`.
///
/// Limitations of this first slice:
/// - No inheritance (parent classes ignored)
/// - No method calls on instances (just field reads/writes via the
///   existing PropertyGet/PropertySet paths)
/// - Constructor cannot use `return <expr>` (would terminate the
///   enclosing function, not the constructor body)
/// - No method dispatch or vtables — those land in Phase C.2/C.3
pub(crate) fn lower_new(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &[Expr],
    cap_args_appended: u32,
) -> Result<String> {
    // #6538: the HIR bare-identifier / anonymous-class `Expr::New` arms append
    // the class's captures as trailing `LocalGet` args ONLY where the captured
    // locals are in scope (the declaring function), recording the count in
    // `Expr::New::cap_args_appended`. Zero means no cap forwards were appended
    // here — a non-capturing class, or a bare `new C(...)` reached from a
    // sibling scope (bundled zod's `ZodType.transform() { new ZodEffects(...) }`)
    // where the trailing args are USER args, NOT caps. The provenance is now
    // explicit, so the codegen no longer infers it from the arg shape (the old
    // `new_site_args_carry_appended_caps` heuristic, which could misfire on a
    // forward-referenced capture class whose user args happened to equal its
    // captured locals).
    lower_new_impl(ctx, class_name, args, cap_args_appended == 0)
}

/// Member-callee `new ns.C(...)` construct (#5437): the captures were NOT
/// appended at the `new` site (the captured enclosing local is out of scope
/// there), so every synthesized `__perry_cap_*` ctor param fills from the
/// class's decl-site capture snapshot instead. All of `args` are USER args.
pub(crate) fn lower_new_member_captured(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &[Expr],
) -> Result<String> {
    lower_new_impl(ctx, class_name, args, true)
}

/// Refresh `lowered_args` after something that may have collected (#6969).
///
/// Two cases, and both are mandatory rather than defensive:
///
/// - a **rooted** argument is re-read from its slot, because the slot is a
///   *mutable* root that an evacuating cycle rewrites in place, leaving the
///   register pushed beforehand stale;
/// - an argument that was NOT rooted because it reads an *immutable* registered
///   root — a string literal, the only `operand_is_reloadable` case — is
///   **re-loaded**. It is never swept, but evacuation rewrote its handle
///   global too, so the cached register points at where the string used to be.
///   Re-lowering emits the load again and costs no runtime call. (A
///   shadow-slotted local or a module global is a registered root as well, but
///   a *mutable* one, so it takes a temp-root slot instead: re-deriving it
///   would observe an assignment made after the call-time value was taken.)
///
/// Called after the instance allocation and again before the late consumers
/// that sit behind further arbitrary lowering (field initializers, an inlined
/// constructor body) — each of those is another chance to relocate.
fn refresh_rooted_args(ctx: &mut FnCtx<'_>, group: &RootedGroup<'_>) -> Result<Vec<String>> {
    // `RootedGroup`'s re-read re-lowers a `Reload` operand through
    // `crate::expr::lower_expr`, while the ORIGINAL lowering of every
    // constructor argument went through `lower_constructor_arg` — which is
    // `lower_expr` with `discard_expr_value` forced false (#7590: the flag
    // means "this STATEMENT's value is discarded" and is not cleared on
    // recursion). Re-lowering under a different flag would be free to pick
    // `materialize_js_value_without_record`, so the re-read is wrapped in the
    // same suppression the first lowering had. A no-op for `Root` and `Reuse`
    // operands, which emit no lowering at all.
    let prev_discard = ctx.discard_expr_value;
    ctx.discard_expr_value = false;
    let out = group.reread_all(ctx);
    ctx.discard_expr_value = prev_discard;
    out
}

fn lower_new_impl(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &[Expr],
    caps_absent_from_args: bool,
) -> Result<String> {
    // #6969: one expression-scope temp-root barrier. The body below roots its
    // constructor arguments across the instance allocation, and it has ~20
    // return paths with `lowered_args` consumed at a dozen of them — one cut
    // here releases the group whichever path ran, instead of a release at each
    // that reviewers and future edits must keep balanced.
    //
    // #7615 slice 8: this is [`open_rooted_group`], and the escaping form is
    // the right one for exactly the reason its doc names — the release has to
    // post-dominate every one of those return paths, which no closure form can
    // own without swallowing the whole 1,000-line dispatch.
    //
    // The null MARKER slot `temp_root_scope_begin` used to push is gone with
    // it. It existed only because a raw truncate needs a base index even when
    // nothing else was pushed; `RootedGroup::release` truncates at the group's
    // own lowest slot and emits nothing at all when the group is empty, so the
    // marker has no work left to do. One fewer slot and one fewer push per
    // `new` site that roots anything.
    let mut group = open_rooted_group(args.len() + 1);
    let result = lower_new_impl_inner(ctx, class_name, args, caps_absent_from_args, &mut group);
    group.release(ctx);
    result
}

/// Lower every constructor argument into `group`, rooting each one **as it is
/// produced** rather than after the list (#6969: rooting a finished list
/// publishes an already-dangling argument 0 to the scanner, which turns a
/// silent wrong answer into a SIGSEGV — strictly worse than not rooting).
///
/// Returns the group indices, in argument order, for the caller to re-read at
/// the point it emits its call. `lower_constructor_arg` rather than
/// `RootedGroup::lower` because it clears `ctx.discard_expr_value` for the
/// operand — #7590: that flag means "this STATEMENT's value is discarded" and
/// is not cleared on recursion, so lowering an operand under it can evaluate a
/// typed-array store to `0`.
fn adopt_constructor_args<'a>(
    ctx: &mut FnCtx<'_>,
    args: &'a [Expr],
    group: &mut RootedGroup<'a>,
) -> Result<Vec<usize>> {
    let mut slots = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        let value = lower_constructor_arg(ctx, a)?;
        let collects = rooting::any_operand_may_collect(ctx, args[i + 1..].iter());
        slots.push(group.adopt(ctx, a, &value, collects));
    }
    Ok(slots)
}

fn lower_new_impl_inner<'a>(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &'a [Expr],
    caps_absent_from_args: bool,
    group: &mut RootedGroup<'a>,
) -> Result<String> {
    // Built-in Web classes that the runtime provides constructors for.
    // These are checked BEFORE the ctx.classes lookup because the user
    // code may shadow the name — if they do, the class lookup below
    // wins.
    if !ctx.classes.contains_key(class_name) {
        if matches!(class_name, "Crypto" | "CryptoKey" | "SubtleCrypto") {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            return Ok(ctx
                .block()
                .call(DOUBLE, "js_webcrypto_illegal_constructor", &[]));
        }
        if let Some((submod_key, exported_name)) =
            ctx.import_function_node_submodule.get(class_name).cloned()
        {
            if submod_key == "readline_promises" && exported_name == "Readline" {
                // #6986: `output` was live in an SSA register across `options`'
                // lowering AND across every `extra`'s — each an arbitrary
                // expression — before `js_readline_promises_readline_new` read
                // it. Same repair as the main class loop below: adopt into the
                // enclosing group as each operand is lowered (never after the
                // list — that publishes an already-dangling pointer, #6969),
                // and re-read at the call.
                //
                // The `undefined` fillers are literals, not operands: nothing
                // to root, and `Arg::Plain` keeps them out of the group so an
                // absent argument still costs no slot.
                let output = match args.first() {
                    Some(first) => {
                        let collects = rooting::any_operand_may_collect(ctx, args[1..].iter());
                        Some(group.lower(ctx, first, collects)?)
                    }
                    None => None,
                };
                let options = match args.get(1) {
                    Some(second) => {
                        let collects = rooting::any_operand_may_collect(ctx, args[2..].iter());
                        Some(group.lower(ctx, second, collects)?)
                    }
                    None => None,
                };
                for extra in args.iter().skip(2) {
                    let _ = lower_expr(ctx, extra)?;
                }
                ctx.pending_declares.push((
                    "js_readline_promises_readline_new".to_string(),
                    DOUBLE,
                    vec![DOUBLE, DOUBLE],
                ));
                let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                let output = match output {
                    Some(i) => group.reread(ctx, i)?,
                    None => undef.clone(),
                };
                let options = match options {
                    Some(i) => group.reread(ctx, i)?,
                    None => undef,
                };
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_readline_promises_readline_new",
                    &[(DOUBLE, &output), (DOUBLE, &options)],
                ));
            }
        }
        if let Some(val) = lower_builtin_new(ctx, class_name, args, group)? {
            return Ok(val);
        }
        // Aliased built-in import: a minified bundle renames a node built-in
        // constructor (`import { AsyncLocalStorage as xQ5 } from "async_hooks";
        // new xQ5()`). The syntactic callee is the alias `xQ5`, so the
        // canonical-name arms in `lower_builtin_new` (keyed on
        // `"AsyncLocalStorage"`) never fired and `new xQ5()` fell through to the
        // empty-object placeholder — the instance had no `.run`/`.getStore`, so
        // `xQ5().getStore()` threw `TypeError: getStore is not a function`.
        // Recover the original export name and retry. The alias is only present
        // here when it was NOT already a user-defined class (the enclosing
        // `!ctx.classes.contains_key(class_name)` guard), so a renamed import
        // can't shadow a real local class.
        if let Some(original) = ctx.imported_class_original_names.get(class_name).cloned() {
            if original != class_name {
                if let Some(val) = lower_builtin_new(ctx, &original, args, group)? {
                    return Ok(val);
                }
            }
        }
    }

    // Local class alias rerouting: `let C = SomeClass; new C()` lowers
    // as `Expr::New { class_name: "C" }` because the parser sees an
    // Ident callee. The HIR doesn't statically resolve "C" to the
    // underlying class, so without this rerouting we'd fall through to
    // the empty-object placeholder. The Stmt::Let lowering populates
    // `ctx.local_class_aliases[let_name] = class_name` whenever a
    // `let` is initialized from `Expr::ClassRef(class_name)`. We
    // resolve the class name to its underlying real class here and
    // shadow the parameter so the rest of the function uses the
    // resolved name (alloc, ctor lookup, field offsets, etc).
    // Shadow `class_name` with the alias-resolved version. The
    // `resolved_owned` binding outlives the shadowed `&str` because it's
    // declared in the same scope. After this point everything in
    // `lower_new` (alloc, ctor lookup, field offsets, this_stack push)
    // sees the resolved class name and the rest of the function is
    // identical to the direct `new SomeClass()` path.
    let resolved_owned: String;
    let class_name: &str = if !ctx.classes.contains_key(class_name) {
        if let Some(resolved) = ctx.local_class_aliases.get(class_name).cloned() {
            if resolved != class_name {
                resolved_owned = resolved;
                &resolved_owned
            } else {
                class_name
            }
        } else {
            class_name
        }
    } else {
        class_name
    };

    let class = match ctx.classes.get(class_name).copied() {
        Some(c) => c,
        None => {
            // #4698: `new <importedFn>()` where `<importedFn>` is a function —
            // or a `const`/`let` holding a closure — imported from another
            // module (e.g. `import { Dep } from "./m"`). The name is not a
            // registered class, so without this it would fall through to the
            // empty-object placeholder below and the constructor body would
            // never run (so `this.x = …` / `Object.defineProperty(this, …)`
            // writes are lost — the zod-v4 `ch._zod.onattach` crash for bare
            // named imports). When the name resolves to an imported binding
            // (`import_function_prefixes`) that isn't a V8-fallback specifier,
            // lower it as an `ExternFuncRef` value and construct it via
            // `js_new_function_construct`, which binds `this`, runs the body,
            // and returns the populated instance. Imported *classes* are
            // registered in `ctx.classes` and take the construction path above,
            // so they never reach here; a non-callable value still falls back
            // to a class_id=0 empty object inside the runtime helper.
            if ctx.import_function_prefixes.contains_key(class_name)
                && !ctx.import_function_v8_specifiers.contains_key(class_name)
            {
                // #6986: `func_double` was live across every argument's
                // lowering (arbitrary user code) and each argument across the
                // ones after it, all of them in bare SSA registers, before
                // `js_new_function_construct` read them. `lower_js_args_array`
                // is no rescue — it is a plain `alloca_entry_array` pack with
                // no `js_shadow_slot_bind`, so it copies whatever bits it is
                // handed, stale or not.
                let func_double = lower_expr(
                    ctx,
                    &Expr::ExternFuncRef {
                        name: class_name.to_string(),
                        param_types: Vec::new(),
                        return_type: HirType::Any,
                    },
                )?;
                let func_collects = rooting::any_operand_may_collect(ctx, args.iter());
                let func_root = group.adopt_emitted(ctx, Repr::Boxed, &func_double, func_collects);
                let arg_slots = adopt_constructor_args(ctx, args, group)?;
                let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
                for slot in &arg_slots {
                    lowered_args.push(group.reread(ctx, *slot)?);
                }
                let (args_ptr, args_len) = lower_js_args_array(ctx, &lowered_args);
                let func_double = group.reread_emitted(ctx, func_root);
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_new_function_construct",
                    &[(DOUBLE, &func_double), (PTR, &args_ptr), (I64, &args_len)],
                ));
            }
            // `new Function(p1, …, body)` with a RUNTIME-constructed body (the
            // const-foldable / static-literal case was handled in HIR lowering;
            // only dynamic bodies reach here). Perry is AOT-compiled and can't
            // compile an arbitrary runtime string, so historically this produced
            // a non-callable placeholder object. Route it through a runtime
            // helper that recognizes the small set of well-known codegen-library
            // templates (currently `depd`'s deprecation-wrapper, used eagerly by
            // `send` → Next.js) and returns a working native function; anything
            // else still gets the placeholder. NO general JS interpreter.
            if class_name == "Function" {
                // #6986: same shape as the imported-constructor branch above —
                // argument `i` was live in a bare SSA register across every
                // argument after it. `new Function(fresh(0), "return " + churn(N))`
                // is the reproducer named in the issue.
                let arg_slots = adopt_constructor_args(ctx, args, group)?;
                let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
                for slot in &arg_slots {
                    lowered_args.push(group.reread(ctx, *slot)?);
                }
                let (args_ptr, args_len) = lower_js_args_array(ctx, &lowered_args);
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_function_ctor_from_strings",
                    &[(PTR, &args_ptr), (I64, &args_len)],
                ));
            }
            // Built-in / native class (Promise, Error, Date, etc.) with
            // no dedicated lower_builtin_new handler — lower args for
            // side effects (closures, string literal interning) and
            // return a sentinel. Real dispatch happens via later
            // NativeMethodCall / PropertyGet paths.
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            // Allocate an empty object as the placeholder.
            let class_id = "0".to_string();
            let count = "0".to_string();
            let handle =
                ctx.block()
                    .call(I64, "js_object_alloc", &[(I32, &class_id), (I32, &count)]);
            return Ok(nanbox_pointer_inline(ctx.block(), &handle));
        }
    };

    // #6538: `caps_absent_from_args` is now authoritative. The bare-identifier
    // path (`lower_new`) derives it from `Expr::New::cap_args_appended` — the
    // explicit count of trailing cap forwards the HIR appended at THIS site —
    // and the member-callee path (`lower_new_member_captured`) passes `true`
    // unconditionally. This replaced the old `new_site_args_carry_appended_caps`
    // shape check, which inferred presence from the arg tail matching
    // `LocalGet(<cap_id>)` against the synthesized `__perry_cap_<id>` params
    // (#6530) and could misfire on a forward-referenced capture class whose
    // user args happened to equal its captured locals.

    // Lower the args first (constructor params).
    //
    // #6969: each argument is rooted as soon as it is lowered, NOT after the
    // loop — `new Pair(fresh(0), churn(N))` collects inside `churn`, which is
    // argument 1's lowering, and by then argument 0 exists only in an SSA
    // register. (Rooting after the loop is worse than not rooting at all: it
    // publishes an already-dangling pointer to the scanner.) The roots also
    // carry the arguments across the instance allocation below, which always
    // collects; the re-read is immediately after it (see `obj_box`), and the
    // scope cut in `lower_new_impl` is the release.
    let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
    for a in args {
        let value = lower_constructor_arg(ctx, a)?;
        // `collects` is unconditionally true: the instance allocation below
        // always collects, so every argument is live across it. That is the
        // same answer the pre-migration code gave by consulting
        // `operand_needs_root` with no window test at all.
        group.adopt(ctx, a, &value, true);
        lowered_args.push(value);
    }

    // #7615 slice 8: the field-count computation and the three-arm instance
    // allocation moved verbatim to `new_alloc.rs` (see its header for why).
    let alloc = super::new_alloc::emit_instance_alloc(ctx, class_name, class);
    let obj_handle = alloc.handle;
    // #7154: root the instance for the duration of the constructor body.
    //
    // Until now the instance existed ONLY as an SSA register while that body
    // ran, and a constructor body allocates. Under back-edge polls
    // (`PERRY_GC_MOVING_LOOP_POLLS=1`) an evacuating minor inside the
    // constructor RELOCATES it: the callee's own `this` shadow slot roots it,
    // so it survives and moves, and the collector rewrites the callee's root —
    // but not the caller's register, which is not a root at all. Every
    // subsequent use in this function then names from-space memory, and
    // `js_ctor_return_override` publishes that dead address straight into the
    // caller's shadow slot. The result is a *rooted* slot holding a dangling
    // pointer, which is why #7154's from-space scan only ever saw offenders
    // one or more cycles after the target died, with correct layout coverage.
    //
    // This is #7184's sibling: there the root store landed outside the pushed
    // frame, here it lands after a collection point. Same invariant — the root
    // store must dominate every site that can collect — and the same symptom
    // ("value is not a function" on a stale closure/instance field).
    //
    // The slot is released by the scope cut in `lower_new_impl`, which covers
    // all ~20 return paths below.
    // #7510: declare the canonical layout HERE — the instance is allocated, its
    // slots still hold the allocator's `undefined` fill, and the constructor
    // has not run. That ordering is the whole point: the post-constructor
    // `emit_typed_shape_layout_init` arrives after the only stores that wanted
    // the descriptor, so a `number`-declared class field could never pass its
    // intact-bit guard (#7512). Gated and suppressed as one — see
    // `layout_declared_at_allocation`.
    //
    // Before the instance root's push, so the handle this names is the one the
    // allocator returned: nothing between here and there can collect.
    let typed_layout_baked = alloc.typed_layout_baked;
    let constructor_layout_ready = alloc.constructor_stores_ready
        && super::typed_shape_init::layout_declared_at_allocation(ctx, class_name);
    emit_typed_shape_layout_declare(ctx, class_name, &obj_handle, typed_layout_baked);
    let instance = {
        let protected = construction_runs_user_code(ctx, class_name);
        Instance {
            root: group.adopt_emitted(ctx, Repr::Ptr, &obj_handle, protected),
            protected,
        }
    };
    let obj_box = nanbox_pointer_inline(ctx.block(), &obj_handle);
    // #6969: the instance allocation has run, so refresh every argument before
    // the constructor consumes them.
    lowered_args = refresh_rooted_args(ctx, group)?;

    // Constructor bodies may contain terminating recursive construction
    // shapes such as `if (typeof opts === "function") return new C(...)`.
    // Structurally inlining `C` while `C` is already active expands the
    // same constructor body forever at compile time. Use the standalone
    // constructor symbol for the nested construction instead; it preserves
    // the ordinary initializer path without recursively cloning HIR.
    //
    // Same redirect when inlining would alias the constructor's own locals
    // with the ENCLOSING closure's captures. `class F { constructor(){ const
    // t = this; t.mk = () => new F(t._cc); } }` lifts the arrow to a separate
    // function that captures `t` (the `const t = this` alias). When `new F`
    // inside that arrow is inlined, the inlined ctor's `const t = this` reuses
    // the same LocalId — which is a capture in this closure — so reads/writes
    // of `t` resolve through `js_closure_get_capture_bits` and land on the
    // CAPTURED outer instance instead of the freshly-allocated one (the new
    // instance gets no fields → wall 44 `BaseContext.setValue` → "Cannot read
    // properties of undefined"). The standalone symbol takes `this` as an
    // explicit parameter, so it is immune to the collision.
    let ctor_alias_collision = !ctx.closure_captures.is_empty()
        && local_constructor_symbol_exists(ctx, class)
        && class.constructor.as_ref().is_some_and(|c| {
            let mut ids: std::collections::HashSet<u32> = c.params.iter().map(|p| p.id).collect();
            collect_decl_local_ids(&c.body, &mut ids);
            ids.iter().any(|id| ctx.closure_captures.contains_key(id))
        });
    // [#bloat] Default: CALL the shared standalone-symbol constructor instead of
    // inlining the constructor body at every `new` site. The inlined ctor body
    // (field-init stores etc.) is the dominant per-`new`-site IR after the
    // allocator (~136 lines/site); calling the shared ctor symbol emits it once.
    // Measured win-win vs inlining: ~2.5x FASTER on an 8M construct-heavy loop
    // AND much smaller IR. Opt back into inlining with PERRY_INLINE_CTOR=1.
    // Restricted to classes with their OWN constructor: a no-own-ctor subclass
    // (`class C extends B {}`) gets a synthesized symbol, but the symbol-call
    // path doesn't reproduce the inline path's leaf-keys/shape setup, so by-name
    // field reads on the instance return undefined. Own-ctor classes (incl. ones
    // with `super(...)`/rest params) round-trip correctly through the call.
    let force_ctor_call = std::env::var_os("PERRY_INLINE_CTOR").is_none()
        && class.constructor.is_some()
        && local_constructor_symbol_exists(ctx, class)
        // These bases create an exotic object in `super()`. Keep their own
        // constructors inline so the replacement derived-this value remains
        // authoritative at the surrounding `new` expression.
        && !class
            .extends_name
            .as_deref()
            .is_some_and(crate::expr::is_other_builtin_constructor_name);
    if ctx.class_stack.iter().any(|active| active == class_name)
        || ctor_alias_collision
        || force_ctor_call
    {
        // Apply ECMAScript constructor return-override semantics on the
        // standalone-symbol path too. The shared `<class>_constructor` symbol
        // returns `undefined` for an ordinary ctor (implicit `return this`) or
        // the explicitly-returned value for a `return <expr>` body. Pre-fix this
        // path discarded that value and always yielded `obj_box`, so a ctor like
        // chalk's `class Chalk { constructor(o){ return chalkFactory(o); } }`
        // produced the empty default instance instead of the returned factory
        // function ("value is not a function" on `new Chalk(...).red(...)`).
        // `js_ctor_return_override` returns `obj_box` for an `undefined`/
        // primitive (base) return, so ordinary ctors are unaffected.
        //
        // #2768/new.target: the standalone `<class>_constructor` symbol is a
        // separate compiled function, so its only `new.target` source is the
        // runtime cell — which this path never set, leaving `new.target ===
        // undefined` for a base class. Set the cell to this class's ref (the
        // `INT32_TAG | class_id` value `Expr::ClassRef` produces) around the
        // call and restore it after, but ONLY when the ctor actually reads
        // `new.target`, so the common ctor keeps the zero-overhead fast path.
        // The gate spans the WHOLE super(...) chain, not just the leaf's own
        // body: the symbol inlines `super(...)` into itself, so an ancestor
        // ctor that reads `new.target` (e.g. an abstract-class guard in a base)
        // observes the same cell — `new Child()` where only `Base` reads
        // `new.target` would otherwise see `undefined` instead of `Child`.
        // ponytail: a throw inside the ctor skips the restore, leaving the cell
        // set — same edge case the runtime construct paths already have; fix
        // holistically if it bites.
        // #7664: `prev` is saved across the WHOLE constructor body, and the
        // cell it comes out of is a registered mutable root that evacuation
        // rewrites — so it goes in a temp root, not a bare register.
        let saved_new_target = if ctor_chain_uses_new_target(ctx, class) {
            ctx.class_ids.get(class_name).copied().map(|cid| {
                let class_ref = double_literal(f64::from_bits(
                    crate::nanbox::INT32_TAG | (cid as u64 & 0xFFFF_FFFF),
                ));
                crate::rooting::new_target_save(ctx, &class_ref)
            })
        } else {
            None
        };
        // Constructor-free construction: when the whole constructor body is a
        // run of `this.<f> = <param>` stores into the shape the inline bump
        // allocator just baked, store the fields here and skip the call. See
        // `ctor_prologue_stores` for the proof — the short version is that every
        // condition the per-field precheck tests is a compile-time constant this
        // very site wrote three instructions ago, so the only things left to
        // decide at runtime are the policy latch, the INTACT result for a
        // runtime-declared pointer layout, and whether raw-f64 values are plain
        // finite numbers. All are decided ONCE for the construction instead of
        // once per field.
        //
        // Emitted only when `saved_new_target` is absent: a `new.target`-reading
        // chain needs the runtime cell the call path sets, and a class whose
        // body is nothing but field stores cannot read it anyway.
        // `local_constructor_symbol_exists` is re-tested because this arm is
        // also reached by the recursion guard and the capture-alias redirect,
        // neither of which requires it — and the diamond's slow arm IS the call,
        // so emitting it when the call cannot be emitted would leave the fast
        // arm branching into a block nothing terminates.
        let prologue_plan =
            if saved_new_target.is_none() && local_constructor_symbol_exists(ctx, class) {
                super::ctor_prologue_stores::prologue_store_plan(
                    ctx,
                    class_name,
                    class,
                    lowered_args.len(),
                    constructor_layout_ready,
                )
            } else {
                None
            };
        // `(merge block, fast-arm predecessor label)` when the diamond was
        // emitted; the slow arm is the current block from here on.
        let mut prologue_merge: Option<(usize, String)> = None;
        if let Some(plan) = prologue_plan.as_ref() {
            // Pure derived values are safe to compute before the diamond. A
            // non-finite result selects the ordinary constructor below, which
            // recomputes the same `fadd` before taking its guarded slow store.
            let prologue_values: Vec<String> = plan
                .iter()
                .map(|store| {
                    let arg = &lowered_args[store.arg_index];
                    store.numeric_addend.map_or_else(
                        || arg.clone(),
                        |addend| ctx.block().fadd(arg, &double_literal(addend)),
                    )
                })
                .collect();
            let fast_idx = ctx.new_block("ctor_prologue.fast");
            let slow_idx = ctx.new_block("ctor_prologue.slow");
            let merge_idx = ctx.new_block("ctor_prologue.merge");
            let fast_label = ctx.block_label(fast_idx);
            let slow_label = ctx.block_label(slow_idx);
            let merge_label = ctx.block_label(merge_idx);
            {
                let blk = ctx.block();
                // The sticky policy latch, volatile for the same reason every
                // other reader loads it volatile: the runtime flips it 0 -> 1
                // mid-execution and LLVM must not hoist a stale 0 across it.
                let flag =
                    blk.load_volatile(crate::types::I8, "@PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED");
                let mut acc = blk.icmp_eq(crate::types::I8, &flag, "0");
                // A pointer-bearing layout is installed by the declaration
                // immediately above. Unlike the pointer-free baked case, its
                // success is dynamic: an ambiguous ShapeId falls back to a
                // per-object descriptor, while a rejected declaration clears
                // INTACT. Test the authoritative header bit before bypassing
                // the constructor's per-field guards.
                if !typed_layout_baked {
                    let obj_ptr = blk.inttoptr(I64, &obj_handle);
                    // `obj_handle` points just past the 8-byte GcHeader;
                    // `_reserved: u16` starts two bytes into that header.
                    let reserved_ptr = blk.gep(I8, &obj_ptr, &[(I64, "-6")]);
                    let reserved = blk.load(I16, &reserved_ptr);
                    let intact_bits = blk.and(I16, &reserved, "4096");
                    let intact = blk.icmp_ne(I16, &intact_bits, "0");
                    acc = blk.and(crate::types::I1, &acc, &intact);
                }
                for (store_index, store) in plan.iter().enumerate() {
                    if !store.requires_raw_f64 {
                        continue;
                    }
                    let bits = blk.bitcast_double_to_i64(&prologue_values[store_index]);
                    let finite =
                        crate::expr::class_field_inline_guard::emit_plain_finite_number_check(
                            blk, &bits,
                        );
                    acc = blk.and(crate::types::I1, &acc, &finite);
                }
                blk.cond_br(&acc, &fast_label, &slow_label);
            }
            ctx.current_block = fast_idx;
            // arm64_32 watchOS: the fields region starts at
            // `size_of::<ObjectHeader>()` past the user pointer (16 on both
            // LP64 and ILP32 since #8047) — same derivation as every other
            // packed slot access.
            let header_skip =
                crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
            let fields_base = {
                let blk = ctx.block();
                let obj_ptr = blk.inttoptr(I64, &obj_handle);
                blk.gep(crate::types::I8, &obj_ptr, &[(I64, &header_skip)])
            };
            let parent_bits = ctx.block().bitcast_double_to_i64(&obj_box);
            for (store_index, store) in plan.iter().enumerate() {
                let (field_addr, child_bits) = {
                    let blk = ctx.block();
                    let field_ptr =
                        blk.gep(DOUBLE, &fields_base, &[(I64, &store.slot.to_string())]);
                    // GC_STORE_AUDIT(INIT): constructor prologue initializes a freshly allocated, unpublished object's field.
                    blk.store(DOUBLE, &prologue_values[store_index], &field_ptr);
                    let field_addr = blk.ptrtoint(&field_ptr, I64);
                    let child_bits = blk.bitcast_double_to_i64(&prologue_values[store_index]);
                    (field_addr, child_bits)
                };
                if !store.requires_raw_f64 {
                    crate::expr::emit_write_barrier_slot_value_and_generation_tested(
                        ctx,
                        &obj_handle,
                        &parent_bits,
                        &field_addr,
                        &child_bits,
                        "ctor_prologue",
                    );
                }
            }
            let fast_pred_label = ctx.block().label.clone();
            ctx.block().br(&merge_label);
            prologue_merge = Some((merge_idx, fast_pred_label));
            ctx.current_block = slow_idx;
        }
        if let Some(ctor_ret) = call_local_constructor_symbol(
            ctx,
            class,
            &obj_box,
            &lowered_args,
            caps_absent_from_args,
        ) {
            if let Some(save) = &saved_new_target {
                crate::rooting::new_target_restore(ctx, save);
            }
            // Rejoin the constructor-free arm. The phi is over the CONSTRUCTOR'S
            // RETURN VALUE, not the instance: the fast arm ran no constructor,
            // which is `undefined` — the same thing an ordinary ctor body
            // returns — so `emit_ctor_return_override` below yields the instance
            // on both arms without this path having to reason about it.
            let ctor_ret = match prologue_merge.take() {
                Some((merge_idx, fast_pred)) => {
                    let slow_pred = ctx.block().label.clone();
                    let merge_label = ctx.block_label(merge_idx);
                    ctx.block().br(&merge_label);
                    ctx.current_block = merge_idx;
                    let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    ctx.block()
                        .phi(DOUBLE, &[(&undef, &fast_pred), (&ctor_ret, &slow_pred)])
                }
                None => ctor_ret,
            };
            // #7154: the constructor body has run, so every register holding
            // the instance is potentially pre-move. Re-read it from its root
            // before anything else touches it — `emit_typed_shape_layout_init`
            // would otherwise install the layout descriptor on the abandoned
            // from-space copy, and `js_ctor_return_override` would hand the
            // caller that copy's address.
            let (obj_handle, obj_box) =
                reload_instance(ctx, group, &instance, &obj_handle, &obj_box);
            // The constructor body has run and set the declared fields; register
            // the typed raw-f64/pointer slot layout so class-field accesses hit
            // the slot-direct fast path instead of the by-name hashmap fallback.
            // The inline-ctor path does this at its tail (below); this
            // standalone-symbol path returns here, so it must do it too.
            emit_typed_shape_layout_init(ctx, class_name, &obj_handle);
            // Write-back: propagate constructor mutations to outer captured locals.
            // The standalone constructor symbol receives captured values by value
            // and stores mutations to `this.__perry_cap_*` fields, but never
            // updates the outer local's alloca slot. Read the fields back here so
            // the enclosing scope sees the updated values (e.g. `++called` in a
            // subclass constructor is visible after `new SubClass(...)` returns).
            // When `caps_absent_from_args` is true (member-callee `new ns.C()`
            // path), the HIR `args` slice contains ONLY user args — the cap args
            // were NOT appended. Passing `args` to `emit_class_capture_writeback`
            // would let the position-based lookup misidentify a user `LocalGet` as
            // a cap arg and write to the wrong outer slot. Fall back to suffix-based
            // lookup (empty slice) in that case.
            let writeback_args = if caps_absent_from_args { &[][..] } else { args };
            emit_class_capture_writeback(ctx, class, &obj_handle, writeback_args);
            let is_derived = class.extends.is_some()
                || class.extends_name.is_some()
                || class.native_extends.is_some()
                || class.extends_expr.is_some();
            let final_box =
                super::new_helpers::emit_ctor_return_override(ctx, &obj_box, &ctor_ret, is_derived);
            return Ok(final_box);
        }
        if let Some(save) = &saved_new_target {
            crate::rooting::new_target_restore(ctx, save);
        }
        // #6921: `call_local_constructor_symbol` returned `None` — this module
        // has no `<Class>_constructor` entry, so no constructor ran and the
        // instance leaves here exactly as `js_object_alloc_class_*` produced
        // it. Every OTHER `new` exit initializes the typed-shape layout; this
        // one used to return the instance at `GC_LAYOUT_POINTER_FREE` with no
        // `TypedLayoutDescriptor`, the one state in which the per-store
        // `layout_note_slot` call is load-bearing for GC correctness rather
        // than a precision hint — so eliding that note (Phase 4b.1) could
        // strand a live child on an object the collector scans zero slots of.
        //
        // Initialize it here too, so the invariant "a user-class instance
        // reaching a class-field store carries a typed descriptor, or is
        // explicitly `GC_LAYOUT_UNKNOWN`" is total. This is safe by
        // construction rather than by reasoning about this path: the fields
        // are still `TAG_UNDEFINED` (no ctor ran), and `init_typed_shape_layout`
        // validates every live field word before promoting — a raw-f64 slot
        // holding `undefined` fails `layout_raw_f64_bits` and the object lands
        // in `GC_LAYOUT_UNKNOWN`, the conservative state, instead of a wrong
        // mask. `emit_typed_shape_layout_init` is itself a no-op for a class
        // with no `class_keys_globals` entry.
        //
        // Reachability, measured (not assumed): this arm is currently DEAD.
        // `call_local_constructor_symbol` returns `None` only when
        // `ctx.methods` lacks `(class.name, "<Class>_constructor")`, but
        // `lower_new_impl` resolves `class` exclusively from `ctx.classes`
        // (the `class_table`), and `build_method_names` iterates
        // `class_table.values()` inserting that key unconditionally for every
        // entry — local and imported alike. So no class reaching here can miss
        // it. An instrumented compiler over the whole `test_gap_*` corpus plus
        // hand-written recursive-construction shapes never hit this arm.
        // The emitter stays anyway: the invariant must hold by construction at
        // this exit, not by an accident of the registry that a future change
        // to `build_method_names` (or a new `ctx.classes` population path)
        // could silently revoke.
        emit_typed_shape_layout_init(ctx, class_name, &obj_handle);
        return Ok(obj_box);
    }

    // Allocate a `this` slot and store the new object there. The
    // slot lives on this_stack for the duration of the inlined ctor
    // body (which may span many basic blocks and contain nested
    // closures that capture `this`), so hoist to the entry block for
    // dominance safety.
    let this_slot = ctx.func.alloca_entry(DOUBLE);
    // #7202: this alloca holds the INSTANCE for the whole inlined constructor
    // body, and every `this` read below is a `load` from it. It is a plain
    // `alloca_entry` — not a shadow slot, not a temp root — so an evacuating
    // minor at a field initializer's back-edge poll neither marks nor rewrites
    // it, and every `this.x = …` after that collection stores into abandoned
    // from-space memory.
    //
    // #7192 rooted the instance for the *caller* (`instance.root` above,
    // re-read by `reload_instance` at the tail) precisely because this window
    // collects — so the object survives and MOVES. That made the caller's copy
    // correct and left this one behind: the same address, taken one line later,
    // that nothing rewrites. The #7154 comment on `ctor_result_slot` states the
    // invariant and applies it only to that sibling.
    //
    // Reachability is the default, not an opt-in: `force_ctor_call` requires
    // `class.constructor.is_some()`, so `class C { payload = mk() }` and
    // `class C extends B {}` take this path with `PERRY_INLINE_CTOR` unset —
    // and `construction_runs_user_code` (which gates `instance.root`) is true
    // for exactly those, i.e. the code already asserts this window collects.
    //
    // Binding it — rather than routing `Expr::This` through a temp root —
    // leaves all ~30 `ctx.this_stack.last()` readers untouched: they load from
    // the alloca, and `js_shadow_slot_bind` makes evacuation rewrite the alloca
    // in place. The `undefined` seed is required by `root_entry_alloca`'s
    // contract: the bind is hoisted to entry setup, so the slot is live to the
    // collector before this store executes.
    //
    // Gated on `instance.protected`, i.e. on the very same
    // `construction_runs_user_code` predicate that decided the instance needed
    // a temp root at all. When it is false no user code runs between this store
    // and the pop, so nothing in the window can collect and the slot cannot go
    // stale — and a class with no constructor, no fields and no heritage keeps
    // its previous IR exactly, frame size included. One predicate, one place:
    // forking a second one here is how #7114's two predicates diverged.
    if instance.protected {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        ctx.func
            .entry_allocas_push_store(DOUBLE, &undef, &this_slot);
        ctx.block().store(DOUBLE, &obj_box, &this_slot);
        crate::expr::root_entry_alloca(ctx, &this_slot);
    } else {
        ctx.block().store(DOUBLE, &obj_box, &this_slot);
    }
    ctx.this_stack.push(this_slot.clone());
    ctx.class_stack.push(class_name.to_string());

    // #2768/new.target: `new C()` is fully inlined here, so the runtime
    // `js_new_target_*` cell is never set on this path. Bind `new.target`
    // inside the (own or inherited-via-super) constructor body to THIS leaf
    // class's ref via a `new_target_stack` slot. Using the codegen slot
    // rather than the runtime cell keeps a non-constructor method called from
    // the ctor body — compiled as a separate function whose `new_target_stack`
    // is empty — correctly reading `undefined`. A class ref is
    // `INT32_TAG | class_id`, the same value `Expr::ClassRef` produces, so
    // `new.target === C`, `new.target.name`, and `new.target.prototype` all
    // work. Falls back to `undefined` if the class id is somehow unresolved.
    let new_target_bits = ctx
        .class_ids
        .get(class_name)
        .map(|&cid| crate::nanbox::INT32_TAG | (cid as u64 & 0xFFFF_FFFF))
        .unwrap_or(crate::nanbox::TAG_UNDEFINED);
    let new_target_slot = ctx.func.alloca_entry(DOUBLE);
    ctx.block().store(
        DOUBLE,
        &double_literal(f64::from_bits(new_target_bits)),
        &new_target_slot,
    );
    ctx.new_target_stack.push(new_target_slot);

    // Set up the inline-constructor return target. An explicit `return`
    // inside the (about-to-be-inlined) ctor body must apply spec
    // return-override semantics and yield the `new` expression's value —
    // NOT emit a function-level `ret` that terminates the enclosing
    // function. `Stmt::Return` overwrites the slot with the returned value
    // (or throws for a derived ctor returning a primitive), then branches to
    // `after_idx`. Refs class/subclass/derived-class-return-override-*.
    //
    // #7154: the slot starts at `undefined`, NOT at `this`.
    //
    // It is a plain entry alloca — not a shadow slot, not a temp root — so the
    // collector neither marks nor rewrites it. Seeding it with `obj_box` put
    // the PRE-constructor instance address in unrooted memory for the whole
    // body; on fall-through (no explicit `return`) `js_ctor_return_override`
    // then saw an *object* in `raw` and returned THAT — the stale address —
    // discarding the re-read `obj_box` the reload below just recovered. The
    // instance-root fix was defeated at its last instruction.
    //
    // `undefined` is exactly equivalent for every path and carries no address:
    //   - fall-through     → `raw` is undefined → the override yields `this_val`,
    //                        i.e. the RE-READ instance (previously: `raw`, the
    //                        stale one — same value only when nothing moved);
    //   - bare `return;`   → the slot is untouched, so also `this_val`;
    //   - `return <expr>`  → `Stmt::Return` overwrote the slot; unchanged;
    //   - inherited-symbol ctor → the call's return value overwrote it; unchanged.
    let ctor_result_slot = ctx.func.alloca_entry(DOUBLE);
    ctx.block().store(
        DOUBLE,
        &double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
        &ctor_result_slot,
    );
    let after_idx = ctx.new_block("ctor.return.after");
    let after_label = ctx.block_label(after_idx);
    ctx.inline_ctor_return.push(crate::expr::InlineCtorReturn {
        result_slot: ctor_result_slot.clone(),
        after_label,
        // A class is "derived" (and thus subject to the stricter
        // return-override rules) if it has ANY heritage — a named parent,
        // a resolved parent id, a native parent, or a dynamic
        // `extends <expr>` clause (e.g. `extends class {}`).
        is_derived: class.extends.is_some()
            || class.extends_name.is_some()
            || class.native_extends.is_some()
            || class.extends_expr.is_some(),
    });

    // Apply ANCESTOR field initializers — refs #420 / #631-followup.
    //
    // For the own-ctor case (class has its own ctor body): apply ALL
    // ancestors up-front so the parent body's first read of any inherited
    // field sees the right initial value. The leaf's own fields are
    // applied at the SuperCall site (see expr.rs Expr::SuperCall).
    //
    // For the no-own-ctor case: only apply fields up to and INCLUDING
    // the inherited-ctor class. Intermediate classes between the
    // inherited-ctor and the leaf (e.g. SQLiteBaseInteger between
    // SQLiteColumn and SQLiteInteger in drizzle) have their fields
    // applied AFTER the inherited-ctor body returns, because their
    // initializers may reference state set by the parent body (e.g.
    // SQLiteBaseInteger's `autoIncrement = this.config.autoIncrement`
    // depends on Column's body running `this.config = config` first).
    let has_own_ctor = class.constructor.is_some();
    let has_extends = class.extends_name.is_some();
    let has_imported_ctor = ctx.imported_class_ctors.contains_key(class_name);
    // A local class whose imported parent is represented by both a static
    // name and a runtime heritage value must construct through that runtime
    // value. The source module's standalone ctor owns all imported-parent
    // field initialization; this module must only replay the local leaf.
    // #9043: the dynamic heritage edge may belong to a constructor-free
    // ANCESTOR rather than the leaf itself (`Leaf -> Mid -> <captured Base>`).
    // The registered runtime parent is keyed by that edge's owner (`Mid`), and
    // the edge remains authoritative across every static default-ctor hop below
    // it. Keep the owner name for the dispatch and use the boolean everywhere
    // the old direct-leaf-only path deferred static/imported construction.
    let dynamic_parent_owner = if has_imported_ctor {
        None
    } else {
        default_ctor_dynamic_parent_owner(ctx, class)
    };
    let defer_to_dynamic_parent = dynamic_parent_owner.is_some();
    let builtin_parent_runtime = if !has_own_ctor && !has_imported_ctor {
        match class.extends_name.as_deref() {
            Some("Writable") => Some("js_node_stream_writable_subclass_init"),
            Some("Duplex") => Some("js_node_stream_duplex_subclass_init"),
            Some("Transform") => Some("js_node_stream_transform_subclass_init"),
            _ => None,
        }
    } else {
        None
    };
    // `class X extends Request/Response {}` with no own constructor — forward
    // `new X(input, init)` to the native fetch subclass-init shim (stashes the
    // underlying handle on `this`). Two user args (input/init), unlike the
    // single-opts stream shims above, so it has its own emit block below.
    let fetch_parent_runtime = if !has_own_ctor && !has_imported_ctor {
        match class.extends_name.as_deref() {
            Some("Request") => Some("js_request_subclass_init"),
            Some("Response") => Some("js_response_subclass_init"),
            _ => None,
        }
    } else {
        None
    };
    // `class X extends Promise {}` with no own ctor — `new X(executor)` runs the
    // Promise constructor against a hidden backing cell (see new_helpers). (#5991)
    let promise_parent_runtime =
        !has_own_ctor && !has_imported_ctor && class.extends_name.as_deref() == Some("Promise");
    // `class X extends URLSearchParams {}` (Next's `ReadonlyURLSearchParams`) with
    // no own ctor — `new X(init)` builds a native URLSearchParams and stashes it
    // as a hidden backing on `this` (#6710 follow-up).
    let usp_parent_runtime = !has_own_ctor
        && !has_imported_ctor
        && class.extends_name.as_deref() == Some("URLSearchParams");
    let inherited_ctor_class: Option<String> = if !has_own_ctor && has_extends {
        // Walk the inheritance chain to find the closest ancestor with
        // an explicit ctor — same logic as the body-inlining loop below.
        let mut walker = class.extends_name.as_deref();
        let mut found: Option<String> = None;
        while let Some(pname) = walker {
            if let Some(parent_class) = ctx.classes.get(pname).copied() {
                if parent_class.constructor.is_some() {
                    found = Some(pname.to_string());
                    break;
                }
                if parent_class.extends_expr.is_some() {
                    break;
                }
                walker = parent_class.extends_name.as_deref();
            } else {
                break;
            }
        }
        found
    } else {
        None
    };
    // Issue #740: synthesized `__perry_cap_<id>` ctor params (added by
    // `lower_class_decl` when a class declared inside a function captures
    // outer-scope locals) must be visible to field initializers, since
    // those field initializers were rewritten to read the captured value
    // via `LocalGet(fresh_param_id)`. Bind ALL ctor params (own + cap)
    // before `apply_field_initializers_recursive` so the soft-fallback at
    // `LocalGet` codegen doesn't return 0.0. Locals/local_types are
    // saved-and-restored around the whole inlined ctor flow below; we
    // mirror that here so the ctor params don't leak out of `new`.
    let ctor_capture_fill = ctx
        .class_ids
        .get(class_name)
        .copied()
        .map(|cid| CaptureFill {
            cid,
            caps_absent_from_args,
        });
    let mut saved_scope_for_ctor = class.constructor.as_ref().map(|ctor| {
        bind_inline_constructor_params(ctx, &ctor.params, &lowered_args, args, ctor_capture_fill)
    });
    // #9081: the ctor body below is lowered into the CALLER's frame, whose
    // slot map never saw the ctor's locals. Root them (and the params just
    // bound) before the field initializers or body can allocate.
    if let Some(ctor) = &class.constructor {
        crate::expr::root_inlined_ctor_pointer_locals(ctx, &ctor.params, &ctor.body);
    }

    // A dynamic parent constructor owns the fields above its registered edge.
    // Every local class from the edge owner through the leaf is derived, so
    // their fields run only after that runtime `super(...args)` returns.
    if dynamic_parent_owner.is_none() {
        if let Some(stop_at) = inherited_ctor_class.clone() {
            apply_field_initializers_recursive(
                ctx,
                class_name,
                FieldInitMode::UpToInclusive(stop_at),
            )?;
        } else {
            apply_field_initializers_recursive(ctx, class_name, FieldInitMode::AncestorsOnly)?;
        }
    }
    if !has_extends && class.extends_expr.is_none() {
        // Base class — no super(), apply own fields now (before body).
        apply_field_initializers_recursive(ctx, class_name, FieldInitMode::SelfOnly)?;
    }

    // If there's a constructor, inline its body. We allocate slots for
    // each constructor parameter and pre-populate them with the lowered
    // argument values. Locals/local_types are saved and restored to keep
    // the constructor's bindings scoped to its body — they don't leak
    // back into the enclosing function.
    if let Some(ctor) = &class.constructor {
        // Issue #740: ctor params were already bound above so field
        // initializers could read them. Don't re-bind (the slots already
        // hold the lowered arg values); just lower the body.
        let _ = ctor;
        // ECMAScript TDZ-on-`this`: a DERIVED constructor (any heritage) that
        // never calls `super()` leaves `this` uninitialized, so the implicit
        // `return this` throws ReferenceError. Detect the static no-super case
        // and throw at construction time. (A base class with no heritage has
        // `this` initialized up front, so this only applies when derived.)
        // Refs class/subclass/builtin-objects/*/super-must-be-called.
        let is_derived_class = class.extends.is_some()
            || class.extends_name.is_some()
            || class.native_extends.is_some()
            || class.extends_expr.is_some();
        if is_derived_class {
            crate::expr::this_super_call::push_shared_super_called_slot(ctx);
        }
        // A closure-captured `super()` may run during construction, so it
        // suppresses the static throw — but only when the body never touches
        // `this` directly (a direct `this` in a no-direct-super derived ctor
        // throws before any closure could fire). A value-bearing `return`
        // takes the return-override path instead of the implicit `return
        // this`, so it suppresses the throw too.
        let no_super_throw_statically = !ctor_body_calls_super(&ctor.body)
            && !(ctor_body_closure_calls_super(&ctor.body) && !ctor_body_uses_this(&ctor.body))
            && !ctor_body_has_value_return(&ctor.body);
        if is_derived_class && no_super_throw_statically {
            ctx.block()
                .call(DOUBLE, "js_throw_reference_error_this_before_super", &[]);
            ctx.block().unreachable();
        } else {
            // A constructor body is inlined into the surrounding function.
            // Its `return` completes the constructor, not an enclosing
            // source-level `try`, so return cleanup must count only handlers
            // opened by the inlined body. The caller's LLVM EH scope remains
            // active and still receives every throwing invoke.
            let caller_try_depth = ctx.try_depth;
            ctx.try_depth = 0;
            let lower_result =
                crate::stmt::lower_stmts(ctx, &class.constructor.as_ref().unwrap().body);
            ctx.try_depth = caller_try_depth;
            lower_result?;
        }
        if is_derived_class {
            crate::expr::this_super_call::pop_shared_super_called_slot(ctx);
        }

        // Restore the enclosing function's local scope.
        if let Some(saved) = saved_scope_for_ctor.take() {
            restore_inline_constructor_scope(ctx, saved);
        }
    } else {
        // No own constructor — walk the parent chain to find an
        // inherited constructor and inline it. TypeScript semantics:
        // `class Child extends Parent {}` auto-forwards constructor
        // arguments to the parent constructor.
        let mut parent_name = class.extends_name.as_deref();
        let mut found_inherited_ctor = false;
        while let Some(pname) = parent_name {
            if let Some(parent_class) = ctx.classes.get(pname).copied() {
                if let Some(parent_ctor) = &parent_class.constructor {
                    // #5437: snapshot-fill the parent's cap params. #806:
                    // unconditionally caps-absent — a capturing leaf always
                    // has a synthesized own ctor, so a leaf reaching this
                    // walk appended no cap args; the site's flag split the
                    // tail by the ANCESTOR's caps and ate user args.
                    let parent_capture_fill =
                        ctx.class_ids.get(pname).copied().map(|cid| CaptureFill {
                            cid,
                            caps_absent_from_args: true,
                        });
                    let saved_scope = bind_inline_constructor_params(
                        ctx,
                        &parent_ctor.params,
                        &lowered_args,
                        args,
                        parent_capture_fill,
                    );
                    // #9081: same frame-splice rooting as the own-ctor
                    // inline above, for the inherited body.
                    crate::expr::root_inlined_ctor_pointer_locals(
                        ctx,
                        &parent_ctor.params,
                        &parent_ctor.body,
                    );

                    // Push the parent class name so `this` inside the
                    // parent ctor body resolves field names via the
                    // parent's field list.
                    ctx.class_stack.pop();
                    ctx.class_stack.push(pname.to_string());

                    // The inherited body is the `super(...args)` half of this
                    // class's implicit default derived constructor.  A
                    // `return <object>` in that ANCESTOR replaces the value of
                    // `this`, but it does not complete the leaf constructor:
                    // the leaf's instance fields and private elements still
                    // have to be installed on the replacement object.
                    //
                    // Reusing the leaf's inline-return target made the parent
                    // return branch straight to the end of `new C(...)`.
                    // Besides skipping the leaf initializers, subsequent
                    // lowering happened in an already-terminated block and
                    // produced references to SSA names that were never
                    // emitted.  Give the parent body its own completion slot,
                    // apply its constructor return-override here, and publish
                    // the resulting `this` through the rooted this-slot before
                    // continuing with the leaf initialization.
                    let parent_result_slot = ctx.func.alloca_entry(DOUBLE);
                    ctx.block().store(
                        DOUBLE,
                        &double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
                        &parent_result_slot,
                    );
                    let parent_after_idx = ctx.new_block("inherited.ctor.return.after");
                    let parent_after_label = ctx.block_label(parent_after_idx);
                    let parent_is_derived = parent_class.extends.is_some()
                        || parent_class.extends_name.is_some()
                        || parent_class.native_extends.is_some()
                        || parent_class.extends_expr.is_some();
                    ctx.inline_ctor_return.push(crate::expr::InlineCtorReturn {
                        result_slot: parent_result_slot,
                        after_label: parent_after_label.clone(),
                        is_derived: parent_is_derived,
                    });
                    if parent_is_derived {
                        crate::expr::this_super_call::push_shared_super_called_slot(ctx);
                    }
                    let caller_try_depth = ctx.try_depth;
                    ctx.try_depth = 0;
                    let lower_result = crate::stmt::lower_stmts(ctx, &parent_ctor.body);
                    ctx.try_depth = caller_try_depth;
                    lower_result?;
                    if parent_is_derived {
                        crate::expr::this_super_call::pop_shared_super_called_slot(ctx);
                    }
                    let parent_return = ctx
                        .inline_ctor_return
                        .pop()
                        .expect("inherited constructor return target");
                    if !ctx.block().is_terminated() {
                        ctx.block().br(&parent_after_label);
                    }
                    ctx.current_block = parent_after_idx;
                    let parent_raw = ctx.block().load(DOUBLE, &parent_return.result_slot);
                    let inherited_this = ctx.block().load(DOUBLE, &this_slot);
                    let effective_this = super::new_helpers::emit_ctor_return_override(
                        ctx,
                        &inherited_this,
                        &parent_raw,
                        parent_return.is_derived,
                    );
                    ctx.block().store(DOUBLE, &effective_this, &this_slot);
                    if instance.protected {
                        crate::expr::root_entry_alloca(ctx, &this_slot);
                    }

                    // Restore class_stack to the child.
                    ctx.class_stack.pop();
                    ctx.class_stack.push(class_name.to_string());

                    restore_inline_constructor_scope(ctx, saved_scope);

                    // The shared post-constructor tail below installs every
                    // class below this inherited constructor exactly once.
                    // Keeping a second copy here was harmless for ordinary
                    // assignment-like fields, but became observable as a
                    // duplicate private-element installation.
                    found_inherited_ctor = true;
                    break; // Found and inlined the parent ctor.
                }
                // A dynamic heritage expression is the authoritative next
                // edge. Following its optional static metadata would skip the
                // runtime class value and can call the wrong constructor.
                if parent_class.extends_expr.is_some() {
                    break;
                }
                parent_name = parent_class.extends_name.as_deref();
            } else {
                break;
            }
        }
        if !found_inherited_ctor {
            if let Some(kind) = node_stream_parent_kind(ctx, class) {
                let undef_lit =
                    crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                let opts_box = lowered_args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| undef_lit.clone());
                let runtime_fn = match kind {
                    "readable" => "js_node_stream_readable_subclass_init",
                    "duplex" => "js_node_stream_duplex_subclass_init",
                    "transform" => "js_node_stream_transform_subclass_init",
                    _ => unreachable!("node stream parent kind {}", kind),
                };
                ctx.block().call(
                    DOUBLE,
                    runtime_fn,
                    &[(DOUBLE, &obj_box), (DOUBLE, &opts_box)],
                );
                found_inherited_ctor = true;
            }
        }
        // #5137 / #6325 / #6326: implicit-ctor subclass of a native base whose
        // surface perry stamps onto the INSTANCE — `EventEmitter`, `Map`/`Set`,
        // `Event`/`CustomEvent`. The explicit-`super()` arm
        // (`expr/this_super_call.rs`) installs it when a constructor is written;
        // a class with no own constructor writes no `super()`, so the install
        // has to happen here or the instance is left bare (`class M extends Map
        // {}` → `m.set` is not a function).
        //
        // Keyed on the class CHAIN reaching the base rather than on a literal
        // `extends` name: an INDIRECT subclass names an intermediate USER class
        // (`class D extends B {}` with `class B extends EventEmitter {}`), so the
        // old one-level name test lost the base entirely. The walk stops at any
        // ancestor with a constructor — its `super()` does the install — so this
        // never double-initializes.
        //
        // Gated `!has_imported_ctor` so an imported class whose real ctor lives
        // in another module (commander's `Command`) still reaches the
        // imported-ctor fallback below and runs its real `super()`.
        if !found_inherited_ctor && !has_imported_ctor {
            if let Some(base) = crate::lower_call::native_instance_base_in_chain(ctx, class) {
                crate::lower_call::emit_native_instance_base_init(
                    ctx,
                    base,
                    &obj_box,
                    &lowered_args,
                );
                found_inherited_ctor = true;
            }
        }
        // The remaining native builtins require real exotic instances rather
        // than state stamped onto Perry's initially allocated plain object.
        // Invoke their [[Construct]] with this class as newTarget and replace
        // the derived `this` binding with the returned branded value.
        if !found_inherited_ctor && !has_imported_ctor {
            if let Some(parent) = class.extends_name.as_deref().filter(|name| {
                matches!(
                    *name,
                    "ArrayBuffer"
                        | "SharedArrayBuffer"
                        | "DataView"
                        | "Boolean"
                        | "Number"
                        | "String"
                        | "Date"
                        | "RegExp"
                        | "Function"
                        | "BigInt"
                        | "Symbol"
                        | "Object"
                        | "Int8Array"
                        | "Uint8Array"
                        | "Uint8ClampedArray"
                        | "Int16Array"
                        | "Uint16Array"
                        | "Int32Array"
                        | "Uint32Array"
                        | "Float32Array"
                        | "Float64Array"
                        | "BigInt64Array"
                        | "BigUint64Array"
                )
            }) {
                lowered_args = refresh_rooted_args(ctx, group)?;
                let (args_ptr, args_len) = lower_js_args_array(ctx, &lowered_args);
                let class_id = ctx
                    .class_ids
                    .get(class_name)
                    .copied()
                    .unwrap_or(0)
                    .to_string();
                let name_idx = ctx.strings.intern(parent);
                let entry = ctx.strings.entry(name_idx);
                let name_bytes = format!("@{}", entry.bytes_global);
                let name_len = entry.byte_len.to_string();
                let constructed = ctx.block().call(
                    DOUBLE,
                    "js_builtin_subclass_construct",
                    &[
                        (I32, &class_id),
                        (PTR, &name_bytes),
                        (I64, &name_len),
                        (PTR, &args_ptr),
                        (I64, &args_len),
                    ],
                );
                ctx.block().store(DOUBLE, &constructed, &this_slot);
                found_inherited_ctor = true;
            }
        }
        // Issue #573: if the parent walk reached an Error-like built-in
        // without finding any user-class constructor, synthesize the JS
        // spec default ctor `constructor(...args) { super(...args); }` —
        // i.e. forward the first arg to Error's initialization, which
        // sets `this.message` + `this.name`. Without this, `new MyError(
        // "hello")` returns an object with `.message` / `.name`
        // unset — the SIGABRT-on-property-read happens because the slot
        // index lookup misses and downstream NaN-box decode reads
        // garbage.
        //
        // Walk the chain to find the terminating Error-like name (so
        // `class A extends Error {}; class B extends A {}` also flows
        // through correctly). If found, set `this.message = args[0]`
        // and `this.name = <error_kind>` directly, mirroring the
        // SuperCall Error-like arm in expr.rs.
        //
        // BUT: if `class_name` is an imported stub with a cross-module
        // ctor with a real body/effect, defer to that path — the source
        // module's ctor body knows the real param order
        // (e.g. `constructor(public statusCode, msg)` where args[0] is
        // statusCode, not message). Running Error-init here would
        // assign the wrong arg to `message` and corrupt the instance.
        // When the imported ctor is a synthesized empty 0-param ctor for the
        // bare-extends-Error case, calling it is a no-op and we still need
        // Error-init to populate `this.message` / `this.name`.
        let imported_ctor_has_body_or_fields = ctx
            .imported_class_ctors
            .get(class_name)
            .map(|ctor| ctor.stops_constructor_walk())
            .unwrap_or(false);
        if !found_inherited_ctor && !imported_ctor_has_body_or_fields {
            // Trace the chain to find the first Error-like ancestor name.
            let mut error_kind: Option<String> = None;
            let mut cur = class.extends_name.clone();
            let mut depth = 0usize;
            while let Some(pname) = cur {
                if matches!(
                    pname.as_str(),
                    "Error"
                        | "TypeError"
                        | "RangeError"
                        | "ReferenceError"
                        | "SyntaxError"
                        | "URIError"
                        | "EvalError"
                        | "AggregateError"
                ) {
                    error_kind = Some(pname);
                    break;
                }
                cur = ctx
                    .classes
                    .get(pname.as_str())
                    .and_then(|c| c.extends_name.clone());
                depth += 1;
                if depth > 32 {
                    break;
                }
            }
            if let Some(kind) = error_kind {
                let this_slot_for_err = ctx.this_stack.last().cloned().unwrap_or_default();
                let blk = ctx.block();
                let this_box = blk.load(DOUBLE, &this_slot_for_err);
                let this_bits = blk.bitcast_double_to_i64(&this_box);
                let this_handle = blk.and(I64, &this_bits, POINTER_MASK_I64);
                if let Some(msg_val) = lowered_args.first() {
                    let key_idx = ctx.strings.intern("message");
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    // Spec: built-in Error sets `message` non-enumerable via
                    // DefinePropertyOrThrow (Test262 NativeError/*-message).
                    blk.call_void(
                        "js_object_set_field_by_name_nonenum",
                        &[(I64, &this_handle), (I64, &key_raw), (DOUBLE, msg_val)],
                    );
                }
                let name_idx = ctx.strings.intern("name");
                let name_handle_global = format!("@{}", ctx.strings.entry(name_idx).handle_global);
                let name_val_idx = ctx.strings.intern(&kind);
                let name_val_global = format!("@{}", ctx.strings.entry(name_val_idx).handle_global);
                let blk = ctx.block();
                let name_key_box = blk.load(DOUBLE, &name_handle_global);
                let name_key_bits = blk.bitcast_double_to_i64(&name_key_box);
                let name_key_raw = blk.and(I64, &name_key_bits, POINTER_MASK_I64);
                let name_val_box = blk.load(DOUBLE, &name_val_global);
                blk.call_void(
                    "js_object_set_field_by_name",
                    &[
                        (I64, &this_handle),
                        (I64, &name_key_raw),
                        (DOUBLE, &name_val_box),
                    ],
                );
                found_inherited_ctor = true; // skip the imported-ctor fallback below
            }
        }
        if let Some(runtime_fn) = builtin_parent_runtime {
            let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let opts = lowered_args
                .first()
                .cloned()
                .unwrap_or_else(|| undef_lit.clone());
            let this_box = ctx
                .this_stack
                .last()
                .cloned()
                .map(|slot| ctx.block().load(DOUBLE, &slot))
                .unwrap_or_else(|| undef_lit.clone());
            ctx.block()
                .call(DOUBLE, runtime_fn, &[(DOUBLE, &this_box), (DOUBLE, &opts)]);
            found_inherited_ctor = true;
        }
        if let Some(runtime_fn) = fetch_parent_runtime {
            let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let arg0 = lowered_args
                .first()
                .cloned()
                .unwrap_or_else(|| undef_lit.clone());
            let arg1 = lowered_args
                .get(1)
                .cloned()
                .unwrap_or_else(|| undef_lit.clone());
            let this_box = ctx
                .this_stack
                .last()
                .cloned()
                .map(|slot| ctx.block().load(DOUBLE, &slot))
                .unwrap_or_else(|| undef_lit.clone());
            ctx.block().call(
                DOUBLE,
                runtime_fn,
                &[(DOUBLE, &this_box), (DOUBLE, &arg0), (DOUBLE, &arg1)],
            );
            found_inherited_ctor = true;
        }
        if promise_parent_runtime {
            emit_promise_subclass_init(ctx, &lowered_args);
            found_inherited_ctor = true;
        }
        if usp_parent_runtime {
            let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let init = lowered_args
                .first()
                .cloned()
                .unwrap_or_else(|| undef_lit.clone());
            let this_box = ctx
                .this_stack
                .last()
                .cloned()
                .map(|slot| ctx.block().load(DOUBLE, &slot))
                .unwrap_or_else(|| undef_lit.clone());
            ctx.block().call(
                DOUBLE,
                "js_url_search_params_subclass_init",
                &[(DOUBLE, &this_box), (DOUBLE, &init)],
            );
            found_inherited_ctor = true;
        }
        // If no parent constructor was found (imported class with no
        // inlineable constructor body), call the cross-module constructor.
        // Refs #420: walk past empty-bodied ancestors with param_count==0
        // imports too — when `class PgSerial extends PgColumn extends Column`
        // and Column is imported with the real ctor body, lower_new for
        // PgSerial needs to dispatch to Column_constructor (forwarding the
        // ctor args). Without this walk, `new PgSerial(table, config)`
        // produced an empty object since none of the chain's bodies ran.
        // An imported identifier used as a local class's heritage carries both
        // `extends_name` (for static shape/field metadata) and `extends_expr`
        // (the authoritative runtime parent value). Do not also call the
        // imported constructor symbol here: its consumer-side metadata records
        // only the source class's own constructor arity, which is zero for a
        // default-derived constructor even though the emitted source symbol
        // forwards its ancestor's arguments. Calling it therefore runs the
        // parent once with missing arguments, then the dynamic-parent arm below
        // runs it again with the real arguments. Drizzle's
        // `MySqlInt extends MySqlColumnWithAutoIncrement` reaches
        // `MySqlColumn(table, config)` first as `(undefined, undefined)` and
        // throws while assigning `config.uniqueName`.
        //
        // Imported leaf classes (`new ImportedClass(...)`) still use their own
        // imported constructor symbol; only a LOCAL leaf whose parent is the
        // dynamic cross-module value defers to `js_fetch_or_value_super`.
        if !found_inherited_ctor && !defer_to_dynamic_parent {
            let lookup_class = class_name.to_string();
            let mut effective_class_name = lookup_class.clone();
            let mut effective_extends = class.extends_name.clone();
            loop {
                let has_effectful_ctor = ctx
                    .imported_class_ctors
                    .get(&effective_class_name)
                    .map(|ctor| ctor.stops_constructor_walk())
                    .unwrap_or(false);
                if has_effectful_ctor {
                    break;
                }
                // v0.5.759: stop walking ONLY for the leaf class (the user's
                // `new X(...)` target) when it has its own synthesized
                // imported_class_ctor symbol AND its stub has fields. The
                // synthesized ctor applies SelfOnly + forwards super(), so
                // it handles the leaf's field inits (arrow fields,
                // default-value fields). Skipping the walk on the LEAF
                // (effective == lookup) doesn't break the drizzle PgSerial
                // → PgColumn → Column chain because that walks past
                // intermediate empty-stub classes; only the leaf gets the
                // walk-stop. Refs #420 / #618 followup.
                if effective_class_name == lookup_class {
                    let leaf_has_synth_ctor =
                        ctx.imported_class_ctors.contains_key(&effective_class_name);
                    let leaf_has_fields = ctx
                        .classes
                        .get(&effective_class_name)
                        .map(|c| !c.fields.is_empty())
                        .unwrap_or(false);
                    if leaf_has_synth_ctor && leaf_has_fields {
                        break;
                    }
                }
                let Some(parent) = effective_extends.clone() else {
                    break;
                };
                let Some(parent_class) = ctx.classes.get(&parent).copied() else {
                    break;
                };
                effective_class_name = parent;
                effective_extends = parent_class.extends_name.clone();
            }
            if let Some(ctor) = ctx
                .imported_class_ctors
                .get(&effective_class_name)
                .cloned()
                .filter(|_| effective_class_name != lookup_class)
            {
                // Walked to an ancestor — call its ctor with this and forwarded args.
                // `...rest` ctors get the trailing args packed into one array
                // for the final slot (mirrors method_has_rest, #672).
                // Field initializers / an inlined constructor body were lowered
                // between the instance allocation and here, so refresh again.
                lowered_args = refresh_rooted_args(ctx, group)?;
                let marshalled = marshal_imported_ctor_args(ctx, &ctor, &lowered_args);
                let mut ctor_args: Vec<(crate::types::LlvmType, &str)> =
                    Vec::with_capacity(1 + marshalled.len());
                ctor_args.push((DOUBLE, &obj_box));
                let ctor_param_types: Vec<crate::types::LlvmType> = std::iter::once(DOUBLE)
                    .chain(marshalled.iter().map(|_| DOUBLE))
                    .collect();
                for la in &marshalled {
                    ctor_args.push((DOUBLE, la.as_str()));
                }
                // Walked to an ANCESTOR ctor: its return-override does not replace
                // the leaf instance, so discard the return value. Declared DOUBLE
                // to match the symbol's real signature (see codegen/mod.rs).
                ctx.pending_declares
                    .push((ctor.symbol.clone(), DOUBLE, ctor_param_types));
                // new.target cross-module: the imported ctor symbol is compiled
                // in its SOURCE module and reads `new.target` from the runtime
                // cell, NOT this module's codegen `new_target_stack` slot. Bind
                // the cell to the LEAF class ref around the call so an ancestor
                // ctor (e.g. Auth.js `AuthError`'s `this.type = new.target.type`)
                // sees the class being constructed instead of a stale/undefined
                // value. Without this, `new CredentialsSignin()` from another
                // chunk threw `Cannot read properties of undefined (reading
                // 'type')`, or silently set `type = undefined` → the auth error
                // was mis-categorized and the login redirect fell back to
                // `?error=Configuration`.
                let nt_ref = double_literal(f64::from_bits(new_target_bits));
                let nt_save = crate::rooting::new_target_save(ctx, &nt_ref);
                let _ = ctx.block().call(DOUBLE, &ctor.symbol, &ctor_args);
                crate::rooting::new_target_restore(ctx, &nt_save);
            } else if let Some(ctor) = ctx.imported_class_ctors.get(class_name).cloned() {
                // Pad missing optional args with TAG_UNDEFINED so the constructor
                // doesn't read garbage from stale registers, and pack the rest
                // slot into an array when the ctor's last param is `...rest`.
                // Field initializers / an inlined constructor body were lowered
                // between the instance allocation and here, so refresh again.
                lowered_args = refresh_rooted_args(ctx, group)?;
                let marshalled = marshal_imported_ctor_args(ctx, &ctor, &lowered_args);
                // Pass `this` as NaN-boxed double (same as compile_method's this_arg).
                let mut ctor_args: Vec<(crate::types::LlvmType, &str)> =
                    Vec::with_capacity(1 + marshalled.len());
                ctor_args.push((DOUBLE, &obj_box));
                let ctor_param_types: Vec<crate::types::LlvmType> = std::iter::once(DOUBLE)
                    .chain(marshalled.iter().map(|_| DOUBLE))
                    .collect();
                for la in &marshalled {
                    ctor_args.push((DOUBLE, la.as_str()));
                }
                // The standalone `<class>_constructor` symbol returns DOUBLE: the
                // value an explicit `return <obj/fn>` produced (ECMAScript ctor
                // return-override) or `undefined` for an ordinary ctor. Capture it
                // into `ctor_result_slot` so the return-override applied at the end
                // of `lower_new` honors it — chalk's `class Chalk { constructor(o){
                // return chalkFactory(o); } }` returns a FUNCTION, so `new Chalk(o)`
                // must yield that function, not the empty allocated instance
                // ("value is not a function" on `new Chalk(...).red(...)`).
                ctx.pending_declares
                    .push((ctor.symbol.clone(), DOUBLE, ctor_param_types));
                // new.target cross-module: bind the runtime cell to the leaf
                // class ref around the imported ctor call (see the ANCESTOR arm
                // above for why). This is the direct `new ImportedClass()` case.
                let nt_ref = double_literal(f64::from_bits(new_target_bits));
                let nt_save = crate::rooting::new_target_save(ctx, &nt_ref);
                let ctor_ret = ctx.block().call(DOUBLE, &ctor.symbol, &ctor_args);
                crate::rooting::new_target_restore(ctx, &nt_save);
                ctx.block().store(DOUBLE, &ctor_ret, &ctor_result_slot);
                found_inherited_ctor = true;
            }
        } // end !found_inherited_ctor

        // A no-own-ctor class whose parent is a DYNAMIC runtime value
        // (`class D extends <fn/value> {}`, captured as `extends_expr`) gets
        // an implicit default derived ctor `constructor(...args){ super(...args) }`.
        // The inline `new` path above only finds inherited ctors that live in
        // `ctx.classes` / `imported_class_ctors`; a parent that resolves to a
        // plain function value at runtime (zod 4's `$constructor` pattern, where
        // a class extends another `$constructor`-returned function) matches none
        // of those, so without this branch `super(...)` is never emitted and the
        // parent function body never runs on the new instance — its
        // `this.<field> = …` / `Object.defineProperty(this, …)` writes are lost,
        // and (when the parent function returns its own `this`) the derived
        // instance is left uninitialized. Mirrors the synthesized-default-ctor
        // dynamic-parent super in `codegen/method.rs` (the standalone-symbol
        // path) and the explicit `Expr::SuperCall` dynamic-parent arm in
        // `expr/this_super_call.rs`: resolve the decl-time-registered parent
        // value and dispatch it on `this` via `js_fetch_or_value_super`, which
        // binds IMPLICIT_THIS to the instance for the duration of the call.
        //
        // #5657: a native BUILTIN base (`class X extends ArrayBuffer / Map /
        // Promise / %TypedArray% / RegExp / Function / …`) is also captured as
        // `extends_expr` (a bare `ArrayBuffer` Ident doesn't resolve through
        // `lookup_class`), but its parent VALUE is a builtin constructor that
        // rejects being *called* as a plain function — `js_fetch_or_value_super`
        // would route it through `js_native_call_value`, throwing "X is not a
        // function" / "Constructor X requires 'new'". Perry can't give a subclass
        // instance the builtin's internal slots, so `super()` to such a base is a
        // best-effort no-op (the instance is already allocated with the correct
        // dynamic-parent prototype chain, so `instanceof` holds). Skip the
        // dispatch for those names — mirroring the identical guard the explicit
        // `Expr::SuperCall` arm already applies via `is_other_builtin_constructor_name`
        // (`expr/this_super_call.rs`). Request/Response/Error are deliberately NOT
        // in that set: they DO need the dispatch (native fetch-handle attach /
        // callable error thunk), so they keep running it. This is a fast-path
        // skip on the textual name; an ALIASED builtin parent (`const AB =
        // ArrayBuffer; class X extends AB {}`) whose `extends_name` isn't a known
        // builtin still emits the call, but the runtime backstops it by value —
        // `js_fetch_or_value_super` no-ops the same builtin set via
        // `is_uncallable_builtin_super_parent` (perry-runtime, kept in lockstep).
        let parent_is_uncallable_builtin = dynamic_parent_owner
            .as_deref()
            .and_then(|owner| ctx.classes.get(owner).copied())
            .and_then(|owner| owner.extends_name.as_deref())
            .map(crate::expr::is_other_builtin_constructor_name)
            .unwrap_or(false)
            // SharedArrayBuffer construction now returns a real branded
            // buffer and honors the subclass newTarget/prototype in the
            // runtime dispatcher. It must run rather than retaining Perry's
            // provisional plain-object receiver.
            && dynamic_parent_owner
                .as_deref()
                .and_then(|owner| ctx.classes.get(owner).copied())
                .and_then(|owner| owner.extends_name.as_deref())
                != Some("SharedArrayBuffer");
        if !found_inherited_ctor && dynamic_parent_owner.is_some() && !parent_is_uncallable_builtin
        {
            if let Some(cid) = dynamic_parent_owner
                .as_deref()
                .and_then(|owner| ctx.class_ids.get(owner))
                .copied()
                .filter(|c| *c != 0)
            {
                let parent_val = ctx.block().call(
                    DOUBLE,
                    "js_get_dynamic_parent_value",
                    &[(I32, &cid.to_string())],
                );
                // Same here: the dynamic-parent `super(...)` buffer is filled long
                // after the allocation, behind further lowering.
                lowered_args = refresh_rooted_args(ctx, group)?;
                let (args_ptr, args_len) = if lowered_args.is_empty() {
                    ("null".to_string(), "0".to_string())
                } else {
                    let buf_reg = ctx.func.alloca_entry_array(DOUBLE, lowered_args.len());
                    for (i, a_val) in lowered_args.iter().enumerate() {
                        let slot = ctx
                            .block()
                            .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                        ctx.block().store(DOUBLE, a_val, &slot);
                    }
                    let ptr_reg = ctx.block().next_reg();
                    ctx.block().emit_raw(format!(
                        "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                        ptr_reg,
                        lowered_args.len(),
                        buf_reg
                    ));
                    (ptr_reg, lowered_args.len().to_string())
                };
                // Bug #5587: in the no-own-ctor path, `this_stack` was never
                // pushed for this `new` call, so `last()` would return the
                // outer function's `this` (or undef at module scope). Use
                // `obj_box` — the freshly-allocated object — directly.
                let this_box = obj_box.clone();
                let parent_result = ctx.block().call(
                    DOUBLE,
                    "js_fetch_or_value_super",
                    &[
                        (DOUBLE, &parent_val),
                        (DOUBLE, &this_box),
                        (PTR, &args_ptr),
                        (I64, &args_len),
                    ],
                );
                // A function-valued base constructor can return a replacement
                // object (notably a Proxy).  The implicit derived constructor
                // binds that object as `this`; primitives retain the allocation.
                // Keep the rooted slot authoritative so field initialization and
                // the final `new` result both use the replacement.
                let current_this = ctx.block().load(DOUBLE, &this_slot);
                let effective_this = super::new_helpers::emit_ctor_return_override(
                    ctx,
                    &current_this,
                    &parent_result,
                    false,
                );
                ctx.block().store(DOUBLE, &effective_this, &this_slot);
            }
        }
    }

    // Now that the parent body chain has run (setting `this.config`, etc.),
    // apply the leaf class's own field initializers — they may reference
    // state set by the parent body. For the own-ctor case, this is handled
    // at the SuperCall site inside the body. For the no-own-ctor case and
    // for classes with no extends (already applied above), we skip here.
    // Refs #420 (drizzle's PgText.enumValues = this.config.enumValues).
    //
    // Issue #631-followup: also apply intermediate-class fields between
    // the inherited-ctor class (exclusive) and the leaf (inclusive). Per
    // ECMAScript spec, each default-ctor class's field initializers run
    // immediately after that class's super() call returns. For drizzle's
    // SQLiteInteger ← SQLiteBaseInteger ← SQLiteColumn ← Column chain,
    // SQLiteBaseInteger's `autoIncrement = this.config.autoIncrement`
    // must run AFTER Column's body sets `this.config`.
    // v0.5.758: skip the post-init re-apply when the cross-module imported
    // constructor handles fields itself (via compile_method's
    // is_constructor_method path applying SelfOnly internally). The
    // re-apply uses the STUB's fields (no inits → all Undefined), which
    // would overwrite the freshly-set values. This applies whether the
    // imported ctor is synthesized (no own body, just forwards
    // super + applies SelfOnly) or has an explicit body. Drizzle's
    // `BetterSQLiteSession` (explicit ctor) and arrow-field cross-
    // module classes are both load-bearing. Refs #420 / #618 followup.
    // `extends_expr` (dynamic-parent, e.g. zod 4's `$constructor`) classes also
    // need their own field initializers re-applied here — AFTER the parent body
    // ran via `js_fetch_or_value_super` above. ECMAScript runs derived-class
    // field initializers after `super()` returns; `has_extends` only covers
    // static `extends_name`, so include the `extends_expr` case (SelfOnly,
    // mirroring the explicit-`SuperCall` dynamic-parent arm in this_super_call.rs).
    if !has_own_ctor && (has_extends || class.extends_expr.is_some()) && !has_imported_ctor {
        if let Some(owner) = dynamic_parent_owner {
            apply_field_initializers_recursive(
                ctx,
                class_name,
                FieldInitMode::FromInclusive(owner),
            )?;
        } else if builtin_parent_runtime.is_some()
            || fetch_parent_runtime.is_some()
            || promise_parent_runtime
            || usp_parent_runtime
            || (class.extends_expr.is_some() && !has_extends)
        {
            apply_field_initializers_recursive(ctx, class_name, FieldInitMode::SelfOnly)?;
        } else if let Some(stop_at) = inherited_ctor_class {
            apply_field_initializers_recursive(
                ctx,
                class_name,
                FieldInitMode::BetweenExclusiveTo(stop_at),
            )?;
        } else {
            apply_field_initializers_recursive(ctx, class_name, FieldInitMode::AfterRoot)?;
        }
    }
    // Close the inline constructor's control flow before emitting anything
    // that consumes the constructed receiver.  An explicit `return` has
    // already terminated the body block and branched to `after_idx`; emitting
    // a receiver reload while that terminated block is still current only
    // manufactures an SSA name with no defining instruction (invalid LLVM).
    let inline_return = ctx.inline_ctor_return.pop();
    if let Some(ret) = inline_return.as_ref() {
        if !ctx.block().is_terminated() {
            ctx.block().br(&ret.after_label);
        }
        ctx.current_block = after_idx;
    }

    // #7154: same re-read as the standalone-symbol path above. The inlined
    // constructor body (field initializers, `super(...)`, nested `new`s) can
    // reach a back-edge poll, and the evacuating minor there relocates the
    // instance out from under `obj_handle`/`obj_box`.
    let (obj_handle, obj_box) = if instance.protected {
        // `super()` is allowed to replace `this` (an ancestor constructor may
        // return an object).  The rooted this-slot is the authoritative value
        // after constructor execution; the allocation root still names the
        // original leaf allocation in that case.
        let boxed = ctx.block().load(DOUBLE, &this_slot);
        let bits = ctx.block().bitcast_double_to_i64(&boxed);
        let handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
        (handle, boxed)
    } else {
        reload_instance(ctx, group, &instance, &obj_handle, &obj_box)
    };
    emit_typed_shape_layout_init(ctx, class_name, &obj_handle);

    // Close the inline-constructor return: fall through (or branch) to the
    // shared after-block, then apply the spec return-override at construction
    // completion. `result_slot` holds the constructed `this` on fall-through
    // (initial value) or the raw value from an explicit `return`. The override
    // runs HERE (outside any `try` in the body) so a derived ctor's
    // `try { return <primitive>; } catch {}` still throws uncaught.
    let final_box = if let Some(ret) = inline_return {
        let raw = ctx.block().load(DOUBLE, &ret.result_slot);
        super::new_helpers::emit_ctor_return_override(ctx, &obj_box, &raw, ret.is_derived)
    } else {
        obj_box
    };

    ctx.new_target_stack.pop();
    ctx.this_stack.pop();
    ctx.class_stack.pop();
    Ok(final_box)
}
