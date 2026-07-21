//! `new ClassName(args…)` lowering.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. Holds `lower_new` (Phase C.1 constructor inlining).
//! The `FieldInitMode` enum and `apply_field_initializers_recursive` live
//! in the sibling `field_init` module.

use anyhow::Result;
use perry_hir::Expr;
use perry_types::Type as HirType;

use super::field_init::{apply_field_initializers_recursive, FieldInitMode};
use super::lower_builtin_new;
use super::new_ctor_args::{
    bind_inline_constructor_params, call_local_constructor_symbol, lower_constructor_arg,
    marshal_imported_ctor_args, restore_inline_constructor_scope, CaptureFill,
};
use super::new_helpers::{
    collect_decl_local_ids, ctor_body_calls_super, ctor_body_closure_calls_super,
    ctor_body_has_value_return, ctor_body_uses_this, ctor_chain_uses_new_target,
    emit_promise_subclass_init, local_constructor_symbol_exists, node_stream_parent_kind,
};
use crate::expr::{lower_expr, lower_js_args_array, nanbox_pointer_inline, FnCtx};
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::types::{DOUBLE, I32, I64, I8, PTR};

/// Emit the `js_gc_init_typed_shape_layout` call that registers the freshly
/// constructed instance's raw-f64 / pointer slot masks with the GC so the
/// typed-feedback class-field fast path engages. Must run AFTER the constructor
/// body has set the declared fields to their numeric values (the runtime
/// validates each raw-f64 slot currently holds a plain double before promoting).
/// No-op for classes without an inline-keys shape global. Refs the standalone
/// `<class>_constructor` symbol path, which previously returned before reaching
/// this — leaving every numeric class field permanently on the by-name hashmap
/// fallback (10M `counter.increment()` ran ~640ns/call instead of slot-direct).
fn emit_typed_shape_layout_init(ctx: &mut FnCtx<'_>, class_name: &str, obj_handle: &str) {
    let Some(keys_global_name) = ctx.class_keys_globals.get(class_name).cloned() else {
        return;
    };
    // Refs #5094: prefer the prefix-disambiguated chain so slot/word counts
    // agree with the mask globals emitted in compile_module (same-named
    // cross-module parents mis-resolve in the name-keyed walk).
    let typed_layout = ctx
        .class_init_chains
        .get(class_name)
        .map(|chain| crate::typed_shape::class_typed_layout_from_chain(chain))
        .unwrap_or_else(|| crate::typed_shape::class_typed_layout(ctx.classes, class_name));
    let slot_count_str = typed_layout.slot_count.to_string();
    let raw_mask_word_count_str = typed_layout.raw_f64_mask_words.len().to_string();
    let pointer_mask_word_count_str = typed_layout.pointer_mask_words.len().to_string();
    let raw_mask_ref = if typed_layout.raw_f64_mask_words.is_empty() {
        "null".to_string()
    } else {
        format!(
            "@{}",
            crate::typed_shape::raw_f64_mask_global_name_from_keys_global(&keys_global_name)
        )
    };
    let pointer_mask_ref = if typed_layout.pointer_mask_words.is_empty() {
        "null".to_string()
    } else {
        format!(
            "@{}",
            crate::typed_shape::mask_global_name_from_keys_global(&keys_global_name)
        )
    };
    ctx.block().call_void(
        "js_gc_init_typed_shape_layout",
        &[
            (I64, obj_handle),
            (I32, &slot_count_str),
            (PTR, &raw_mask_ref),
            (I32, &raw_mask_word_count_str),
            (PTR, &pointer_mask_ref),
            (I32, &pointer_mask_word_count_str),
        ],
    );
}

pub(crate) use super::capture_writeback::emit_class_capture_writeback;

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

