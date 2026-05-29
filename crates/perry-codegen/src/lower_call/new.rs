//! `new ClassName(args…)` lowering + recursive field-initializer application.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. Holds `lower_new` (Phase C.1 constructor inlining),
//! the `FieldInitMode` enum, and `apply_field_initializers_recursive`.

use anyhow::Result;
use perry_hir::Expr;
use perry_types::Type as HirType;

use super::lower_builtin_new;
use crate::expr::{lower_expr, nanbox_pointer_inline, FnCtx};
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::types::{DOUBLE, I32, I64, I8, PTR};

fn node_stream_parent_kind(ctx: &FnCtx<'_>, class: &perry_hir::Class) -> Option<&'static str> {
    let mut cur = class.extends_name.as_deref();
    let mut depth = 0usize;
    while let Some(name) = cur {
        match name {
            "Readable" => return Some("readable"),
            "Duplex" => return Some("duplex"),
            _ => {}
        }
        if ctx.imported_class_ctors.contains_key(name) {
            return None;
        }
        let Some(parent) = ctx.classes.get(name).copied() else {
            return None;
        };
        if parent.constructor.is_some() {
            return None;
        }
        cur = parent.extends_name.as_deref();
        depth += 1;
        if depth > 32 {
            break;
        }
    }
    None
}

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
pub(crate) fn lower_new(ctx: &mut FnCtx<'_>, class_name: &str, args: &[Expr]) -> Result<String> {
    // Built-in Web classes that the runtime provides constructors for.
    // These are checked BEFORE the ctx.classes lookup because the user
    // code may shadow the name — if they do, the class lookup below
    // wins.
    if !ctx.classes.contains_key(class_name) {
        if let Some(val) = lower_builtin_new(ctx, class_name, args)? {
            return Ok(val);
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

    // Lower the args first (constructor params).
    let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
    for a in args {
        lowered_args.push(lower_expr(ctx, a)?);
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
    let obj_handle = if let Some(keys_global_name) = ctx.class_keys_globals.get(class_name).cloned()
    {
        // Compile-time layout constants.
        const GC_HEADER_SIZE: u64 = 8;
        const OBJECT_HEADER_SIZE: u64 = 24;
        const FIELD_SLOT_SIZE: u64 = 8;
        const MIN_FIELD_SLOTS: u64 = 8;
        const GC_TYPE_OBJECT: u64 = 2;
        const GC_FLAG_ARENA: u64 = 0x02;
        // PR #1146: pointer-free hint for inline-allocated regular
        // objects. The field-store sites issue per-slot
        // `js_gc_note_slot_layout` so the GC sees real pointer-bearing
        // slots regardless of this initial tag.
        const GC_LAYOUT_POINTER_FREE: u64 = 0x4000;
        const OBJECT_TYPE_REGULAR: u64 = 1;

        let alloc_field_count = std::cmp::max(field_count as u64, MIN_FIELD_SLOTS);
        let payload_size = OBJECT_HEADER_SIZE + alloc_field_count * FIELD_SLOT_SIZE;
        let total_size = GC_HEADER_SIZE + payload_size; // e.g. 96 for any class with ≤8 fields
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
        // includes the 8-byte GcHeader and `MIN_FIELD_SLOTS=8` slots ×
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
        blk.store(I64, &keys_ptr, &oh_addr_3);

        // User pointer = raw + 8 (the ObjectHeader address — what the
        // function-call path returned). Convert to i64 to match what
        // the existing nanbox_pointer_inline expects.
        let user_ptr = blk.gep(I8, &raw, &[(I64, "8")]);
        blk.ptrtoint(&user_ptr, I64)
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

    // Allocate a `this` slot and store the new object there. The
    // slot lives on this_stack for the duration of the inlined ctor
    // body (which may span many basic blocks and contain nested
    // closures that capture `this`), so hoist to the entry block for
    // dominance safety.
    let this_slot = ctx.func.alloca_entry(DOUBLE);
    ctx.block().store(DOUBLE, &obj_box, &this_slot);
    ctx.this_stack.push(this_slot);
    ctx.class_stack.push(class_name.to_string());

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
            _ => None,
        }
    } else {
        None
    };
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
    let mut saved_locals_for_ctor: Option<std::collections::HashMap<u32, String>> = None;
    let mut saved_local_types_for_ctor: Option<std::collections::HashMap<u32, HirType>> = None;
    if let Some(ctor) = &class.constructor {
        saved_locals_for_ctor = Some(ctx.locals.clone());
        saved_local_types_for_ctor = Some(ctx.local_types.clone());
        for (param, arg_val) in ctor.params.iter().zip(lowered_args.iter()) {
            let slot = ctx.func.alloca_entry(DOUBLE);
            ctx.block().store(DOUBLE, arg_val, &slot);
            ctx.locals.insert(param.id, slot);
            ctx.local_types.insert(param.id, param.ty.clone());
        }
    }

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
        // Lower the constructor body. Errors propagate.
        crate::stmt::lower_stmts(ctx, &class.constructor.as_ref().unwrap().body)?;

        // Restore the enclosing function's local scope.
        ctx.locals = saved_locals_for_ctor.take().unwrap_or_default();
        ctx.local_types = saved_local_types_for_ctor.take().unwrap_or_default();
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
                    let saved_locals = ctx.locals.clone();
                    let saved_local_types = ctx.local_types.clone();

                    // Map constructor params from the parent's ctor to
                    // the supplied args. If caller passed fewer args
                    // than the parent expects, extra params get
                    // undefined.
                    for (i, param) in parent_ctor.params.iter().enumerate() {
                        // Parent-ctor params become ctx.locals for the
                        // inlined body; capturable by nested closures,
                        // so hoist to the entry block.
                        let slot = ctx.func.alloca_entry(DOUBLE);
                        if i < lowered_args.len() {
                            ctx.block().store(DOUBLE, &lowered_args[i], &slot);
                        } else {
                            let undef = crate::nanbox::double_literal(f64::from_bits(
                                crate::nanbox::TAG_UNDEFINED,
                            ));
                            ctx.block().store(DOUBLE, &undef, &slot);
                        }
                        ctx.locals.insert(param.id, slot);
                        ctx.local_types.insert(param.id, param.ty.clone());
                    }

                    // Push the parent class name so `this` inside the
                    // parent ctor body resolves field names via the
                    // parent's field list.
                    ctx.class_stack.pop();
                    ctx.class_stack.push(pname.to_string());

                    crate::stmt::lower_stmts(ctx, &parent_ctor.body)?;

                    // Restore class_stack to the child.
                    ctx.class_stack.pop();
                    ctx.class_stack.push(class_name.to_string());

                    ctx.locals = saved_locals;
                    ctx.local_types = saved_local_types;
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
        // ctor that has REAL params, defer to that path — the source
        // module's ctor body knows the real param order
        // (e.g. `constructor(public statusCode, msg)` where args[0] is
        // statusCode, not message). Running Error-init here would
        // assign the wrong arg to `message` and corrupt the instance.
        // When the imported ctor's param_count is 0, the source had no
        // own ctor (codegen synthesized an empty 0-param ctor for the
        // bare-extends-Error case), so calling it is a no-op and we
        // still need Error-init to populate `this.message` / `this.name`.
        let imported_ctor_has_real_params = ctx
            .imported_class_ctors
            .get(class_name)
            .map(|(_, n)| *n > 0)
            .unwrap_or(false);
        if !found_inherited_ctor && !imported_ctor_has_real_params {
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
                    blk.call_void(
                        "js_object_set_field_by_name",
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
        if builtin_parent_runtime.is_some() {
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
                let has_real_ctor = ctx
                    .imported_class_ctors
                    .get(&effective_class_name)
                    .map(|(_, n)| *n > 0)
                    .unwrap_or(false);
                if has_real_ctor {
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
            if let Some((ctor_name, param_count)) = ctx
                .imported_class_ctors
                .get(&effective_class_name)
                .cloned()
                .filter(|(_, _)| effective_class_name != lookup_class)
            {
                // Walked to an ancestor — call its ctor with this and forwarded args.
                let undef_lit =
                    crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                while lowered_args.len() < param_count {
                    lowered_args.push(undef_lit.clone());
                }
                let mut ctor_args: Vec<(crate::types::LlvmType, &str)> =
                    Vec::with_capacity(1 + lowered_args.len());
                ctor_args.push((DOUBLE, &obj_box));
                let ctor_param_types: Vec<crate::types::LlvmType> = std::iter::once(DOUBLE)
                    .chain(lowered_args.iter().map(|_| DOUBLE))
                    .collect();
                for la in &lowered_args {
                    ctor_args.push((DOUBLE, la.as_str()));
                }
                ctx.pending_declares.push((
                    ctor_name.clone(),
                    crate::types::VOID,
                    ctor_param_types,
                ));
                ctx.block().call_void(&ctor_name, &ctor_args);
            } else if let Some((ctor_name, param_count)) =
                ctx.imported_class_ctors.get(class_name).cloned()
            {
                // Pad missing optional args with TAG_UNDEFINED so the constructor
                // doesn't read garbage from stale registers.
                let undef_lit =
                    crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                while lowered_args.len() < param_count {
                    lowered_args.push(undef_lit.clone());
                }
                // Pass `this` as NaN-boxed double (same as compile_method's this_arg).
                let mut ctor_args: Vec<(crate::types::LlvmType, &str)> =
                    Vec::with_capacity(1 + lowered_args.len());
                ctor_args.push((DOUBLE, &obj_box));
                let ctor_param_types: Vec<crate::types::LlvmType> = std::iter::once(DOUBLE)
                    .chain(lowered_args.iter().map(|_| DOUBLE))
                    .collect();
                for la in &lowered_args {
                    ctor_args.push((DOUBLE, la.as_str()));
                }
                ctx.pending_declares.push((
                    ctor_name.clone(),
                    crate::types::VOID,
                    ctor_param_types,
                ));
                ctx.block().call_void(&ctor_name, &ctor_args);
            }
        } // end !found_inherited_ctor
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
    if !has_own_ctor && has_extends && !has_imported_ctor {
        if let Some(stop_at) = inherited_ctor_class {
            apply_field_initializers_recursive(
                ctx,
                class_name,
                FieldInitMode::BetweenExclusiveTo(stop_at),
            )?;
        } else {
            apply_field_initializers_recursive(ctx, class_name, FieldInitMode::AfterRoot)?;
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
    }

    if let Some(keys_global_name) = ctx.class_keys_globals.get(class_name).cloned() {
        let typed_layout = crate::typed_shape::class_typed_layout(ctx.classes, class_name);
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
                (I64, &obj_handle),
                (I32, &slot_count_str),
                (PTR, &raw_mask_ref),
                (I32, &raw_mask_word_count_str),
                (PTR, &pointer_mask_ref),
                (I32, &pointer_mask_word_count_str),
            ],
        );
    }

    ctx.this_stack.pop();
    ctx.class_stack.pop();
    Ok(obj_box)
}

/// Walk the inheritance chain from the root down and apply each class's
/// field initializers to `this`. Call this inside `lower_new` after the
/// `this` slot is pushed but before the constructor body is inlined.
///
/// Initializers run in declaration order: root parent first, then each
/// child, matching JavaScript / TypeScript class semantics where fields
/// are initialized before user-written constructor code executes (field
/// initializers are conceptually prepended to the constructor body).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FieldInitMode {
    /// Apply field initializers for the entire chain root → leaf.
    All,
    /// Apply only the ancestors' field initializers (skip the leaf class).
    /// Used to set up parent fields before a parent ctor body runs.
    AncestorsOnly,
    /// Apply only the named class's own field initializers (skip ancestors).
    /// Used after a parent ctor body has run to install the leaf's fields,
    /// which may reference state set by the parent body (e.g.
    /// `enumValues = this.config.enumValues` in drizzle's PgText). Refs #420.
    SelfOnly,
    /// Issue #631-followup: apply fields for the chain root → `stop_at`
    /// (inclusive). Used in the no-own-ctor path BEFORE the inherited-
    /// ctor body runs, so only the inherited-ctor class's chain has its
    /// fields set up. Intermediate classes between `stop_at` and the leaf
    /// (e.g. SQLiteBaseInteger between SQLiteColumn and SQLiteInteger)
    /// have their fields applied AFTER the inherited-ctor body, via
    /// `BetweenExclusiveTo`.
    UpToInclusive(String),
    /// Apply fields for chain (`stop_at` exclusive) → leaf (inclusive).
    /// Mirror of `UpToInclusive` for the post-body chain. Skips
    /// `stop_at` itself because that class's SelfOnly fields are
    /// applied via the SuperCall site inside the inlined body.
    BetweenExclusiveTo(String),
    /// Apply every class after the root ancestor through the leaf. Used
    /// when a default-derived constructor chain has no explicit inherited
    /// constructor body, so there is no SuperCall site to apply intermediate
    /// class fields.
    AfterRoot,
}

pub(crate) fn apply_field_initializers_recursive(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    mode: FieldInitMode,
) -> Result<()> {
    // Issue #26 / #321: prefer the authoritative, source-prefix-disambiguated
    // ancestor chain (built once in `compile_module` alongside the per-class
    // keys global). Walking `ctx.classes` by `extends_name` mis-resolves
    // same-named cross-module parents (effect's `Type` in SchemaAST.ts vs
    // ParseResult.ts) and writes that wrong parent's fields onto the instance
    // as `undefined`, surfacing as spurious enumerable keys (`_tag,ast,actual,
    // message` on a `PropertySignature`). The authoritative chain is root →
    // leaf and carries each ancestor's resolved fields, so we use both its
    // ORDER (for the mode filter) and its FIELDS (per class below).
    let mut chain_field_override: std::collections::HashMap<String, Vec<perry_hir::ClassField>> =
        std::collections::HashMap::new();
    // Collect the inheritance chain from root down.
    let mut chain: Vec<String> = Vec::new();
    if let Some(auth) = ctx.class_init_chains.get(class_name) {
        for (name, fields) in auth {
            chain.push(name.clone());
            chain_field_override.insert(name.clone(), fields.clone());
        }
    } else {
        let mut cur = Some(class_name.to_string());
        while let Some(c) = cur {
            let Some(class) = ctx.classes.get(&c).copied() else {
                break;
            };
            chain.push(c.clone());
            cur = class.extends_name.clone();
        }
        chain.reverse();
    }

    // Apply mode filter:
    //   All: keep entire chain
    //   AncestorsOnly: drop the leaf (last entry)
    //   SelfOnly: keep only the leaf
    //   UpToInclusive(stop_at): keep chain[0..=index_of(stop_at)]
    //   BetweenExclusiveTo(stop_at): keep chain[index_of(stop_at)+1..]
    //   AfterRoot: keep chain[1..]
    let chain: Vec<String> = match &mode {
        FieldInitMode::All => chain,
        FieldInitMode::AncestorsOnly => {
            // Issue #631-followup: keep only the ROOT class's fields.
            // Per ECMAScript spec, derived-class field initializers run
            // AFTER super() returns (so they may depend on parent body
            // state, e.g. drizzle's `class SQLiteBaseInteger extends
            // SQLiteColumn { autoIncrement = this.config.autoIncrement }`
            // — `this.config` is set by Column's body two levels up).
            // Pre-#631 this kept all-ancestors-but-leaf which incorrectly
            // ran SQLiteBaseInteger's init before Column's body.
            //
            // Each intermediate class's fields are applied via the
            // SuperCall site (`expr.rs::Expr::SuperCall`'s post-body
            // intermediate-walk added in this commit). Root's fields
            // need to be applied here because root has no super() and
            // its body may reference its own fields directly.
            if chain.len() <= 1 {
                Vec::new()
            } else {
                vec![chain[0].clone()]
            }
        }
        FieldInitMode::SelfOnly => {
            if let Some(last) = chain.last().cloned() {
                vec![last]
            } else {
                Vec::new()
            }
        }
        FieldInitMode::UpToInclusive(stop_at) => {
            if let Some(idx) = chain.iter().position(|n| n == stop_at) {
                chain[..=idx].to_vec()
            } else {
                Vec::new()
            }
        }
        FieldInitMode::BetweenExclusiveTo(stop_at) => {
            if let Some(idx) = chain.iter().position(|n| n == stop_at) {
                if idx + 1 < chain.len() {
                    chain[idx + 1..].to_vec()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        FieldInitMode::AfterRoot => {
            if chain.len() > 1 {
                chain[1..].to_vec()
            } else {
                Vec::new()
            }
        }
    };

    for class_name_in_chain in chain {
        // Issue #26: prefer the authoritative chain's resolved fields for this
        // class (correct cross-module parent layout); fall back to the
        // name-keyed `ctx.classes` only when no authoritative entry exists.
        // Local classes carry their real init exprs here; imported/inherited
        // fields carry `init: None` (→ `undefined`), exactly as before — just
        // resolved against the RIGHT parent.
        let class_fields: Vec<perry_hir::ClassField> =
            if let Some(fields) = chain_field_override.get(&class_name_in_chain) {
                fields.clone()
            } else {
                match ctx.classes.get(&class_name_in_chain).copied() {
                    Some(c) => c.fields.clone(),
                    None => continue,
                }
            };
        // Collect (property_name, init_expr) pairs up-front to avoid
        // holding an immutable borrow of ctx.classes across lower_expr.
        // Computed-key fields (`[Symbol.for("k")]` etc.) live in a parallel
        // list since their key is an expression that needs runtime evaluation.
        //
        // Fields declared without an initializer (`#x;` / `x: any;`) must
        // still be written in the constructor as `undefined` — JS semantics
        // is `new C().x === undefined`, not zero-bytes from the allocator.
        // Without the explicit write, regular methods see `undefined` (the
        // field-by-name dispatcher returns undefined for absent fields),
        // but arrow-class-field bodies that load `this.x` through the
        // captured-this slot read raw zero bytes — `0 ?? fallback` then
        // takes the wrong branch (0 is falsy but not nullish), breaking
        // common patterns like `this.#preparedHeaders ?? new Headers()`
        // in hono's Context. Lower the missing-init case to
        // `Expr::Undefined` so the constructor writes the spec-correct
        // value into the field slot. Refs #486.
        let mut init_pairs: Vec<(String, Expr)> = Vec::new();
        let mut init_pairs_computed: Vec<(Expr, Expr)> = Vec::new();
        for field in &class_fields {
            let init = match &field.init {
                Some(e) => e.clone(),
                None => Expr::Undefined,
            };
            match &field.key_expr {
                Some(key) => init_pairs_computed.push((key.clone(), init)),
                None => init_pairs.push((field.name.clone(), init)),
            }
        }
        if init_pairs.is_empty() && init_pairs_computed.is_empty() {
            continue;
        }

        // Temporarily swap class_stack so `this.field` in the init
        // resolves against the correct class.
        ctx.class_stack.push(class_name_in_chain.clone());
        for (prop, init_expr) in init_pairs {
            // Issue #263: arrow-function class fields like
            // `arrowField = () => this.value` need their reserved `this`
            // capture slot patched with the constructor's `this` AFTER
            // the closure is built — same pattern `lower_object_literal`
            // already uses for object-literal methods. Without this, the
            // arrow's body reads slot `auto_captures.len()` of the
            // closure's capture array (initialized to 0.0 by the
            // closure-build site at expr.rs:3294-3304), then `this.value`
            // dereferences address 0 and SIGSEGVs.
            if let Expr::Closure {
                params: cparams,
                body: cbody,
                captures: ccaps,
                captures_this: true,
                ..
            } = &init_expr
            {
                let auto_caps =
                    crate::type_analysis::compute_auto_captures(ctx, cparams, cbody, ccaps);
                let this_idx = auto_caps.len() as u32;

                // Lower the closure expression to a NaN-boxed pointer.
                let closure_val = lower_expr(ctx, &init_expr)?;

                // Read the current `this` from the constructor's this_stack.
                let this_val = if let Some(slot) = ctx.this_stack.last().cloned() {
                    ctx.block().load(DOUBLE, &slot)
                } else {
                    double_literal(0.0)
                };

                // Patch the closure's reserved this-slot in-place, then
                // store the closure as the field via the runtime FFI.
                let blk = ctx.block();
                let bits = blk.bitcast_double_to_i64(&closure_val);
                let closure_handle = blk.and(I64, &bits, POINTER_MASK_I64);
                let idx_str = this_idx.to_string();
                blk.call_void(
                    "js_closure_set_capture_f64",
                    &[(I64, &closure_handle), (I32, &idx_str), (DOUBLE, &this_val)],
                );

                // Now store the patched closure as the field. Emit the
                // property-write call directly, mirroring PropertySet's
                // codegen path (expr.rs:2559+) — we can't go through
                // `lower_expr` again because that would re-lower the
                // closure expression and produce a fresh, unpatched
                // closure pointer.
                let key_idx = ctx.strings.intern(&prop);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let blk = ctx.block();
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                let this_bits = blk.bitcast_double_to_i64(&this_val);
                let this_raw = blk.and(I64, &this_bits, POINTER_MASK_I64);
                blk.call_void(
                    "js_object_set_field_by_name",
                    &[(I64, &this_raw), (I64, &key_raw), (DOUBLE, &closure_val)],
                );
                continue;
            }

            // Non-closure (or non-this-capturing closure) initializer:
            // build a PropertySet { this, prop, init_expr } and lower
            // through the existing path.
            let set_expr = Expr::PropertySet {
                object: Box::new(Expr::This),
                property: prop,
                value: Box::new(init_expr),
            };
            let _ = lower_expr(ctx, &set_expr)?;
        }

        // Computed-key fields: `[Parent.Symbol.X] = init` lowers to
        // `this[Parent.Symbol.X] = init`. The key expression is evaluated
        // at construction time per ES spec — `Object.defineProperty(this, k, …)`
        // semantics through the IndexSet path. arrow-with-this-capture is
        // unusual on a computed-key field; if it ever surfaces in real code
        // we extend this branch the same way the string-keyed loop above
        // does.
        for (key_expr, init_expr) in init_pairs_computed {
            let set_expr = Expr::IndexSet {
                object: Box::new(Expr::This),
                index: Box::new(key_expr),
                value: Box::new(init_expr),
            };
            let _ = lower_expr(ctx, &set_expr)?;
        }
        ctx.class_stack.pop();
    }
    Ok(())
}
