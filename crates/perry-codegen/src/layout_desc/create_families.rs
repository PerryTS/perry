//! Object model step 1 (OBJECT_MODEL_2026-10-13.md §3.2, §3.6):
//! `Object.create(P)` families whose `P` is a module's prototype binding.
//!
//! Two creation points make a family:
//!
//! * `Object.assign(Object.create(P), { k1: e1, …, kn: en })` with `P` a
//!   top-level `const` bound to a closed object literal: its objects are born
//!   with the keys `k1…kn` in that order ([`site`], [`lower_assign`]);
//! * `const a = Object.create(P)` followed by stores `a.k1 = …; a.k2 = …`
//!   among the statements after it (Option's `some`): born with one slot per
//!   key, on the state "no key yet", and each add, in source order, moves it
//!   to the next state ([`before_stmt`], [`lower_create`], [`lower_add`]).
//!   Here `P` is any top-level `const` whose initializer the compiler follows
//!   ([`born`]: literals with spreads, computed data keys, methods and
//!   accessors, `Object.create(Q)` of another such binding, `Object.assign`
//!   with literal sources, `Object.defineProperty` with a static key), so the
//!   accessors and possibly non-writable keys of every prototype on `P`'s
//!   chain are known at the binding.
//!
//! The family's layouts are fixed here: the keys, one slot each, and `P`'s
//! identity (`PerryProtoFamily`), registered at startup like every carried
//! layout. Each one is named by its content (the module, `P`'s binding and
//! the keys), so every creation point with the same family shares it, and
//! the module emits the ones its code used ([`emission`]).
//!
//! Each key add is a `[[Set]]` on the new object, and whether a prototype
//! intercepts it is a compile-time fact: no prototype of the chain names the
//! key as an accessor or a possibly non-writable property at its binding,
//! `%Object.prototype%`'s only accessor is `__proto__`, and only a
//! definition of that name, an integrity operation, a relink or dynamic code
//! anywhere in the program could change that (`ProtoCensus::
//! create_add_free`). When the census proves every key free, the add is a
//! slot store and no chain is read at run time. Otherwise the creation point
//! keeps the ordinary operation. (A chain that can change takes the checked
//! proof of decision 150, a compare of each prototype's layout word with its
//! carried layout's id, once prototypes are born on carried layouts.)
//!
//! `P` takes the family's prototype serial at its birth
//! (`perry_bind_family_prototype`, emitted after the binding's
//! initialization), and `perry_create_family` checks that the `P` it is
//! handed carries it. A `P` that does not (unbound, or any other value)
//! takes the ordinary `Object.create`; the creation point then performs the
//! ordinary `[[Set]]`s, so its semantics never depend on the check.
//!
//! Nothing here is looked up by an expression's address or kept past one
//! module's lowering: the module's prototype facts are derived once from its
//! HIR ([`collect`]), a binding's family is read off its `let` and the
//! statements after it while that body is lowered ([`CreateState`]), and an
//! `Object.assign` site's from the expression itself.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::Result;
use perry_hir::{Expr, Module, Stmt};

use crate::expr::FnCtx;
use crate::rooting::{self, Repr};
use crate::types::{DOUBLE, I32, I64, I8, PTR};

/// One family layout.
#[derive(Clone, Debug)]
pub(crate) struct Site {
    /// The family's carried descriptor.
    pub desc_global: String,
    /// Its keys, in order (slot = position).
    pub keys: Vec<String>,
    /// Its slot count (at least one, at least the key count).
    pub slots: u32,
    /// The `PerryProtoFamily` of its prototype binding.
    pub proto_global: String,
}

/// A `const a = Object.create(P)` creation point's family: born with one
/// slot for each key the statements after it store on `a`, in source order,
/// and one state per prefix of them (§3.2, as `layout_desc::FamilyAdd` for
/// literals).
#[derive(Clone, Debug)]
pub(crate) struct Create {
    /// `states[i]` holds `keys[..i]`; every state has `keys.len()` slots,
    /// and `states[0]` is the birth.
    pub states: Vec<String>,
    pub keys: Vec<String>,
    /// `P`'s binding.
    pub proto: u32,
    pub proto_global: String,
}