fn lower_new_impl(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &[Expr],
    caps_absent_from_args: bool,
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
                let output = if let Some(first) = args.first() {
                    lower_expr(ctx, first)?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let options = if let Some(second) = args.get(1) {
                    lower_expr(ctx, second)?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                for extra in args.iter().skip(2) {
                    let _ = lower_expr(ctx, extra)?;
                }
                ctx.pending_declares.push((
                    "js_readline_promises_readline_new".to_string(),
                    DOUBLE,
                    vec![DOUBLE, DOUBLE],
                ));
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_readline_promises_readline_new",
                    &[(DOUBLE, &output), (DOUBLE, &options)],
                ));
            }
        }
        if let Some(val) = lower_builtin_new(ctx, class_name, args)? {
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
                if let Some(val) = lower_builtin_new(ctx, &original, args)? {
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
                let func_double = lower_expr(
                    ctx,
                    &Expr::ExternFuncRef {
                        name: class_name.to_string(),
                        param_types: Vec::new(),
                        return_type: HirType::Any,
                    },
                )?;
                let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
                for a in args {
                    lowered_args.push(lower_constructor_arg(ctx, a)?);
                }
                let (args_ptr, args_len) = lower_js_args_array(ctx, &lowered_args);
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
                let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
                for a in args {
                    lowered_args.push(lower_constructor_arg(ctx, a)?);
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
    let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
    for a in args {
        lowered_args.push(lower_constructor_arg(ctx, a)?);
    }

    // Compute total field count including inherited parent fields.
    // The runtime allocates at least 8 inline slots regardless, so this
    // mostly matters for shapes >8 fields.
    let mut field_count = class.fields.len() as u32;
    // Imported classes now carry their real field_names from the source
    // module. If the field count is still 0 (no fields info available),
    // use a generous default as a safety net.
    if field_count == 0 && class.constructor.is_none() {
        field_count = 32;
    }
    let mut parent = class.extends_name.as_deref();
    while let Some(parent_name) = parent {
        if let Some(p) = ctx.classes.get(parent_name).copied() {
            field_count += p.fields.len() as u32;
            parent = p.extends_name.as_deref();
        } else {
            break;
        }
    }
    // Issue #26 / #321: prefer the authoritative per-class field count computed
    // by the source-prefix-disambiguated keys-global builder. The walk above
    // resolves parents via `ctx.classes` — a name-keyed map that holds only
    // ONE same-named stub — so when a cross-module parent name collides
    // (effect's `Type` in SchemaAST.ts vs ParseResult.ts) it counts the wrong
    // parent's fields. Using the keys-global's count keeps the allocated slot
    // count and the header `field_count` in lockstep with the keys array,
    // which `Object.keys()` walks. Falls back to the computed walk when this
    // class has no keys global (anonymous / no-keys path).
    if let Some(&authoritative) = ctx.class_field_counts.get(class_name) {
        field_count = authoritative;
    }

    // Allocate the object with the per-class id and (if applicable)
    // parent class id, so the runtime registers the inheritance
    // chain for instanceof / virtual dispatch lookups.
    //
    // Use `js_object_alloc_class_with_keys`, which pre-populates the
    // `keys_array` with the class's field names in declaration order
    // (parent fields first, walking from the deepest ancestor down,
    // then own fields). This is REQUIRED so the LLVM PropertyGet/Set
    // fast path's slot indices match the runtime's by-name dispatch
    // (which walks `keys_array`). Mixing the two access patterns on
    // the same object — e.g. constructor writes via the fast path,
    // PropertyUpdate reads via the runtime helper — only produces
    // consistent results when both agree on the slot mapping.
    //
    // The packed-keys constant is interned via the StringPool. Two
    // classes with the same field-name set + order share one constant.
    let cid = ctx.class_ids.get(class_name).copied().unwrap_or(0);
    let parent_cid = class
        .extends_name
        .as_deref()
        .and_then(|p| ctx.class_ids.get(p).copied())
        .unwrap_or(0);
    let cid_str = cid.to_string();
    let parent_cid_str = parent_cid.to_string();
    let n_str = field_count.to_string();

    // Fast path: if the class has a per-class keys global (built once
    // at module init via `js_build_class_keys_array`), emit INLINE
    // bump-allocator IR — no function call into the runtime at all on
    // the hot path. The runtime exposes a `InlineArenaState` struct
    // (data ptr at offset 0, current bump offset at offset 8, current
    // block size at offset 16) via `js_inline_arena_state()`. We call
    // that ONCE per JS function entry (cached in `arena_state_slot`)
    // and then emit a 5-instruction bump check + GcHeader/ObjectHeader
    // store sequence at every `new ClassName()` site. The slow path
    // (block overflow) calls `js_inline_arena_slow_alloc` which syncs
    // the inline state back to the underlying arena, allocates a new
    // block, and updates the inline state.
    //
    // Cycles per inlined alloc on the M-series fast path:
    //    load offset       (1)
    //    add+and align     (2)
    //    add new_offset    (1)
    //    load size + cmp   (2)
    //    cond br           (predicted, 0)
    //    store offset      (1)
    //    load data + gep   (2)
    //    write GcHeader    (1)  — packed i64 store
    //    write ObjectHeader×2 (2) — packed i64 stores
    //    write keys_ptr    (1)
    //  total: ~13 cycles vs ~140 cycles for the function-call path.
    //
    // Layout assumption: GcHeader is 8 bytes
    //    {obj_type:u8, gc_flags:u8, _reserved:u16, size:u32}
    // and ObjectHeader is 24 bytes
    //    {object_type:u32, class_id:u32, parent_class_id:u32,
    //     field_count:u32, keys_array:*ptr}
    // followed by `max(field_count, 8)` 8-byte field slots. The user
    // pointer the rest of the codegen sees is `raw + 8` (i.e. the
    // ObjectHeader address) — same as what
    // `js_object_alloc_class_inline_keys` returns.
    //
    // Layout constants are duplicated here from the runtime; if
    // `GcHeader` or `ObjectHeader` ever change in
    // `crates/perry-runtime/src/{gc,object}.rs`, update both sides.
    let obj_handle = if class.extends_expr.is_some() {
        // Wall 45: dynamic-parent subclass (`class X extends _mod.default`).
        // The parent's field layout is unknown at this compile time (the
        // `extends` target is an unresolvable cross-module value, so the
        // parent-chain walk above contributed 0 fields and `field_count` /
        // `packed_keys` cover only X's OWN fields). Allocating with that
        // own-only layout under-sizes and mis-lays-out the instance: the
        // parent's constructor and inherited methods address the inherited
        // fields at the PARENT's slot indices (parent fields first), which fall
        // past X's own slots → OOB heap reads (captures read as garbage).
        // Route to `js_object_alloc_class_dynamic_parent`, which resolves the
        // runtime-registered parent edge + keys-array (both established at
        // module init by `js_register_class_parent_dynamic` /
        // `js_build_class_keys_array`, before any `new X()`) and allocates with
        // the merged `[parent keys..] ++ [own keys..]` layout. Bypasses the
        // inline bump-alloc fast path (which would bake the wrong layout).
        let mut packed_keys = String::new();
        for f in &class.fields {
            if f.key_expr.is_some() {
                continue;
            }
            packed_keys.push_str(&f.name);
            packed_keys.push('\0');
        }
        let keys_idx = ctx.strings.intern(&packed_keys);
        let keys_entry = ctx.strings.entry(keys_idx);
        let keys_global = format!("@{}", keys_entry.bytes_global);
        let keys_len_str = keys_entry.byte_len.to_string();
        ctx.block().call(
            I64,
            "js_object_alloc_class_dynamic_parent",
            &[
                (I32, &cid_str),
                (I32, &n_str),
                (PTR, &keys_global),
                (I32, &keys_len_str),
            ],
        )
    } else if let Some(keys_global_name) = ctx.class_keys_globals.get(class_name).cloned() {
        if std::env::var_os("PERRY_INLINE_NEW").is_none() {
            // [#bloat] Default: outline the per-`new`-site allocator. Opt back
            // into the old inline bump-allocator with PERRY_INLINE_NEW=1.
            // Measured win-win vs inline: −45 IR lines/site AND ~17% faster on an
            // 8M-allocation loop (the inline bump bloated the hot loop, hurting
            // icache/regalloc more than the saved call). Outline the per-`new`-site
            // inline bump-allocator (~145 lines of per-class-constant IR) into a
            // single call to the runtime `js_object_alloc_class_inline_keys`,
            // which performs the identical bump alloc + header init + slot
            // zero-fill and returns the same user pointer (as i64). Cuts ~145 IR
            // lines per `new` site to ~3. Only the per-class keys-array global is
            // loaded (cached per function, same as the inline path).
            let keys_slot = if let Some(s) = ctx.class_keys_slots.get(class_name).cloned() {
                s
            } else {
                let s = ctx.func.entry_init_load_global(&keys_global_name, I64);
                ctx.class_keys_slots
                    .insert(class_name.to_string(), s.clone());
                s
            };
            let keys_ptr = ctx.block().load(I64, &keys_slot);
            ctx.pending_declares.push((
                "js_object_alloc_class_inline_keys".to_string(),
                I64,
                vec![I32, I32, I32, I64],
            ));
            ctx.block().call(
                I64,
                "js_object_alloc_class_inline_keys",
                &[
                    (I32, &cid_str),
                    (I32, &parent_cid_str),
                    (I32, &field_count.to_string()),
                    (I64, &keys_ptr),
                ],
            )
        } else {
            // Compile-time layout constants.
            const GC_HEADER_SIZE: u64 = 8;
            // arm64_32 watchOS: `size_of::<ObjectHeader>()` is 24 on 64-bit but
            // 20 on ILP32 (4-byte `keys_array` pointer). Derive from the target
            // triple so the inline alloc size and field-region base match the
            // target-compiled runtime (no-op on 64-bit; see `target_layout`).
            let object_header_size: u64 =
                crate::target_layout::object_header_size_bytes(ctx.target_triple);
            const FIELD_SLOT_SIZE: u64 = 8;
            // Inline-slot floor — MUST match perry-runtime `object::INLINE_SLOT_FLOOR`
            // (they independently pad `new` objects to the same minimum; a mismatch
            // where codegen allocs fewer slots than the runtime's get/set bound-check
            // assumes is heap corruption). Lowered 8->4 to shrink small-object footprint.
            const MIN_FIELD_SLOTS: u64 = 4;
            const GC_TYPE_OBJECT: u64 = 2;
            const GC_FLAG_ARENA: u64 = 0x02;
            // PR #1146: pointer-free hint for inline-allocated regular
            // objects. The field-store sites issue per-slot
            // `js_gc_note_slot_layout` so the GC sees real pointer-bearing
            // slots regardless of this initial tag.
            const GC_LAYOUT_POINTER_FREE: u64 = 0x4000;
            const OBJECT_TYPE_REGULAR: u64 = 1;

            let alloc_field_count = std::cmp::max(field_count as u64, MIN_FIELD_SLOTS);
            let payload_size = object_header_size + alloc_field_count * FIELD_SLOT_SIZE;
            // Round the whole allocation up to FIELD_SLOT_SIZE (8). The inline
            // bump allocator's offset invariant (below) requires every
            // allocation to be a multiple of 8; on ILP32 `object_header_size`
            // is 20, so an unpadded total is 4-skewed (e.g. 92) and would
            // misalign the next bump. No-op on 64-bit (8 + 24 + 8·n is already
            // 8-aligned → 96 for ≤8 fields).
            let total_size = (GC_HEADER_SIZE + payload_size).next_multiple_of(FIELD_SLOT_SIZE);
            let total_size_str = total_size.to_string();

            // Lazy: allocate the per-function arena-state slot on the
            // first `new` we see. The slot init (`call @js_inline_arena_state`
            // + store) lives in the entry block via `entry_init_call_ptr`,
            // so it dominates every reachable use.
            let arena_state_slot = if let Some(slot) = ctx.arena_state_slot.clone() {
                slot
            } else {
                let slot = ctx.func.entry_init_call_ptr("js_inline_arena_state");
                ctx.arena_state_slot = Some(slot.clone());
                slot
            };

            // Hoist the per-class `keys_array` global load to the function
            // entry block (cached in a stack slot per class). Without this
            // hoisting, LLVM would reload `@perry_class_keys_<class>` on
            // every loop iteration, because the loop body's `call
            // @js_inline_arena_slow_alloc` blocks LICM — LLVM can't prove
            // the call doesn't modify the global.
            let keys_slot = if let Some(s) = ctx.class_keys_slots.get(class_name).cloned() {
                s
            } else {
                let s = ctx.func.entry_init_load_global(&keys_global_name, I64);
                ctx.class_keys_slots
                    .insert(class_name.to_string(), s.clone());
                s
            };
            let keys_ptr = ctx.block().load(I64, &keys_slot);

            // Inline bump-allocator IR.
            let blk = ctx.block();
            let state_ptr = blk.load(PTR, &arena_state_slot);

            // offset = state.offset (at byte offset 8 in InlineArenaState).
            // The offset is invariant 8-aligned: arena blocks start at offset 0
            // (8-aligned), every allocation is a multiple of 8 (`total_size`
            // includes the 8-byte GcHeader and `MIN_FIELD_SLOTS=4` slots ×
            // 8 bytes), and `js_inline_arena_slow_alloc` only ever swings the
            // state to `block.offset` which is also always 8-aligned. So we
            // skip the `(offset + 7) & -8` align-up step entirely — saves
            // 2 instructions per iter on the hot path.
            let offset_field_ptr = blk.gep(I8, &state_ptr, &[(I64, "8")]);
            let offset_val = blk.load(I64, &offset_field_ptr);
            let aligned_off = offset_val.clone();

            // new_offset = aligned + total_size
            let new_offset = blk.add(I64, &aligned_off, &total_size_str);

            // size = state.size (at byte offset 16)
            let size_field_ptr = blk.gep(I8, &state_ptr, &[(I64, "16")]);
            let size_val = blk.load(I64, &size_field_ptr);

            // fits = new_offset <= size
            let fits = blk.icmp_ule(I64, &new_offset, &size_val);

            // Set up fast/slow/merge basic blocks.
            let fast_idx = ctx.new_block("alloc.fast");
            let slow_idx = ctx.new_block("alloc.slow");
            let merge_idx = ctx.new_block("alloc.merge");
            let fast_label = ctx.block_label(fast_idx);
            let slow_label = ctx.block_label(slow_idx);
            let merge_label = ctx.block_label(merge_idx);

            ctx.block().cond_br(&fits, &fast_label, &slow_label);

            // ---- Fast path: bump and return data + aligned ----
            ctx.current_block = fast_idx;
            let blk = ctx.block();
            // GC_STORE_AUDIT(INIT): inline arena bump offset is allocator metadata, not a JS heap edge.
            blk.store(I64, &new_offset, &offset_field_ptr);
            // data ptr is at byte offset 0 in InlineArenaState
            let data_ptr = blk.load(PTR, &state_ptr);
            let raw_fast = blk.gep(I8, &data_ptr, &[(I64, &aligned_off)]);
            let fast_pred_label = blk.label.clone();
            blk.br(&merge_label);

            // ---- Slow path: call into the runtime ----
            ctx.current_block = slow_idx;
            let raw_slow = ctx.block().call(
                PTR,
                "js_inline_arena_slow_alloc",
                &[(PTR, &state_ptr), (I64, &total_size_str), (I64, "8")],
            );
            let slow_pred_label = ctx.block().label.clone();
            ctx.block().br(&merge_label);

            // ---- Merge: phi the raw pointer, write headers, NaN-box ----
            ctx.current_block = merge_idx;
            let blk = ctx.block();
            let raw = blk.phi(
                PTR,
                &[(&raw_fast, &fast_pred_label), (&raw_slow, &slow_pred_label)],
            );

            // Write GcHeader (8 bytes) as a single i64 store. Field
            // packing (little-endian):
            //   bits  0..7   = obj_type (u8)
            //   bits  8..15  = gc_flags (u8)
            //   bits 16..31  = _reserved (u16)
            //   bits 32..63  = size (u32)
            let gc_packed: u64 = GC_TYPE_OBJECT
                | (GC_FLAG_ARENA << 8)
                | (GC_LAYOUT_POINTER_FREE << 16)
                | ((total_size as u64) << 32);
            // GC_STORE_AUDIT(INIT): inline headers initialize freshly allocated unpublished object storage.
            blk.store(I64, &gc_packed.to_string(), &raw);

            // Write ObjectHeader at raw + 8.
            // First 8 bytes: object_type (u32, low) | class_id (u32, high)
            let oh_addr_1 = blk.gep(I8, &raw, &[(I64, "8")]);
            let oh_word_1: u64 = OBJECT_TYPE_REGULAR | ((cid as u64) << 32);
            blk.store(I64, &oh_word_1.to_string(), &oh_addr_1);

            // Second 8 bytes: parent_class_id (u32, low) | field_count (u32, high)
            let oh_addr_2 = blk.gep(I8, &raw, &[(I64, "16")]);
            let oh_word_2: u64 = (parent_cid as u64) | ((field_count as u64) << 32);
            blk.store(I64, &oh_word_2.to_string(), &oh_addr_2);

            // Third 8 bytes: keys_array pointer. The keys_ptr we loaded
            // above is an i64 (carries the ArrayHeader address); store as
            // i64 since the underlying memory is 8 bytes either way.
            let oh_addr_3 = blk.gep(I8, &raw, &[(I64, "24")]);
            // GC_STORE_AUDIT(INIT): keys_array edge is installed before publishing the new object.
            blk.store(I64, &keys_ptr, &oh_addr_3);

            // PerryTS/perry#4717: zero-fill the field slots with `undefined`, mirroring
            // `js_object_alloc_with_parent` (runtime object/alloc.rs), which deliberately
            // initializes ALL `max(field_count, 8)` slots "to prevent stale data from
            // previously freed GC objects from bleeding through." This inline bump path
            // wrote only the headers and left the slots uninitialized, so a field
            // read-before-write — or a GC that scans the still-constructing instance —
            // observed stale arena bytes. When those bytes were a previously-freed
            // `undefined`/pointer (e.g. `marked`'s `this.defaults`), the constructor
            // crashed with "Cannot read properties of undefined". Slots start at
            // raw + GcHeader(8) + ObjectHeader(24) = raw + 32.
            for i in 0..alloc_field_count {
                let slot_off = GC_HEADER_SIZE + object_header_size + i * FIELD_SLOT_SIZE;
                let slot_ptr = blk.gep(I8, &raw, &[(I64, &slot_off.to_string())]);
                // GC_STORE_AUDIT(INIT): freshly allocated inline object slot initialized to undefined.
                blk.store(I64, crate::nanbox::TAG_UNDEFINED_I64, &slot_ptr);
            }

            // User pointer = raw + 8 (the ObjectHeader address — what the
            // function-call path returned). Convert to i64 to match what
            // the existing nanbox_pointer_inline expects.
            let user_ptr = blk.gep(I8, &raw, &[(I64, "8")]);
            blk.ptrtoint(&user_ptr, I64)
        }
    } else {
        // Fallback: build the packed-keys string at this site and
        // call the slower SHAPE_CACHE-aware allocator. Used when the
        // class isn't in `class_keys_globals` (e.g. anonymous /
        // synthetic classes that compile_module doesn't pre-emit a
        // global for).
        let mut packed_keys = String::new();
        let mut parent_chain: Vec<&perry_hir::Class> = Vec::new();
        let mut p = class.extends_name.as_deref();
        while let Some(parent_name) = p {
            if let Some(pc) = ctx.classes.get(parent_name).copied() {
                parent_chain.push(pc);
                p = pc.extends_name.as_deref();
            } else {
                break;
            }
        }
        // Skip computed-key fields: their key is an expression evaluated at
        // construction time, not a stable string, so they don't get an inline
        // slot. The runtime stores them via IndexSet → js_object_set_field /
        // js_object_set_symbol_property paths in `apply_field_initializers_recursive`.
        // Including their synthetic `__computed_field_*` names in packed_keys
        // would surface them as enumerable own properties on Object.keys().
        for pc in parent_chain.iter().rev() {
            for f in &pc.fields {
                if f.key_expr.is_some() {
                    continue;
                }
                packed_keys.push_str(&f.name);
                packed_keys.push('\0');
            }
        }
        for f in &class.fields {
            if f.key_expr.is_some() {
                continue;
            }
            packed_keys.push_str(&f.name);
            packed_keys.push('\0');
        }
        let keys_idx = ctx.strings.intern(&packed_keys);
        let keys_entry = ctx.strings.entry(keys_idx);
        let keys_global = format!("@{}", keys_entry.bytes_global);
        let keys_len_str = keys_entry.byte_len.to_string();

        ctx.block().call(
            I64,
            "js_object_alloc_class_with_keys",
            &[
                (I32, &cid_str),
                (I32, &parent_cid_str),
                (I32, &n_str),
                (PTR, &keys_global),
                (I32, &keys_len_str),
            ],
        )
    };
    let obj_box = nanbox_pointer_inline(ctx.block(), &obj_handle);

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
        && local_constructor_symbol_exists(ctx, class);
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
        let saved_new_target = if ctor_chain_uses_new_target(ctx, class) {
            ctx.class_ids.get(class_name).map(|&cid| {
                let prev = ctx.block().call(DOUBLE, "js_new_target_get", &[]);
                let class_ref = double_literal(f64::from_bits(
                    crate::nanbox::INT32_TAG | (cid as u64 & 0xFFFF_FFFF),
                ));
                ctx.block()
                    .call(DOUBLE, "js_new_target_set", &[(DOUBLE, &class_ref)]);
                prev
            })
        } else {
            None
        };
        if let Some(ctor_ret) = call_local_constructor_symbol(
            ctx,
            class,
            &obj_box,
            &lowered_args,
            caps_absent_from_args,
        ) {
            if let Some(prev) = &saved_new_target {
                ctx.block()
                    .call(DOUBLE, "js_new_target_set", &[(DOUBLE, prev)]);
            }
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
            let is_derived_lit = if is_derived { "1" } else { "0" };
            let final_box = ctx.block().call(
                DOUBLE,
                "js_ctor_return_override",
                &[
                    (DOUBLE, &obj_box),
                    (DOUBLE, &ctor_ret),
                    (crate::types::I32, is_derived_lit),
                ],
            );
            return Ok(final_box);
        }
        if let Some(prev) = &saved_new_target {
            ctx.block()
                .call(DOUBLE, "js_new_target_set", &[(DOUBLE, prev)]);
        }
        return Ok(obj_box);
    }

    // Allocate a `this` slot and store the new object there. The
    // slot lives on this_stack for the duration of the inlined ctor
    // body (which may span many basic blocks and contain nested
    // closures that capture `this`), so hoist to the entry block for
    // dominance safety.
    let this_slot = ctx.func.alloca_entry(DOUBLE);
    ctx.block().store(DOUBLE, &obj_box, &this_slot);
    ctx.this_stack.push(this_slot);
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
    // function. `ctor_result_slot` starts as `this`; `Stmt::Return`
    // overwrites it with a returned object (or throws for a derived ctor
    // returning a primitive), then branches to `after_idx`. Refs
    // class/subclass/derived-class-return-override-*.
    let ctor_result_slot = ctx.func.alloca_entry(DOUBLE);
    ctx.block().store(DOUBLE, &obj_box, &ctor_result_slot);
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
        bind_inline_constructor_params(ctx, &ctor.params, &lowered_args, ctor_capture_fill)
    });

    if let Some(stop_at) = inherited_ctor_class.clone() {
        apply_field_initializers_recursive(ctx, class_name, FieldInitMode::UpToInclusive(stop_at))?;
    } else {
        apply_field_initializers_recursive(ctx, class_name, FieldInitMode::AncestorsOnly)?;
    }
    if !has_extends {
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
            // Lower the constructor body. Errors propagate.
            crate::stmt::lower_stmts(ctx, &class.constructor.as_ref().unwrap().body)?;
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
                        parent_capture_fill,
                    );

                    // Push the parent class name so `this` inside the
                    // parent ctor body resolves field names via the
                    // parent's field list.
                    ctx.class_stack.pop();
                    ctx.class_stack.push(pname.to_string());

                    crate::stmt::lower_stmts(ctx, &parent_ctor.body)?;

                    // Restore class_stack to the child.
                    ctx.class_stack.pop();
                    ctx.class_stack.push(class_name.to_string());

                    restore_inline_constructor_scope(ctx, saved_scope);

                    // Apply the field initializers of every class BELOW the
                    // inherited-ctor class — the leaf and any intermediates —
                    // now that the parent ctor body has run (the post-super()
                    // step, mirroring the own-ctor path's SelfOnly-after). The
                    // up-front pass above used `UpToInclusive(inherited)`, which
                    // keeps `chain[0..=idx(inherited)]` and therefore EXCLUDES
                    // the leaf, so without this a no-own-ctor subclass's own
                    // field initializers never ran — e.g. zod's
                    // `class ZodObject extends ZodType { private _cached = null }`
                    // left `_cached` at the raw-0 slot, so `_getCached()`'s
                    // `this._cached !== null` was true (0 !== null) and returned
                    // 0; `_parse` then destructured `{ keys }` off 0, iterated
                    // nothing, and every `z.object({...}).parse()` dropped all
                    // fields.
                    apply_field_initializers_recursive(
                        ctx,
                        class_name,
                        FieldInitMode::BetweenExclusiveTo(pname.to_string()),
                    )?;

                    found_inherited_ctor = true;
                    break; // Found and inlined the parent ctor.
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
        if !found_inherited_ctor {
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
                let nt_prev = ctx.block().call(DOUBLE, "js_new_target_get", &[]);
                let nt_ref = double_literal(f64::from_bits(new_target_bits));
                ctx.block()
                    .call(DOUBLE, "js_new_target_set", &[(DOUBLE, &nt_ref)]);
                let _ = ctx.block().call(DOUBLE, &ctor.symbol, &ctor_args);
                ctx.block()
                    .call(DOUBLE, "js_new_target_set", &[(DOUBLE, &nt_prev)]);
            } else if let Some(ctor) = ctx.imported_class_ctors.get(class_name).cloned() {
                // Pad missing optional args with TAG_UNDEFINED so the constructor
                // doesn't read garbage from stale registers, and pack the rest
                // slot into an array when the ctor's last param is `...rest`.
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
                let nt_prev = ctx.block().call(DOUBLE, "js_new_target_get", &[]);
                let nt_ref = double_literal(f64::from_bits(new_target_bits));
                ctx.block()
                    .call(DOUBLE, "js_new_target_set", &[(DOUBLE, &nt_ref)]);
                let ctor_ret = ctx.block().call(DOUBLE, &ctor.symbol, &ctor_args);
                ctx.block()
                    .call(DOUBLE, "js_new_target_set", &[(DOUBLE, &nt_prev)]);
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
        let parent_is_uncallable_builtin = class
            .extends_name
            .as_deref()
            .map(crate::expr::is_other_builtin_constructor_name)
            .unwrap_or(false);
        if !found_inherited_ctor && class.extends_expr.is_some() && !parent_is_uncallable_builtin {
            if let Some(cid) = ctx.class_ids.get(class_name).copied().filter(|c| *c != 0) {
                let parent_val = ctx.block().call(
                    DOUBLE,
                    "js_get_dynamic_parent_value",
                    &[(I32, &cid.to_string())],
                );
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
                let _ = ctx.block().call(
                    DOUBLE,
                    "js_fetch_or_value_super",
                    &[
                        (DOUBLE, &parent_val),
                        (DOUBLE, &this_box),
                        (PTR, &args_ptr),
                        (I64, &args_len),
                    ],
                );
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
        if builtin_parent_runtime.is_some()
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
    emit_typed_shape_layout_init(ctx, class_name, &obj_handle);

    // Close the inline-constructor return: fall through (or branch) to the
    // shared after-block, then apply the spec return-override at construction
    // completion. `result_slot` holds the constructed `this` on fall-through
    // (initial value) or the raw value from an explicit `return`. The override
    // runs HERE (outside any `try` in the body) so a derived ctor's
    // `try { return <primitive>; } catch {}` still throws uncaught.
    let final_box = if let Some(ret) = ctx.inline_ctor_return.pop() {
        if !ctx.block().is_terminated() {
            ctx.block().br(&ret.after_label);
        }
        ctx.current_block = after_idx;
        let raw = ctx.block().load(DOUBLE, &ret.result_slot);
        let is_derived = if ret.is_derived { "1" } else { "0" };
        ctx.block().call(
            DOUBLE,
            "js_ctor_return_override",
            &[
                (DOUBLE, &obj_box),
                (DOUBLE, &raw),
                (crate::types::I32, is_derived),
            ],
        )
    } else {
        obj_box
    };

    ctx.new_target_stack.pop();
    ctx.this_stack.pop();
    ctx.class_stack.pop();
    Ok(final_box)
}
