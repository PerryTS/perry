//! Object model step 1 (OBJECT_MODEL_2026-10-13.md §3.2, §3.6):
//! `Object.create(P)` families whose `P` is a module's singleton literal.
//!
//! A creation point `Object.assign(Object.create(P), { k1: e1, …, kn: en })`
//! is one family: its objects are born with the keys `k1…kn` in that order,
//! and their [[Prototype]] is `P`. When `P` is a top-level `const` of the
//! module bound to a closed object literal (data keys only; module code runs
//! once per agent, so that binding names one object per agent), the family's
//! layout is fixed here: the keys, one slot each, and `P`'s identity
//! (`PerryProtoFamily`), registered at startup like every carried layout.
//!
//! Each `[[Set]]` that `Object.assign` performs on the new object is a key
//! add, and whether a prototype intercepts it is a compile-time fact: `P` is
//! born with data keys only and `%Object.prototype%` has no accessor but
//! `__proto__`, so only a definition of that name, an integrity operation, a
//! relink or dynamic code anywhere in the program could make one
//! (`ProtoCensus::create_add_free`). When the census proves every key free,
//! the creation point allocates the object on its layout and stores the
//! slots; no chain is read at run time. Otherwise it keeps the ordinary
//! operation.
//!
//! `P` takes the family's prototype serial at its birth
//! (`perry_bind_family_prototype`, emitted after the binding's
//! initialization), and `perry_create_family` checks that the `P` it is
//! handed carries it. A `P` that does not (unbound, or any other value)
//! takes the ordinary `Object.create`; the creation point then performs the
//! ordinary `[[Set]]`s, so its semantics never depend on the check.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use perry_hir::{Expr, Module, Stmt};

use crate::expr::FnCtx;
use crate::rooting::{self, Repr};
use crate::types::{DOUBLE, I32, I64, I8, PTR};

/// One creation point's family.
#[derive(Clone, Debug)]
pub(crate) struct Site {
    /// The family's carried descriptor.
    pub desc_global: String,
    /// Its keys, in order (slot = position).
    pub keys: Vec<String>,
    /// The `PerryProtoFamily` of its prototype binding.
    pub proto_global: String,
}

/// A `const a = Object.create(P)` creation point's family: born with one
/// slot for the first key the program adds to `a`, and the state that holds
/// it (§3.2, as `layout_desc::FamilyAdd` for literals).
#[derive(Clone, Debug)]
pub(crate) struct Add {
    /// The birth layout: no key, one slot.
    pub birth_desc: String,
    /// The state "`key` present", in that slot.
    pub state_desc: String,
    pub key: String,
    pub proto_global: String,
}

#[derive(Default)]
struct ModuleCreateFamilies {
    /// Prototype binding (top-level `const` local id) -> its
    /// `PerryProtoFamily` global, for bindings some site uses.
    protos: BTreeMap<u32, String>,
    /// `Expr::ObjectAssign` address -> its family.
    sites: HashMap<usize, Site>,
    /// Emission order of the sites' descriptors.
    order: Vec<usize>,
    /// `Expr::ObjectCreate` address -> its binding's family.
    creates: HashMap<usize, u32>,
    /// Binding (`const a`) -> its family.
    adds: BTreeMap<u32, Add>,
}

thread_local! {
    static MODULE_CREATE_FAMILIES: RefCell<ModuleCreateFamilies> =
        RefCell::new(ModuleCreateFamilies::default());
}

/// Forget the previous module's families.
pub(crate) fn clear() {
    MODULE_CREATE_FAMILIES.with(|m| *m.borrow_mut() = ModuleCreateFamilies::default());
}

/// A key a layout slot may carry and `Object.assign` adds in source order:
/// an atom, not an array index (enumerated first), not `__proto__`.
fn layout_key(key: &str) -> bool {
    super::key_is_atom(key)
        && key != "__proto__"
        && !(key.bytes().all(|b| b.is_ascii_digit()) && (key == "0" || !key.starts_with('0')))
}

/// May a family birth hold `key` without the ordinary `[[Set]]`: no
/// prototype of the family can intercept its add (the census), and adding it
/// would not set the shadow bit (it is no prototype name).
fn add_free(key: &str) -> bool {
    layout_key(key)
        && crate::proto_constants::create_add_free(key)
        && !crate::proto_constants::is_prototype_name(key)
}

/// The closed-literal class `e` allocates, if any.
fn literal_class<'a>(hir: &'a Module, e: &Expr) -> Option<&'a perry_hir::Class> {
    let Expr::New { class_name, .. } = e else {
        return None;
    };
    hir.classes
        .iter()
        .find(|c| &c.name == class_name)
        .filter(|c| c.is_literal_shape())
}

