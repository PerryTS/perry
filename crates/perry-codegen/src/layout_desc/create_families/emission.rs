//! Reading a family back out as code: the per-statement hooks (`before_stmt`,
//! `after_stmt`), the holder-read lowering, the layout/contract/chain-proof
//! emission, `lower_create`/`lower_add`/`lower_binding`/`lower_assign`, and the
//! image descriptor list `emission()` returns. Deriving the facts is `collect`'s
//! job; this module only turns a decided family into IR.
use super::*;

/// Statement lowering, before `stmts[i]`: a `const a = Object.create(P)`
/// whose family the statements after it make is the next creation point.
pub(crate) fn before_stmt(ctx: &mut FnCtx<'_>, stmts: &[Stmt], i: usize) {
    ctx.create_families.pending = None;
    let Stmt::Let {
        id,
        mutable: false,
        init: Some(Expr::ObjectCreate(proto, None)),
        ..
    } = &stmts[i]
    else {
        return;
    };
    let Some(p) = proto_binding(proto) else {
        return;
    };
    let family = MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        binding_family(&m, *id, p, &stmts[i + 1..])
    });
    ctx.create_families.pending = family.map(|f| (*id, f));
}

/// Statement lowering, after a statement: a creation point not lowered by
/// [`lower_create`] makes no family.
pub(crate) fn after_stmt(ctx: &mut FnCtx<'_>) {
    ctx.create_families.pending = None;
}

/// Record that lowered code uses `site`'s layout.
pub(super) fn use_layout(site: Site) {
    MODULE_CREATE_FAMILIES.with(|m| {
        m.borrow_mut()
            .used
            .entry(site.desc_global.clone())
            .or_insert(site);
    });
}

/// The proof of a [`HolderRead`] for the object pointer `biased` (biased
/// as `receiver_range` makes it): branches to `miss_l` unless it holds, and
/// returns the holder slot's value bits (`i64`, never the hole).
pub(crate) fn emit_holder_load(
    ctx: &mut FnCtx<'_>,
    read: &HolderRead,
    biased: &str,
    miss_l: &str,
) -> String {
    use crate::expr::receiver_range::emit_field_ptr;
    let header =
        crate::target_layout::object_slot_base_bytes(ctx.target_triple, read.form()) as i64;
    let ids: Vec<String> = read.states.iter().map(|s| layout_id(ctx, s)).collect();
    let chain_ids: Vec<String> = read.chain.iter().map(|(_, d)| layout_id(ctx, d)).collect();
    let mut ok = "false".to_string();
    {
        let blk = ctx.block();
        let wp = emit_field_ptr(blk, biased, 4);
        let word = blk.load(I32, &wp);
        for id in &ids {
            let registered = blk.icmp_ne(I32, id, "0");
            let same = blk.icmp_eq(I32, &word, id);
            let held = blk.and(crate::types::I1, &registered, &same);
            ok = blk.or(crate::types::I1, &ok, &held);
        }
        let mp = emit_field_ptr(blk, biased, header - 8);
        let meta = blk.load(PTR, &mp);
        let no_meta = blk.icmp_eq(PTR, &meta, "null");
        ok = blk.and(crate::types::I1, &ok, &no_meta);
    }
    let mut holder = String::new();
    for ((h, _), id) in read.chain.iter().zip(&chain_ids) {
        let global = ctx.module_globals[h].clone();
        let next = ctx.new_block("cholder.image");
        let next_l = ctx.block_label(next);
        ctx.block().cond_br(&ok, &next_l, miss_l);
        ctx.current_block = next;
        let blk = ctx.block();
        let value = blk.load(DOUBLE, &format!("@{global}"));
        let bits = blk.bitcast_double_to_i64(&value);
        let handle = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        let ptr = blk.inttoptr(I64, &handle);
        let word_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
        let word = blk.load(I32, &word_ptr);
        let registered = blk.icmp_ne(I32, id, "0");
        let same = blk.icmp_eq(I32, &word, id);
        ok = blk.and(crate::types::I1, &registered, &same);
        holder = ptr;
    }
    let hit = ctx.new_block("cholder.image_hit");
    let hit_l = ctx.block_label(hit);
    ctx.block().cond_br(&ok, &hit_l, miss_l);
    ctx.current_block = hit;
    let blk = ctx.block();
    let offset = (header + 8 * i64::from(read.slot)).to_string();
    let sp = blk.gep(I8, &holder, &[(I64, &offset)]);
    let v = blk.load(I64, &sp);
    let hole = blk.icmp_eq(I64, &v, crate::nanbox::TAG_HOLE_I64);
    let value_idx = ctx.new_block("cholder.image_value");
    let value_l = ctx.block_label(value_idx);
    ctx.block().cond_br(&hole, miss_l, &value_l);
    ctx.current_block = value_idx;
    v
}