/// One add of a [`Create`] family: its key `keys[index]`.
#[derive(Clone, Debug)]
pub(crate) struct Add {
    pub family: Create,
    pub index: usize,
}

/// The create families of the body being lowered (a field of its `FnCtx`):
/// the family of the `let` statement being lowered, until its
/// `Object.create` takes it, and the families of the bindings born so far.
#[derive(Clone, Debug, Default)]
pub(crate) struct CreateState {
    pending: Option<(u32, Create)>,
    born: HashMap<u32, Create>,
}

/// What a prototype binding's initializer makes (§3.6): which keys of the
/// object are, or may be, accessors or non-writable, and its [[Prototype]].
/// Every other key it holds, named or not (a spread, a computed key), is a
/// plain writable data property, which an add shadows.
#[derive(Clone, Debug, Default)]
struct Born {
    blocks: BTreeSet<String>,
    proto: Link,
    /// An `Object.assign` source key the compiler cannot name may have
    /// relinked it (an own `__proto__` data key), which the census does not
    /// see: no family stands on its chain.
    dynamic: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Link {
    /// `%Object.prototype%`.
    #[default]
    Object,
    Null,
    /// Another prototype binding of the module.
    Binding(u32),
}

/// What a computed literal key is known to be.
#[derive(Clone, Debug)]
enum KeyConst {
    Str(String),
    Symbol,
}

/// `%Object.prototype%`'s own keys: data methods an add of the name would
/// shadow, and `__proto__`. A family key is none of them (an intrinsic's
/// attributes are not the compiler's to follow).
const OBJECT_PROTOTYPE_KEYS: &[&str] = &[
    "__proto__",
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// The module's prototype facts, derived once from its HIR, and the
/// layouts its lowered code used (the emission's input).
#[derive(Default)]
struct ModuleCreateFamilies {
    prefix: String,
    /// Prototype bindings the compiler follows.
    borns: HashMap<u32, Born>,
    /// Those bound to a closed literal (an `Object.assign` site's `P`).
    literals: BTreeSet<u32>,
    /// Followed prototypes some `Object.create` names (either creation
    /// point): their `let` binds the family identity.
    bound: BTreeSet<u32>,
    /// The layouts lowering used, by descriptor name.
    used: BTreeMap<String, Site>,
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
fn literal_class<'a>(classes: &[&'a perry_hir::Class], e: &Expr) -> Option<&'a perry_hir::Class> {
    let Expr::New { class_name, .. } = e else {
        return None;
    };
    classes
        .iter()
        .find(|c| &c.name == class_name)
        .copied()
        .filter(|c| c.is_literal_shape())
}