/// Collect the module's `Object.create(P)` families (`codegen/mod.rs`,
/// executables with layouts only).
pub(crate) fn collect(hir: &Module, module_prefix: &str) {
    clear();
    // Prototype candidates: top-level `const P = <closed literal>`.
    let mut candidates: HashMap<u32, ()> = HashMap::new();
    let mut bound: HashMap<u32, usize> = HashMap::new();
    for stmt in &hir.init {
        if let Stmt::Let {
            id,
            mutable: false,
            init: Some(init),
            ..
        } = stmt
        {
            *bound.entry(*id).or_default() += 1;
            if literal_class(hir, init).is_some() {
                candidates.insert(*id, ());
            }
        }
    }
    candidates.retain(|id, _| bound.get(id) == Some(&1));
    if candidates.is_empty() {
        return;
    }
    let mut state = ModuleCreateFamilies::default();
    perry_hir::for_each_module_expr(hir, &mut |e| {
        let Expr::ObjectAssign { target, sources } = e else {
            return;
        };
        let Expr::ObjectCreate(proto, None) = target.as_ref() else {
            return;
        };
        let Expr::LocalGet(p) = proto.as_ref() else {
            return;
        };
        if !candidates.contains_key(p) || sources.len() != 1 {
            return;
        }
        let Some(source) = literal_class(hir, &sources[0]) else {
            return;
        };
        let Expr::New { args, .. } = &sources[0] else {
            return;
        };
        let keys: Vec<String> = source.fields.iter().map(|f| f.name.clone()).collect();
        if keys.is_empty()
            || keys.len() != args.len()
            || source.fields.iter().any(|f| f.key_expr.is_some())
            || !keys.iter().all(|k| layout_key(k))
            || (1..keys.len()).any(|i| keys[..i].contains(&keys[i]))
            || !keys.iter().all(|k| add_free(k))
        {
            return;
        }
        let n = state.order.len();
        let proto_global = state
            .protos
            .entry(*p)
            .or_insert_with(|| format!("perry_protofam_{module_prefix}_{p}"))
            .clone();
        let addr = e as *const Expr as usize;
        state.sites.insert(
            addr,
            Site {
                desc_global: format!("perry_layout_create_{module_prefix}_{n}"),
                keys,
                proto_global,
            },
        );
        state.order.push(addr);
    });

    // `const a = Object.create(P)`: the first key the program adds to `a`.
    let bindings: Vec<(u32, usize, u32)> = crate::collectors::object_create_bindings(hir)
        .into_iter()
        .filter(|(_, _, p)| candidates.contains_key(p))
        .collect();
    if !bindings.is_empty() {
        let locals: HashMap<u32, ()> = bindings.iter().map(|(a, _, _)| (*a, ())).collect();
        let mut first: HashMap<u32, String> = HashMap::new();
        perry_hir::for_each_module_expr(hir, &mut |e| {
            if let Some((a, key)) = static_store(e) {
                if locals.contains_key(&a) {
                    first.entry(a).or_insert_with(|| key.to_string());
                }
            }
        });
        for (a, addr, p) in bindings {
            let Some(key) = first.remove(&a) else {
                continue;
            };
            if !add_free(&key) {
                continue;
            }
            let proto_global = state
                .protos
                .entry(p)
                .or_insert_with(|| format!("perry_protofam_{module_prefix}_{p}"))
                .clone();
            state.creates.insert(addr, a);
            state.adds.insert(
                a,
                Add {
                    birth_desc: format!("perry_layout_createb_{module_prefix}_{a}"),
                    state_desc: format!("perry_layout_creates_{module_prefix}_{a}"),
                    key,
                    proto_global,
                },
            );
        }
    }
    MODULE_CREATE_FAMILIES.with(|m| *m.borrow_mut() = state);
}

/// `a.k = v` (either spelling) with `a` a local: `(a, k)`.
fn static_store(e: &Expr) -> Option<(u32, &str)> {
    match e {
        Expr::PropertySet {
            object, property, ..
        } => match object.as_ref() {
            Expr::LocalGet(a) => Some((*a, property.as_str())),
            _ => None,
        },
        Expr::PutValueSet {
            target,
            key,
            receiver,
            ..
        } => match (target.as_ref(), key.as_ref(), receiver.as_ref()) {
            (Expr::LocalGet(a), Expr::String(k), Expr::LocalGet(r)) if a == r => {
                Some((*a, k.as_str()))
            }
            _ => None,
        },
        _ => None,
    }
}

/// The family of the `Object.create` expression `e`, if it is one.
pub(crate) fn create_site(e: &Expr) -> Option<Add> {
    MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        let a = m.creates.get(&(e as *const Expr as usize))?;
        m.adds.get(a).cloned()
    })
}

/// The family add `object.property = …` names, when `object` is a
/// `const a = Object.create(P)` binding whose first added key is `property`.
pub(crate) fn add_site(object: &Expr, property: &str) -> Option<Add> {
    let Expr::LocalGet(a) = object else {
        return None;
    };
    MODULE_CREATE_FAMILIES.with(|m| {
        m.borrow()
            .adds
            .get(a)
            .filter(|add| add.key == property)
            .cloned()
    })
}

/// The family of the `Object.assign` expression `e`, if it is one.
pub(crate) fn site(e: &Expr) -> Option<Site> {
    MODULE_CREATE_FAMILIES.with(|m| m.borrow().sites.get(&(e as *const Expr as usize)).cloned())
}