/// `object.property` through [`holder_read`] (`expr/dispatch.rs`), else
/// `None`. Any receiver the proof refuses takes the ordinary read.
pub(crate) fn lower_holder_read(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    read: &Expr,
) -> Option<Result<String>> {
    let site = holder_read(ctx, object, property)?;
    Some((|| {
        let recv = crate::expr::lower_expr(ctx, object)?;
        let check = ctx.new_block("cholder.recv");
        let slow = ctx.new_block("cholder.generic");
        let merge = ctx.new_block("cholder.merge");
        let check_l = ctx.block_label(check);
        let slow_l = ctx.block_label(slow);
        let merge_l = ctx.block_label(merge);
        let biased = {
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(&recv);
            let fused = crate::expr::receiver_range::emit_fused_receiver_test(blk, &bits);
            blk.cond_br(&fused.is_object_pointer, &check_l, &slow_l);
            fused.biased
        };
        ctx.current_block = check;
        let v = emit_holder_load(ctx, &site, &biased, &slow_l);
        let hit = ctx.block().bitcast_i64_to_double(&v);
        let hit_end = ctx.block_label(ctx.current_block);
        ctx.block().br(&merge_l);
        ctx.current_block = slow;
        let generic = crate::expr::property_get::lower(ctx, read)?;
        let slow_end = ctx.block_label(ctx.current_block);
        ctx.block().br(&merge_l);
        ctx.current_block = merge;
        Ok(ctx
            .block()
            .phi(DOUBLE, &[(&hit, &hit_end), (&generic, &slow_end)]))
    })())
}


/// A carried prototype layout to emit: its descriptor, keys and
/// [[Prototype]] (`None`: `%Object.prototype%`; `Some(None)`: null;
/// `Some(Some(g))`: the `PerryProtoFamily` `g`).
pub(crate) struct ImageDesc {
    pub desc_global: String,
    pub entries: Vec<ImageEntry>,
    pub proto: Option<Option<String>>,
}

/// The prototype records, family descriptors and carried prototype layouts
/// to emit: every followed prototype an `Object.create` names, each layout
/// lowered code used, and the carried layout of each such prototype the
/// compiler can name exactly.
pub(crate) fn emission() -> (Vec<String>, Vec<Site>, Vec<ImageDesc>) {
    MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        let protos = m
            .bound
            .iter()
            .map(|id| format!("perry_protofam_{}_{id}", m.prefix))
            .collect();
        let images = m
            .bound
            .iter()
            .filter(|_| m.checked)
            .filter_map(|id| {
                let b = &m.borns[id];
                let entries = b.image.clone()?;
                let proto = match b.proto {
                    Link::Object => None,
                    Link::Null => Some(None),
                    Link::Binding(q) if m.bound.contains(&q) => {
                        Some(Some(format!("perry_protofam_{}_{q}", m.prefix)))
                    }
                    Link::Binding(_) => return None,
                };
                Some(ImageDesc {
                    desc_global: image_global(&m.prefix, *id),
                    entries,
                    proto,
                })
            })
            .collect();
        let sites = if crate::layout_ordinals::collecting() {
            m.candidates.values().cloned().collect()
        } else {
            m.used.values().cloned().collect()
        };
        (protos, sites, images)
    })
}

/// `Object.create(P)` for a binding's family: born on its first state (every
/// slot a hole) when `P` is the family's prototype, else the ordinary birth.
pub(crate) fn lower_create(ctx: &mut FnCtx<'_>, family: &Create, expr: &Expr) -> Result<String> {
    let Expr::ObjectCreate(proto, None) = expr else {
        anyhow::bail!("create family birth is not Object.create(P)");
    };
    let proto_box = crate::expr::lower_expr(ctx, proto)?;
    Ok(ctx.block().call(
        DOUBLE,
        "perry_create_family",
        &[
            (PTR, &format!("@{}", family.states[0])),
            (DOUBLE, &proto_box),
        ],
    ))
}