/// The prototype binding `Object.create`'s argument reads: `P`, or `P`
/// behind its TDZ check (a hoisted function's read).
fn proto_binding(proto: &Expr) -> Option<u32> {
    match proto {
        Expr::LocalGet(p) => Some(*p),
        Expr::Sequence(parts) => match parts.as_slice() {
            [Expr::Call { callee, args, .. }, Expr::LocalGet(p)]
                if matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. }
                    if name == "js_tdz_check_binding")
                    && matches!(args.first(), Some(Expr::LocalGet(x)) if x == p) =>
            {
                Some(*p)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Derive the module's prototype facts (`codegen/mod.rs`, executables with
/// layouts only): one pass over its top-level bindings, one over its
/// `Object.create` calls.
pub(crate) fn collect(hir: &Module, module_prefix: &str) {
    clear();
    let classes: Vec<&perry_hir::Class> = hir.classes.iter().collect();
    let mut bound_once: HashMap<u32, usize> = HashMap::new();
    for stmt in &hir.init {
        if let Stmt::Let {
            id,
            mutable: false,
            init: Some(_),
            ..
        } = stmt
        {
            *bound_once.entry(*id).or_default() += 1;
        }
    }
    // Prototype bindings the compiler follows, in initialization order (a
    // chain names only earlier ones).
    let mut state = ModuleCreateFamilies {
        prefix: module_prefix.to_string(),
        ..ModuleCreateFamilies::default()
    };
    let mut consts: HashMap<u32, KeyConst> = HashMap::new();
    for stmt in &hir.init {
        let Stmt::Let {
            id,
            mutable: false,
            init: Some(init),
            ..
        } = stmt
        else {
            continue;
        };
        if bound_once.get(id) != Some(&1) {
            continue;
        }
        match init {
            Expr::String(s) => {
                consts.insert(*id, KeyConst::Str(s.clone()));
            }
            Expr::SymbolFor(_) => {
                consts.insert(*id, KeyConst::Symbol);
            }
            _ => {
                if literal_class(&classes, init).is_some() {
                    state.literals.insert(*id);
                }
                if let Some(b) = born(&classes, init, &state.borns, &consts) {
                    state.borns.insert(*id, b);
                }
            }
        }
    }
    if !state.borns.is_empty() {
        perry_hir::for_each_module_expr(hir, &mut |e| {
            if let Expr::ObjectCreate(proto, None) = e {
                if let Some(p) = proto_binding(proto) {
                    if state.borns.contains_key(&p) {
                        state.bound.insert(p);
                    }
                }
            }
        });
    }
    MODULE_CREATE_FAMILIES.with(|m| *m.borrow_mut() = state);
}

/// The family of `const a = Object.create(P)`, from the statements `rest`
/// after it: the longest prefix of the keys they store on `a` whose adds
/// the census proves free on `P`'s chain.
fn binding_family(m: &ModuleCreateFamilies, a: u32, p: u32, rest: &[Stmt]) -> Option<Create> {
    let chain = chain_of(p, &m.borns)?;
    if chain.iter().any(|h| m.borns[h].dynamic) {
        return None;
    }
    let mut keys: Vec<String> = Vec::new();
    for stmt in rest {
        let Stmt::Expr(e) = stmt else { continue };
        let Some((target, key)) = static_store(e) else {
            continue;
        };
        if target != a || keys.iter().any(|k| k == key) {
            continue;
        }
        if keys.len() == MAX_CREATE_KEYS
            || !add_free(key)
            || OBJECT_PROTOTYPE_KEYS.contains(&key)
            || chain.iter().any(|h| m.borns[h].blocks.contains(key))
        {
            break;
        }
        keys.push(key.to_string());
    }
    if keys.is_empty() {
        return None;
    }
    let family = format!(
        "{}_{p}_{:08x}",
        m.prefix,
        super::fnv1a(keys.join("\u{1}").as_bytes()) as u32
    );
    Some(Create {
        states: (0..=keys.len())
            .map(|i| format!("perry_layout_cf_{family}_{i}"))
            .collect(),
        keys,
        proto: p,
        proto_global: format!("perry_protofam_{}_{p}", m.prefix),
    })
}

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
fn use_layout(site: Site) {
    MODULE_CREATE_FAMILIES.with(|m| {
        m.borrow_mut()
            .used
            .entry(site.desc_global.clone())
            .or_insert(site);
    });
}

/// The family whose birth the `Object.create` expression `e` is: the
/// pending creation point's, when `e` is its initializer. Its layouts are
/// used from here on, and the binding's stores find it.
pub(crate) fn create_site(ctx: &mut FnCtx<'_>, e: &Expr) -> Option<Create> {
    let Expr::ObjectCreate(proto, None) = e else {
        return None;
    };
    let p = proto_binding(proto)?;
    if ctx
        .create_families
        .pending
        .as_ref()
        .is_none_or(|(_, f)| f.proto != p)
    {
        return None;
    }
    let (a, family) = ctx.create_families.pending.take()?;
    for (i, desc) in family.states.iter().enumerate() {
        use_layout(Site {
            desc_global: desc.clone(),
            keys: family.keys[..i].to_vec(),
            slots: family.keys.len() as u32,
            proto_global: family.proto_global.clone(),
        });
    }
    ctx.create_families.born.insert(a, family.clone());
    Some(family)
}

/// The family add `object.property = …` names, when `object` is a binding
/// born of a family that holds `property`.
pub(crate) fn add_site(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> Option<Add> {
    let Expr::LocalGet(a) = object else {
        return None;
    };
    let family = ctx.create_families.born.get(a)?;
    let index = family.keys.iter().position(|k| k == property)?;
    Some(Add {
        family: family.clone(),
        index,
    })
}

/// The family of `Object.assign(Object.create(P), { k1: e1, … })`, when `P`
/// is bound to a closed literal and every key's add is free.
pub(crate) fn site(ctx: &FnCtx<'_>, e: &Expr) -> Option<Site> {
    let Expr::ObjectAssign { target, sources } = e else {
        return None;
    };
    let Expr::ObjectCreate(proto, None) = target.as_ref() else {
        return None;
    };
    let p = proto_binding(proto)?;
    let [source] = sources.as_slice() else {
        return None;
    };
    let Expr::New {
        class_name, args, ..
    } = source
    else {
        return None;
    };
    let class = ctx
        .classes
        .get(class_name)
        .filter(|c| c.is_literal_shape())?;
    let keys: Vec<String> = class.fields.iter().map(|f| f.name.clone()).collect();
    if keys.is_empty()
        || keys.len() != args.len()
        || class.fields.iter().any(|f| f.key_expr.is_some())
        || !keys.iter().all(|k| layout_key(k))
        || (1..keys.len()).any(|i| keys[..i].contains(&keys[i]))
        || !keys.iter().all(|k| add_free(k))
    {
        return None;
    }
    let site = MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        m.literals.contains(&p).then(|| Site {
            desc_global: format!(
                "perry_layout_create_{}_{p}_{:08x}",
                m.prefix,
                super::fnv1a(keys.join("\u{1}").as_bytes()) as u32
            ),
            slots: keys.len() as u32,
            keys,
            proto_global: format!("perry_protofam_{}_{p}", m.prefix),
        })
    })?;
    use_layout(site.clone());
    Some(site)
}

/// Is the prototype binding `id` some family's `P` (its `let` binds the
/// family identity)?
fn bound_proto_global(id: u32) -> Option<String> {
    MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        m.bound
            .contains(&id)
            .then(|| format!("perry_protofam_{}_{id}", m.prefix))
    })
}