/// The `PerryProtoFamily` global of the prototype binding `id`, if a family
/// names it.
pub(crate) fn proto_global(id: u32) -> Option<String> {
    MODULE_CREATE_FAMILIES.with(|m| m.borrow().protos.get(&id).cloned())
}

/// The prototype records and family descriptors to emit, in order: each
/// `Object.assign` site's layout, then each binding's birth and state.
pub(crate) fn emission() -> (Vec<String>, Vec<Site>) {
    MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        let mut layouts: Vec<Site> = m.order.iter().map(|a| m.sites[a].clone()).collect();
        for add in m.adds.values() {
            layouts.push(Site {
                desc_global: add.birth_desc.clone(),
                keys: Vec::new(),
                proto_global: add.proto_global.clone(),
            });
            layouts.push(Site {
                desc_global: add.state_desc.clone(),
                keys: vec![add.key.clone()],
                proto_global: add.proto_global.clone(),
            });
        }
        (m.protos.values().cloned().collect(), layouts)
    })
}

/// `Object.create(P)` for a binding's family: born on its birth layout (one
/// hole slot) when `P` is the family's prototype, else the ordinary birth.
pub(crate) fn lower_create(ctx: &mut FnCtx<'_>, add: &Add, expr: &Expr) -> Result<String> {
    let Expr::ObjectCreate(proto, None) = expr else {
        anyhow::bail!("create family birth is not Object.create(P)");
    };
    let proto_box = crate::expr::lower_expr(ctx, proto)?;
    Ok(ctx.block().call(
        DOUBLE,
        "perry_create_family",
        &[(PTR, &format!("@{}", add.birth_desc)), (DOUBLE, &proto_box)],
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
        .entry_init_load_global(&super::layout_id_global(desc_global), I32);
    ctx.block().load(I32, &slot)
}

/// `a.k = v` for a binding's family add: an object in the family's birth or
/// state layout stores the slot and the state's layout word (no prototype is
/// read: the census proved the add free); any other receiver takes the
/// ordinary `[[Set]]`. Returns the stored value.
pub(crate) fn lower_add(
    ctx: &mut FnCtx<'_>,
    add: &Add,
    object: &Expr,
    value: &Expr,
    strict: bool,
) -> Result<String> {
    let barrier_needed = !crate::expr::expr_produces_non_pointer_bits_by_construction(ctx, value);
    let addref_needed = crate::expr::class_field_store_needs_string_addref(ctx, value);
    rooting::with_operands_rooted(ctx, &[object, value], |ctx, vals| {
        let (recv_box, val_double) = (vals[0].clone(), vals[1].clone());
        let birth = layout_id(ctx, &add.birth_desc);
        let state = layout_id(ctx, &add.state_desc);
        let check_idx = ctx.new_block("create_add.layout");
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
        let (obj_handle, obj_ptr) = {
            let blk = ctx.block();
            let obj_handle = crate::expr::receiver_range::emit_handle(blk, &recv.biased);
            let obj_ptr = blk.inttoptr(I64, &obj_handle);
            let word_ptr = blk.gep(I8, &obj_ptr, &[(I64, "4")]);
            let word = blk.load(I32, &word_ptr);
            let registered = blk.icmp_ne(I32, &state, "0");
            let is_birth = blk.icmp_eq(I32, &word, &birth);
            let is_state = blk.icmp_eq(I32, &word, &state);
            let either = blk.or(crate::types::I1, &is_birth, &is_state);
            let ok = blk.and(crate::types::I1, &registered, &either);
            blk.cond_br(&ok, &fast_label, &slow_label);
            (obj_handle, obj_ptr)
        };

        ctx.current_block = fast_idx;
        let header = crate::target_layout::object_header_size_bytes(ctx.target_triple);
        let (slot_ptr, slot_addr) = {
            let blk = ctx.block();
            let slot_ptr = blk.gep(I8, &obj_ptr, &[(I64, &header.to_string())]);
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
            blk.store(I32, &state, &word_ptr);
        }
        if crate::codegen::helpers::check_contracts() {
            emit_contract(ctx, &add.state_desc, &recv_box);
        }
        ctx.block().br(&merge_label);
        let fast_end = ctx.block_label(ctx.current_block);

        ctx.current_block = slow_idx;
        let key_idx = ctx.strings.intern(&add.key);
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
/// object the family's prototype serial (`stmt/mod.rs`).
pub(crate) fn lower_binding(ctx: &mut FnCtx<'_>, id: u32) -> Result<()> {
    let Some(global) = proto_global(id) else {
        return Ok(());
    };
    let read = Expr::LocalGet(id);
    let value = crate::expr::lower_expr(ctx, &read)?;
    ctx.block().call_void(
        "perry_bind_family_prototype",
        &[(PTR, &format!("@{global}")), (DOUBLE, &value)],
    );
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
        let id_global = super::layout_id_global(&site.desc_global);
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
        let header = crate::target_layout::object_header_size_bytes(ctx.target_triple);
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