/// `PERRY_CHECK_CONTRACTS=1`: assert what a family store relied on.
fn emit_contract(ctx: &mut FnCtx<'_>, desc_global: &str, object: &str) {
    ctx.block().call_void(
        "js_create_family_contract_check",
        &[(PTR, &format!("@{desc_global}")), (DOUBLE, object)],
    );
}

/// The registered id of a carried family descriptor, loaded once per
/// function (written once at startup, before any user code).
fn layout_id(ctx: &mut FnCtx<'_>, desc_global: &str) -> String {
    let slot = ctx
        .func
        .entry_init_load_global(&super::super::layout_id_global(desc_global), I32);
    ctx.block().load(I32, &slot)
}

/// GcHeader `_reserved` bits that refuse a key add: frozen, sealed, not
/// extensible (runtime `gc::OBJ_FLAG_FROZEN | OBJ_FLAG_SEALED |
/// OBJ_FLAG_NO_EXTEND`).
const RECEIVER_INTEGRITY_BITS: &str = "7";

/// The checked proof of a family add (decision 150): `i1`, every prototype
/// of the chain still on its carried layout (its module global's layout
/// word equals the layout's id, which also refuses a shadowed word) and
/// `%Object.prototype%` unwritten. Emitted only after the receiver proved
/// its state, so each binding holds its prototype (the receiver was born of
/// it, after the chain's bindings were initialized).
fn emit_chain_proof(ctx: &mut FnCtx<'_>, family: &Create) -> String {
    let mut ok = "true".to_string();
    for (h, desc) in &family.chain {
        let Some(global) = ctx.module_globals.get(h).cloned() else {
            return "false".to_string();
        };
        let id = layout_id(ctx, desc);
        let blk = ctx.block();
        let value = blk.load(DOUBLE, &format!("@{global}"));
        let bits = blk.bitcast_double_to_i64(&value);
        let handle = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        let ptr = blk.inttoptr(I64, &handle);
        let word_ptr = blk.gep(I8, &ptr, &[(I64, "4")]);
        let word = blk.load(I32, &word_ptr);
        let registered = blk.icmp_ne(I32, &id, "0");
        let same = blk.icmp_eq(I32, &word, &id);
        let held = blk.and(crate::types::I1, &registered, &same);
        ok = blk.and(crate::types::I1, &ok, &held);
    }
    if family.object_root {
        let patched = crate::builtin_proto::emit_patched_test(
            ctx,
            crate::builtin_proto::BuiltinProto::Object,
        );
        let blk = ctx.block();
        let unpatched = blk.xor(crate::types::I1, &patched, "true");
        ok = blk.and(crate::types::I1, &ok, &unpatched);
    }
    ok
}