/// The prototype records and family descriptors to emit: every followed
/// prototype an `Object.create` names, and each layout lowered code used.
pub(crate) fn emission() -> (Vec<String>, Vec<Site>) {
    MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        let protos = m
            .bound
            .iter()
            .map(|id| format!("perry_protofam_{}_{id}", m.prefix))
            .collect();
        (protos, m.used.values().cloned().collect())
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
        .entry_init_load_global(&super::layout_id_global(desc_global), I32);
    ctx.block().load(I32, &slot)
}

/// GcHeader `_reserved` bits that refuse a key add: frozen, sealed, not
/// extensible (runtime `gc::OBJ_FLAG_FROZEN | OBJ_FLAG_SEALED |
/// OBJ_FLAG_NO_EXTEND`).
const RECEIVER_INTEGRITY_BITS: &str = "7";

/// `a.k = v` for a binding family's add of `keys[index]`. A receiver in the
/// state before it (the add) or in the state holding it (an overwrite of its
/// own writable data), with no meta record (no overflow, no elements, no
/// recorded [[Prototype]]: its prototype is the family's) and not frozen,
/// sealed or made non-extensible, stores the slot and the holding state's
/// layout word; no prototype is read (the census proved the add free). Any
/// other receiver takes the ordinary `[[Set]]`. Returns the stored value.
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
        let header = crate::target_layout::object_header_size_bytes(ctx.target_triple);
        let pointer = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
            4
        } else {
            8
        };
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
            blk.cond_br(&ok, &fast_label, &slow_label);
            (obj_handle, obj_ptr)
        };

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
/// object the family's prototype serial (`stmt/mod.rs`).
pub(crate) fn lower_binding(ctx: &mut FnCtx<'_>, id: u32) -> Result<()> {
    let Some(global) = bound_proto_global(id) else {
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

/// The most keys one binding's family holds.
const MAX_CREATE_KEYS: usize = 16;

/// The prototype bindings of `p`'s chain (`p` first), when every link up to
/// `%Object.prototype%` or null is a binding the compiler follows.
fn chain_of(p: u32, borns: &HashMap<u32, Born>) -> Option<Vec<u32>> {
    let mut chain = vec![p];
    loop {
        match borns.get(chain.last()?)?.proto {
            Link::Object | Link::Null => return Some(chain),
            Link::Binding(q) => {
                if chain.contains(&q) || chain.len() == 8 {
                    return None;
                }
                chain.push(q);
            }
        }
    }
}

/// The keys of `b`'s chain that are, or may be, accessors or non-writable.
fn chain_blocks(b: &Born, borns: &HashMap<u32, Born>) -> BTreeSet<String> {
    let mut blocks = b.blocks.clone();
    let mut link = b.proto;
    for _ in 0..8 {
        let Link::Binding(q) = link else { break };
        let Some(next) = borns.get(&q) else { break };
        blocks.extend(next.blocks.iter().cloned());
        link = next.proto;
    }
    blocks
}

/// [`Born`] of a prototype binding's initializer `e`, when the compiler can
/// follow it: a literal, `Object.create(Q)` with `Q` null or an earlier such
/// binding, `Object.assign(T, literal…)`, `Object.defineProperty(T, "k",
/// literal)`. No user code that could reach the object runs before the
/// binding holds it: a literal's keys and values never see it, a source's
/// getters see the source, and the binding is in its TDZ until then.
fn born(
    classes: &[&perry_hir::Class],
    e: &Expr,
    borns: &HashMap<u32, Born>,
    consts: &HashMap<u32, KeyConst>,
) -> Option<Born> {
    match e {
        Expr::ObjectCreate(proto, None) => {
            let proto = match proto.as_ref() {
                Expr::Null => Link::Null,
                Expr::LocalGet(q) if borns.contains_key(q) => Link::Binding(*q),
                _ => return None,
            };
            Some(Born {
                proto,
                ..Born::default()
            })
        }
        Expr::ObjectAssign { target, sources } => {
            let mut t = born(classes, target, borns, consts)?;
            // Each source key is a `[[Set]]` on the target: refused where a
            // setter could run (user code with the object) or a key could
            // relink it. The source's own accessors yield data.
            let blocks = chain_blocks(&t, borns);
            for src in sources {
                let (_, names, open) = literal(classes, src, consts)?;
                if (open && !blocks.is_empty())
                    || names.iter().any(|k| k == "__proto__" || blocks.contains(k))
                {
                    return None;
                }
                t.dynamic |= open;
            }
            Some(t)
        }
        Expr::ObjectDefineProperty(target, key, desc) => {
            let Expr::String(k) = key.as_ref() else {
                return None;
            };
            literal(classes, desc, consts)?;
            let mut t = born(classes, target, borns, consts)?;
            // Defaults make a new key non-writable; an accessor intercepts.
            t.blocks.insert(k.clone());
            Some(t)
        }
        _ => literal(classes, e, consts).map(|(b, _, _)| b),
    }
}

/// A literal's [`Born`] (its [[Prototype]] is `%Object.prototype%`), the
/// string keys it names, and whether it may hold keys it does not name.
fn literal(
    classes: &[&perry_hir::Class],
    e: &Expr,
    consts: &HashMap<u32, KeyConst>,
) -> Option<(Born, BTreeSet<String>, bool)> {
    let mut names = BTreeSet::new();
    let mut open = false;
    match e {
        Expr::New { class_name, .. } => {
            let c = classes
                .iter()
                .find(|c| &c.name == class_name)
                .filter(|c| c.is_literal_shape())?;
            for f in &c.fields {
                if f.key_expr.is_some() {
                    open = true;
                } else {
                    names.insert(f.name.clone());
                }
            }
        }
        Expr::Object(props) => {
            for (k, _) in props {
                names.insert(k.clone());
            }
        }
        Expr::ObjectSpread { parts } => {
            for (k, _) in parts {
                match k {
                    Some(k) => {
                        names.insert(k.clone());
                    }
                    None => open = true,
                }
            }
        }
        Expr::Call { callee, args, .. } => {
            let mut blocks = BTreeSet::new();
            open = literal_iife(callee, args, consts, &mut names, &mut blocks)?;
            return Some((
                Born {
                    blocks,
                    ..Born::default()
                },
                names,
                open,
            ));
        }
        _ => return None,
    }
    // `__proto__: v` in a literal sets the [[Prototype]].
    if names.contains("__proto__") {
        return None;
    }
    Some((Born::default(), names, open))
}

/// The object-literal IIFE codegen's lowering of a literal with computed
/// keys or spreads makes: `(o => { define(o, "k", v); set_computed(o, key,
/// v); spread(o, src); method(o, key, f); accessor(o, key, get, set); …;
/// return o })({})`. Every key the compiler can name goes to `names`, an
/// accessor's to `blocks` too. Returns whether some data key is unnamed, or
/// `None` for anything else (an unnamed accessor, `o` escaping).
fn literal_iife(
    callee: &Expr,
    args: &[Expr],
    consts: &HashMap<u32, KeyConst>,
    names: &mut BTreeSet<String>,
    blocks: &mut BTreeSet<String>,
) -> Option<bool> {
    let Expr::Closure { params, body, .. } = callee else {
        return None;
    };
    // The object starts empty: `{}`, or the old-generation birth of a
    // module-singleton literal (`js_object_literal_prototype_birth`).
    let empty = |e: &Expr| match e {
        Expr::Object(p) => p.is_empty(),
        Expr::Call { callee, args, .. } => {
            args.is_empty()
                && matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. }
                    if name == "js_object_literal_prototype_birth")
        }
        _ => false,
    };
    if params.len() != 1 || args.len() != 1 || !empty(&args[0]) {
        return None;
    }
    let o = params[0].id;
    let mut keys: HashMap<u32, Option<KeyConst>> = HashMap::new();
    let key_of = |e: &Expr, keys: &HashMap<u32, Option<KeyConst>>| -> Option<KeyConst> {
        match e {
            Expr::String(s) => Some(KeyConst::Str(s.clone())),
            Expr::SymbolFor(_) => Some(KeyConst::Symbol),
            Expr::LocalGet(x) => keys
                .get(x)
                .cloned()
                .flatten()
                .or_else(|| consts.get(x).cloned()),
            _ => None,
        }
    };
    let mut open = false;
    for (i, stmt) in body.iter().enumerate() {
        match stmt {
            Stmt::Let {
                id,
                mutable: false,
                init: Some(init),
                ..
            } if !mentions(init, o) => {
                let key = match init {
                    Expr::Call { callee, args, .. }
                        if matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. }
                            if name == "js_object_literal_to_property_key")
                            && args.len() == 1 =>
                    {
                        key_of(&args[0], &keys)
                    }
                    other => key_of(other, &keys),
                };
                keys.insert(*id, key);
            }
            Stmt::Expr(Expr::Call { callee, args, .. }) => {
                let Expr::ExternFuncRef { name, .. } = callee.as_ref() else {
                    return None;
                };
                let on_o = matches!(args.first(), Some(Expr::LocalGet(x)) if *x == o);
                if args.iter().skip(usize::from(on_o)).any(|a| mentions(a, o)) {
                    return None;
                }
                match (name.as_str(), on_o) {
                    ("js_object_literal_define", true) if args.len() == 3 => match &args[1] {
                        Expr::String(k) if k != "__proto__" => {
                            names.insert(k.clone());
                        }
                        _ => return None,
                    },
                    (
                        "js_object_literal_set_computed"
                        | "js_object_set_method_by_name"
                        | "js_object_set_property_key_method",
                        true,
                    ) if args.len() == 3 => match key_of(&args[1], &keys) {
                        Some(KeyConst::Str(k)) => {
                            names.insert(k);
                        }
                        Some(KeyConst::Symbol) => {}
                        None => open = true,
                    },
                    ("js_object_define_accessor", true) if args.len() == 4 => {
                        match key_of(&args[1], &keys)? {
                            KeyConst::Str(k) => {
                                names.insert(k.clone());
                                blocks.insert(k);
                            }
                            KeyConst::Symbol => {}
                        }
                    }
                    ("js_object_literal_spread", true) => open = true,
                    ("js_object_literal_infer_computed_function_name", false) => {}
                    _ => return None,
                }
            }
            Stmt::Return(Some(Expr::LocalGet(x))) if *x == o && i + 1 == body.len() => {
                return Some(open);
            }
            _ => return None,
        }
    }
    None
}

/// Does `e` read, write or capture the local `id`?
fn mentions(e: &Expr, id: u32) -> bool {
    match e {
        Expr::LocalGet(x) | Expr::LocalSet(x, _) if *x == id => return true,
        Expr::Closure { captures, .. } if captures.contains(&id) => return true,
        _ => {}
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(e, &mut |c| found = found || mentions(c, id));
    found
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