/// `a.k = v` for a binding family's add of `keys[index]`. A receiver in the
/// state before it (the add) or in the state holding it (an overwrite of its
/// own writable data), with no meta record (no overflow, no elements, no
/// recorded [[Prototype]]: its prototype is the family's) and not frozen,
/// sealed or made non-extensible, stores the slot and the holding state's
/// layout word, when the census proved the add free (no prototype is read)
/// or the chain's checked proof holds. Any other receiver takes the
/// ordinary `[[Set]]`. Returns the stored value.
pub(crate) fn lower_add(
    ctx: &mut FnCtx<'_>,
    add: &Add,
    object: &Expr,
    value: &Expr,
    strict: bool,
) -> Result<String> {
    let family = &add.family;
    let slot = add.index;
    let barrier_needed = !crate::expr::expr_produces_non_pointer_bits_by_construction(ctx, value);
    let addref_needed = crate::expr::class_field_store_needs_string_addref(ctx, value);
    rooting::with_operands_rooted(ctx, &[object, value], |ctx, vals| {
        let (recv_box, val_double) = (vals[0].clone(), vals[1].clone());
        let before = layout_id(ctx, &family.states[slot]);
        let after = layout_id(ctx, &family.states[slot + 1]);
        let check_idx = ctx.new_block("create_add.layout");
        let chain_idx = (!family.chain.is_empty()).then(|| ctx.new_block("create_add.chain"));
        let fast_idx = ctx.new_block("create_add.slot");
        let slow_idx = ctx.new_block("create_add.generic");
        let merge_idx = ctx.new_block("create_add.merge");
        let check_label = ctx.block_label(check_idx);
        let fast_label = ctx.block_label(fast_idx);
        let slow_label = ctx.block_label(slow_idx);
        let merge_label = ctx.block_label(merge_idx);
        let obj_bits = ctx.block().bitcast_double_to_i64(&recv_box);
        let recv = crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), &obj_bits);
        ctx.block()
            .cond_br(&recv.is_object_pointer, &check_label, &slow_label);

        ctx.current_block = check_idx;
        let header = crate::target_layout::object_slot_base_bytes(ctx.target_triple, add.form());
        let pointer = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
            4
        } else {
            8
        };
        let receiver_done = chain_idx.map_or(fast_label.clone(), |c| ctx.block_label(c));
        let (obj_handle, obj_ptr) = {
            let blk = ctx.block();
            let obj_handle = crate::expr::receiver_range::emit_handle(blk, &recv.biased);
            let obj_ptr = blk.inttoptr(I64, &obj_handle);
            let word_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
            let word = blk.load(I32, &word_ptr);
            let registered = blk.icmp_ne(I32, &after, "0");
            let adding = blk.icmp_eq(I32, &word, &before);
            let overwrite = blk.icmp_eq(I32, &word, &after);
            let either = blk.or(crate::types::I1, &adding, &overwrite);
            let meta_ptr = blk.gep(I8, &obj_ptr, &[(I64, &(header - pointer).to_string())]);
            let meta = blk.load(PTR, &meta_ptr);
            let no_meta = blk.icmp_eq(PTR, &meta, "null");
            let gc_ptr = blk.gep(
                I8,
                &obj_ptr,
                &[(I64, &(2 - perry_abi::GC_HEADER_SIZE as i64).to_string())],
            );
            let gc_bits = blk.load(crate::types::I16, &gc_ptr);
            let restricted = blk.and(crate::types::I16, &gc_bits, RECEIVER_INTEGRITY_BITS);
            let extensible = blk.icmp_eq(crate::types::I16, &restricted, "0");
            let ok = blk.and(crate::types::I1, &registered, &either);
            let ok = blk.and(crate::types::I1, &ok, &no_meta);
            let ok = blk.and(crate::types::I1, &ok, &extensible);
            blk.cond_br(&ok, &receiver_done, &slow_label);
            (obj_handle, obj_ptr)
        };

        if let Some(chain_idx) = chain_idx {
            ctx.current_block = chain_idx;
            let held = emit_chain_proof(ctx, family);
            ctx.block().cond_br(&held, &fast_label, &slow_label);
        }

        ctx.current_block = fast_idx;
        let (slot_ptr, slot_addr) = {
            let blk = ctx.block();
            let offset = (header + 8 * slot as u64).to_string();
            let slot_ptr = blk.gep(I8, &obj_ptr, &[(I64, &offset)]);
            let slot_addr = blk.ptrtoint(&slot_ptr, I64);
            (slot_ptr, slot_addr)
        };
        crate::expr::emit_jsvalue_slot_store_pointer_tested(
            ctx,
            &slot_ptr,
            &val_double,
            &obj_handle,
            addref_needed,
            &obj_bits,
            &slot_addr,
            barrier_needed,
            "family_set",
        );
        {
            let blk = ctx.block();
            let word_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
            // GC_STORE_AUDIT(POINTER_FREE): the layout word is a scalar.
            blk.store(I32, &after, &word_ptr);
        }
        if crate::codegen::helpers::check_contracts() {
            emit_contract(ctx, &family.states[slot + 1], &recv_box);
        }
        ctx.block().br(&merge_label);
        let fast_end = ctx.block_label(ctx.current_block);

        ctx.current_block = slow_idx;
        let key_idx = ctx.strings.intern(&family.keys[slot]);
        let handle = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let key_box = ctx.block().load(DOUBLE, &handle);
        let slow_val = ctx.block().call(
            DOUBLE,
            "js_put_value_set",
            &[
                (DOUBLE, &recv_box),
                (DOUBLE, &key_box),
                (DOUBLE, &val_double),
                (DOUBLE, &recv_box),
                (I32, if strict { "1" } else { "0" }),
            ],
        );
        ctx.block().br(&merge_label);
        let slow_end = ctx.block_label(ctx.current_block);

        ctx.current_block = merge_idx;
        Ok(ctx
            .block()
            .phi(DOUBLE, &[(&val_double, &fast_end), (&slow_val, &slow_end)]))
    })
}

/// `P`'s birth: after the top-level binding `id` is initialized, give the
/// object the family's prototype serial, and, when the compiler derived its
/// carried layout, let the runtime bear it on that layout (the binding then
/// holds the object born there; `stmt/mod.rs`).
pub(crate) fn lower_binding(ctx: &mut FnCtx<'_>, id: u32) -> Result<()> {
    let Some((family, image, locals)) = MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        m.bound.contains(&id).then(|| {
            let image = m.borns[&id].image.as_ref().filter(|_| m.checked);
            let locals: Vec<u32> = image
                .into_iter()
                .flatten()
                .filter_map(|e| match e.key {
                    ImageKey::Local(k) => Some(k),
                    _ => None,
                })
                .collect();
            (
                format!("perry_protofam_{}_{id}", m.prefix),
                image.map(|_| image_global(&m.prefix, id)),
                locals,
            )
        })
    }) else {
        return Ok(());
    };
    // Only a module global can take the image (an add reads it there).
    let global = ctx.module_globals.get(&id).cloned();
    let image = image.filter(|_| global.is_some());
    // The image's module-local symbol keys: their bindings' values, in key
    // order (each binding is initialized: `P`'s initializer read it).
    let mut symbols = Vec::new();
    if image.is_some() {
        for k in &locals {
            symbols.push(crate::expr::lower_expr(ctx, &Expr::LocalGet(*k))?);
        }
    }
    let read = Expr::LocalGet(id);
    let value = crate::expr::lower_expr(ctx, &read)?;
    let image_arg = image
        .as_ref()
        .map_or("null".to_string(), |d| format!("@{d}"));
    let symbols_arg = if symbols.is_empty() {
        "null".to_string()
    } else {
        // The call copies them before it allocates. In the entry block (#167).
        let buf = ctx.func.alloca_entry_array(DOUBLE, symbols.len());
        for (i, v) in symbols.iter().enumerate() {
            let blk = ctx.block();
            let at = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
            blk.store(DOUBLE, v, &at);
        }
        buf
    };
    let born = ctx.block().call(
        DOUBLE,
        "perry_bind_family_prototype",
        &[
            (PTR, &format!("@{family}")),
            (DOUBLE, &value),
            (PTR, &image_arg),
            (PTR, &symbols_arg),
            (I32, &symbols.len().to_string()),
        ],
    );
    if let (Some(_), Some(global)) = (image, global) {
        crate::expr::emit_gated_root_nanbox_store(ctx, &born, &format!("@{global}"));
    }
    Ok(())
}

/// `Object.assign(Object.create(P), { k1: e1, … })` for a family `site`.
///
/// Evaluation order is the operation's: `P`, the object's creation (which
/// throws for a `P` that is not an object or null), then `e1…en`, then the
/// adds. The family's object is born with every slot a hole and is not
/// observable before its stores; any other object gets the ordinary
/// `[[Set]]`s (`js_put_value_set`, strict, as `Object.assign` throws).
pub(crate) fn lower_assign(ctx: &mut FnCtx<'_>, site: &Site, expr: &Expr) -> Result<String> {
    let Expr::ObjectAssign { target, sources } = expr else {
        anyhow::bail!("create family site is not Object.assign");
    };
    let Expr::ObjectCreate(proto, None) = target.as_ref() else {
        anyhow::bail!("create family target is not Object.create(P)");
    };
    let Expr::New { args, .. } = &sources[0] else {
        anyhow::bail!("create family source is not a literal");
    };
    let n = args.len();
    rooting::with_rooted_group(ctx, n + 1, |ctx, group| {
        let proto_box = crate::expr::lower_expr(ctx, proto)?;
        let born = ctx.block().call(
            DOUBLE,
            "perry_create_family",
            &[
                (PTR, &format!("@{}", site.desc_global)),
                (DOUBLE, &proto_box),
            ],
        );
        let object = group.adopt_emitted(ctx, Repr::Boxed, &born, true);
        let mut operands = Vec::with_capacity(n);
        for arg in args {
            operands.push(group.lower(ctx, arg, true)?);
        }

        // The layout word tells the two births apart: the ordinary one is
        // keyless, the family's carries `k1…kn`.
        let object_box = group.reread_emitted(ctx, object);
        let id_global = super::super::layout_id_global(&site.desc_global);
        let id_slot = ctx.func.entry_init_load_global(&id_global, I32);
        let (obj_bits, obj_handle, obj_ptr, is_family) = {
            let blk = ctx.block();
            let id = blk.load(I32, &id_slot);
            let obj_bits = blk.bitcast_double_to_i64(&object_box);
            let obj_handle = blk.and(I64, &obj_bits, crate::nanbox::POINTER_MASK_I64);
            let obj_ptr = blk.inttoptr(I64, &obj_handle);
            let word_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
            let word = blk.load(I32, &word_ptr);
            let registered = blk.icmp_ne(I32, &id, "0");
            let same = blk.icmp_eq(I32, &word, &id);
            let is_family = blk.and(crate::types::I1, &registered, &same);
            (obj_bits, obj_handle, obj_ptr, is_family)
        };
        let family_idx = ctx.new_block("create_family.slots");
        let generic_idx = ctx.new_block("create_family.set");
        let merge_idx = ctx.new_block("create_family.merge");
        let family_label = ctx.block_label(family_idx);
        let generic_label = ctx.block_label(generic_idx);
        let merge_label = ctx.block_label(merge_idx);
        ctx.block()
            .cond_br(&is_family, &family_label, &generic_label);

        // The family's slots: nothing between the re-reads and the stores
        // collects, so the object and the values are current.
        ctx.current_block = family_idx;
        let header = crate::target_layout::object_slot_base_bytes(ctx.target_triple, site.form());
        for (slot, arg) in args.iter().enumerate() {
            let value = group.reread(ctx, operands[slot])?;
            let barrier_needed =
                !crate::expr::expr_produces_non_pointer_bits_by_construction(ctx, arg);
            let addref_needed = crate::expr::class_field_store_needs_string_addref(ctx, arg);
            let offset = (header + 8 * slot as u64).to_string();
            let (slot_ptr, slot_addr) = {
                let blk = ctx.block();
                let slot_ptr = blk.gep(I8, &obj_ptr, &[(I64, &offset)]);
                let slot_addr = blk.ptrtoint(&slot_ptr, I64);
                (slot_ptr, slot_addr)
            };
            crate::expr::emit_jsvalue_slot_store_pointer_tested(
                ctx,
                &slot_ptr,
                &value,
                &obj_handle,
                addref_needed,
                &obj_bits,
                &slot_addr,
                barrier_needed,
                "family_set",
            );
        }
        if crate::codegen::helpers::check_contracts() {
            emit_contract(ctx, &site.desc_global, &object_box);
        }
        ctx.block().br(&merge_label);
        let family_end = ctx.block_label(ctx.current_block);

        // Any other object: the ordinary adds, in order. Each can collect
        // (a setter runs user code), so every operand is re-read per add.
        ctx.current_block = generic_idx;
        for (slot, key) in site.keys.iter().enumerate() {
            let key_idx = ctx.strings.intern(key);
            let handle = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let key_box = ctx.block().load(DOUBLE, &handle);
            let target_box = group.reread_emitted(ctx, object);
            let value = group.reread(ctx, operands[slot])?;
            ctx.block().call(
                DOUBLE,
                "js_put_value_set",
                &[
                    (DOUBLE, &target_box),
                    (DOUBLE, &key_box),
                    (DOUBLE, &value),
                    (DOUBLE, &target_box),
                    (I32, "1"),
                ],
            );
        }
        let generic_box = group.reread_emitted(ctx, object);
        ctx.block().br(&merge_label);
        let generic_end = ctx.block_label(ctx.current_block);

        ctx.current_block = merge_idx;
        Ok(ctx.block().phi(
            DOUBLE,
            &[(&object_box, &family_end), (&generic_box, &generic_end)],
        ))
    })
}
