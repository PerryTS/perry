//! Prototype constants (object model step 3, `OBJECT_MODEL_2026-10-13.md`
//! §3.6): the program-wide prototype-write census, as codegen consults it,
//! and the shadow-bit tests of the sites that bind its constants.
//!
//! The driver scans every module (`perry_hir::scan_proto_census`) and hands
//! the union here before any module is lowered. A method name the census
//! proves constant resolves, for a receiver of a known class, to the body its
//! class chain declares: the call binds that body directly. The census is part
//! of the object-cache key (`perry/src/commands/compile/object_cache.rs`):
//! every module lowers against the whole program.
//!
//! When some write in the program has a key or target the census cannot name
//! (a dynamic write) or relinks a [[Prototype]], the binding holds while the
//! objects involved carry no shadow bit (bit 30 of the layout word at header
//! `+4`, set by the runtime's `object::shadow` on exactly those writes): the
//! site compares the receiver's word and each static-chain holder's word with
//! `UNSHADOWED_WORD_END` (one unsigned compare each; shadowed, dictionary and
//! exotic words all lie above it) and takes its generic arm otherwise. A
//! program with no such write binds with no test at all.
//!
//! With no census installed (unit tests, tools that lower one module) nothing
//! is constant, which is the conservative answer.

use std::sync::RwLock;

use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

static PROGRAM_PROTO_CENSUS: RwLock<Option<perry_hir::ProtoCensus>> = RwLock::new(None);

/// Install the program's census (driver, before module codegen).
pub fn set_program_proto_census(census: Option<perry_hir::ProtoCensus>) {
    *PROGRAM_PROTO_CENSUS.write().unwrap() = census;
}

/// The object-cache key of the installed census.
pub fn program_proto_census_key() -> String {
    PROGRAM_PROTO_CENSUS
        .read()
        .unwrap()
        .as_ref()
        .map(perry_hir::ProtoCensus::cache_key)
        .unwrap_or_default()
}

fn with_census<R>(f: impl FnOnce(&perry_hir::ProtoCensus) -> R) -> Option<R> {
    #[cfg(test)]
    if let Some(census) = TEST_CENSUS.with(|c| c.borrow().clone()) {
        return Some(f(&census));
    }
    PROGRAM_PROTO_CENSUS.read().unwrap().as_ref().map(f)
}

/// Is every prototype slot named `name` written exactly once in the program,
/// and can no object gain an own property of that name except through a
/// write that sets its shadow bit?
pub fn method_is_constant(name: &str) -> bool {
    with_census(|c| c.method_is_constant(name)).unwrap_or(false)
}

/// Object model step 1: is a key add of `name` on an object whose
/// prototype is a closed literal never intercepted (`ProtoCensus::
/// create_add_free`)? False with no census installed.
pub(crate) fn create_add_free(name: &str) -> bool {
    with_census(|c| c.create_add_free(name)).unwrap_or(false)
}

/// Is `name` a prototype name (`ProtoCensus::prototype_names`, the list the
/// entry module emits as `perry_shadow_names`)? The runtime sets the shadow
/// bit when one is added to an object with a recorded [[Prototype]]; a
/// compile-time family birth never does, so its keys must not be one. True
/// with no census installed.
pub(crate) fn is_prototype_name(name: &str) -> bool {
    with_census(|c| c.prototype_names().any(|n| n == name)).unwrap_or(true)
}

/// Must a binding site test the shadow bits (a dynamic write or a relink
/// exists somewhere in the program)?
pub(crate) fn shadow_tests() -> bool {
    with_census(perry_hir::ProtoCensus::shadow_reachable).unwrap_or(true)
}

/// No compiled or interpreted operation can relink a constructor's heritage.
/// Missing facts decline the proof; no program-wide dynamic-code veto.
pub(crate) fn heritage_is_unrelinked(ctx: &crate::expr::FnCtx<'_>, class_id: u32) -> bool {
    if ctx.wpfacts.super_unrelinked.contains(&class_id) {
        return true;
    }
    // Unit tests install a complete synthetic program census. Production
    // requires the data-flow proof too: an opaque callee can mutate a class
    // it receives even when no compiled module names setPrototypeOf.
    #[cfg(test)]
    {
        return with_census(|c| !c.relink && !c.dynamic_write && !c.dynamic_code).unwrap_or(false);
    }
    #[cfg(not(test))]
    false
}

/// The names whose dynamic add, delete or redefinition sets the shadow bit
/// (emitted once, by the entry module, as `perry_shadow_names`).
/// `(name, receiver)`: every class member name (a prototype's shadow
/// names), `receiver` when it is also a receiver's (a constant member in a
/// program with a dynamic write or relink).
pub(crate) fn shadow_names() -> Vec<(String, bool)> {
    with_census(|c| {
        let receiver: std::collections::BTreeSet<&String> = c.shadow_names().collect();
        c.prototype_names()
            .map(|n| (n.clone(), receiver.contains(n)))
            .collect()
    })
    .unwrap_or_default()
}

#[cfg(test)]
thread_local! {
    /// Unit tests lower one module with no driver: the census of that module
    /// alone, which writes nothing (an empty census), unless a test installs
    /// another with [`with_test_census`].
    static TEST_CENSUS: std::cell::RefCell<Option<perry_hir::ProtoCensus>> =
        std::cell::RefCell::new(Some(perry_hir::ProtoCensus::default()));
}

/// Run `f` with `census` as this thread's program census (unit tests).
#[cfg(test)]
pub(crate) fn with_test_census<R>(census: perry_hir::ProtoCensus, f: impl FnOnce() -> R) -> R {
    let saved = TEST_CENSUS.with(|c| c.replace(Some(census)));
    let result = f();
    TEST_CENSUS.with(|c| *c.borrow_mut() = saved);
    result
}

/// Does `class`'s `method` resolve, through the prototype chain the compiler
/// names (`static_prototype::chain`: declared classes with complete static
/// prototype shapes, the method a ConstFn slot of its holder), to a slot the
/// census proves constant? Then every instance of `class` whose receiver and
/// holders carry no shadow bit calls that body.
pub(crate) fn class_method_is_constant(
    ctx: &crate::expr::FnCtx<'_>,
    class: &str,
    method: &str,
) -> bool {
    method_is_constant(method)
        && crate::codegen::static_prototype::chain(ctx, class, method).is_some()
}

/// `i1`: the layout word `word` (an `i32` read at header `+4`) is
/// unshadowed (`perry_abi::UNSHADOWED_WORD_END`).
pub(crate) fn emit_word_unshadowed(blk: &mut crate::block::LlBlock, word: &str) -> String {
    let end = crate::runtime_abi::LAYOUT_ORDINARY_END as i32;
    blk.icmp_ult(I32, word, &end.to_string())
}

/// `i1`: the receiver whose packed header word is `packed` (`class_id |
/// layout word << 32`, as `emit_inline_direct_method_word_probe` loads it;
/// 0 for a non-object) carries no shadow bit. `"true"` when the program needs
/// no test.
pub(crate) fn emit_receiver_unshadowed(ctx: &mut crate::expr::FnCtx<'_>, packed: &str) -> String {
    if !shadow_tests() {
        return "true".into();
    }
    let blk = ctx.block();
    let hi = blk.lshr(I64, packed, "32");
    let word = blk.trunc(I64, &hi, I32);
    emit_word_unshadowed(blk, &word)
}

/// `i1`: the boxed object `value` (proven an ordinary object) carries no
/// shadow bit.
fn emit_value_unshadowed(ctx: &mut crate::expr::FnCtx<'_>, value: &str) -> String {
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(value);
    let handle = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let at = blk.add(I64, &handle, "4");
    let ptr = blk.inttoptr(I64, &at);
    let word = blk.load(I32, &ptr);
    emit_word_unshadowed(blk, &word)
}

/// The holder proof of a site that already proved its receiver's exact
/// `(class_id, ShapeId)` (which refuses a shadowed receiver word): an `i1`
/// that the holders of `class`'s static chain for `method` carry no shadow
/// bit. `"false"` when the compiler names no chain; `"true"` for a constant
/// method in a program with no dynamic write or relink. The select path
/// (`dynamic_dispatch_select`), which proves only the receiver's class,
/// still requires a constant name.
pub(crate) fn holder_constant(
    ctx: &mut crate::expr::FnCtx<'_>,
    class: &str,
    method: &str,
) -> String {
    if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        return "false".into();
    }
    let Some(chain) = crate::codegen::static_prototype::chain(ctx, class, method) else {
        return "false".into();
    };
    if method_is_constant(method) && !shadow_tests() {
        return "true".into();
    }
    // A name the program writes (or any name, once a dynamic write exists):
    // a clear bit on each holder proves its slots still hold what the class
    // definition stored (a prototype's bit is set by any write after its
    // creation, static or dynamic).
    holders_unshadowed(ctx, &chain)
}

/// `i1`: every prototype of `chain` (class ids, holder last) carries no
/// shadow bit. A class prototype is materialized lazily, on first demand
/// through the agent's class-value directory; one that was never
/// materialized was never reachable by any write, so it is unshadowed.
fn holders_unshadowed(ctx: &mut crate::expr::FnCtx<'_>, chain: &[(u32, u32)]) -> String {
    let dir = crate::expr::agent_ptr::emit_agent_ptr(
        ctx,
        crate::runtime_abi::AGENT_PTR_CLASS_VALUES,
        "perry_class_value_dir_cell",
    );
    let mut result = "true".to_string();
    for &(cid, _) in chain {
        let ok = holder_unshadowed(ctx, &dir, cid);
        result = ctx.block().and(I1, &result, &ok);
    }
    result
}

fn holder_unshadowed(ctx: &mut crate::expr::FnCtx<'_>, dir: &str, cid: u32) -> String {
    let page = ctx.new_block("holder.page");
    let ctor = ctx.new_block("holder.constructor");
    let link = ctx.new_block("holder.link");
    let word = ctx.new_block("holder.word");
    let merge = ctx.new_block("holder.merge");
    let pl = ctx.block_label(page);
    let cl = ctx.block_label(ctor);
    let ll = ctx.block_label(link);
    let wl = ctx.block_label(word);
    let ml = ctx.block_label(merge);
    let start = ctx.block().label.clone();
    let pages = ctx.block().load(PTR, dir);
    let at = ctx.block().gep(I8, dir, &[(I64, "8")]);
    let len = ctx.block().load(I64, &at);
    let present = ctx.block().icmp_ult(I64, &(cid >> 8).to_string(), &len);
    ctx.block().cond_br(&present, &pl, &ml);
    ctx.current_block = page;
    let at = ctx
        .block()
        .gep(PTR, &pages, &[(I64, &(cid >> 8).to_string())]);
    let p = ctx.block().load(PTR, &at);
    let present = ctx.block().icmp_ne(PTR, &p, "null");
    ctx.block().cond_br(&present, &cl, &ml);
    ctx.current_block = ctor;
    let at = ctx.block().gep(PTR, &p, &[(I64, &(cid & 255).to_string())]);
    let c = ctx.block().load(PTR, &at);
    let present = ctx.block().icmp_ne(PTR, &c, "null");
    ctx.block().cond_br(&present, &ll, &ml);
    ctx.current_block = link;
    let off = crate::runtime_abi::CLOSURE_HEADER_SIZE
        + crate::runtime_abi::CLASS_PROTOTYPE_LINK_CAPTURE * 8;
    let at = ctx.block().gep(I8, &c, &[(I64, &off.to_string())]);
    let bits = ctx.block().load(I64, &at);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let present = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::POINTER_TAG_TOP16_I64);
    ctx.block().cond_br(&present, &wl, &ml);
    ctx.current_block = word;
    let raw = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let ptr = ctx.block().inttoptr(I64, &raw);
    let at = ctx.block().gep(I8, &ptr, &[(I64, "4")]);
    let live = ctx.block().load(I32, &at);
    let unshadowed = emit_word_unshadowed(ctx.block(), &live);
    ctx.block().br(&ml);
    ctx.current_block = merge;
    ctx.block().phi(
        I1,
        &[
            ("true", &start),
            ("true", &pl),
            ("true", &cl),
            ("true", &ll),
            (&unshadowed, &wl),
        ],
    )
}

/// Contract-check builds: [`emit_contract`] for a selection case whose
/// `answer` (`i32`) may be 0 at run time (a shadowed holder): the witness
/// runs only when the case is really taken.
pub(crate) fn emit_selected_contract(
    ctx: &mut crate::expr::FnCtx<'_>,
    recv: &str,
    answer: &str,
    class_id: u32,
    property: &str,
    function: &str,
) {
    if answer.parse::<u32>().is_ok() {
        emit_contract(ctx, recv, class_id, property, function);
        if shadow_tests() {
            emit_shadow_contract(ctx, recv, "true", property);
        }
        return;
    }
    let check = ctx.new_block("pconst.contract");
    let cont = ctx.new_block("pconst.contract.done");
    let cl = ctx.block_label(check);
    let dl = ctx.block_label(cont);
    let taken = ctx.block().icmp_ne(I32, answer, "0");
    ctx.block().cond_br(&taken, &cl, &dl);
    ctx.current_block = check;
    emit_contract(ctx, recv, class_id, property, function);
    emit_shadow_contract(ctx, recv, "true", property);
    ctx.block().br(&dl);
    ctx.current_block = cont;
}

/// Contract-check builds: `js_proto_shadow_contract_check(ok, recv, name)`,
/// the witness of a site that tests the shadow bit. When the emitted test
/// passed, the runtime re-derives what it stands for: the receiver has no own
/// property of the bound name.
fn emit_shadow_contract(ctx: &mut crate::expr::FnCtx<'_>, recv: &str, ok: &str, property: &str) {
    let key = ctx.strings.intern(property);
    let dispatch = ctx.strings.static_dispatch_global(key);
    let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch);
    let blk = ctx.block();
    let ok32 = if ok == "true" {
        "1".to_string()
    } else {
        blk.zext(I1, ok, I32)
    };
    blk.call_void(
        "js_proto_shadow_contract_check",
        &[(I32, &ok32), (DOUBLE, recv), (I64, &method_id)],
    );
}

/// Contract-check builds: assert at a bound call that the live receiver is
/// in the proven family (`class_id`, 0 = any) and its chain still resolves
/// `property` to exactly `function`'s body
/// (`js_proto_constant_contract_check`). Release code emits nothing.
pub(crate) fn emit_contract(
    ctx: &mut crate::expr::FnCtx<'_>,
    recv: &str,
    class_id: u32,
    property: &str,
    function: &str,
) {
    let key = ctx.strings.intern(property);
    let dispatch = ctx.strings.static_dispatch_global(key);
    let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch);
    let info = ctx.block().fn_info_ref(&format!("{function}__eclo"));
    let info = ctx.block().ptrtoint(&info, I64);
    ctx.block().call_void(
        "js_proto_constant_contract_check",
        &[
            (DOUBLE, recv),
            (I64, &method_id),
            (I32, &class_id.to_string()),
            (I64, &info),
        ],
    );
}

/// [`emit_contract`] for an exactly proven receiver class.
pub(crate) fn emit_exact_family_contract(
    ctx: &mut crate::expr::FnCtx<'_>,
    recv: &str,
    class: &str,
    property: &str,
    function: &str,
) {
    let class_id = ctx.class_ids.get(class).copied().unwrap_or(0);
    emit_contract(ctx, recv, class_id, property, function);
}

/// Inherited data through a known prototype (§3.6): `o.k` where `o`'s family
/// is `Object.create(P)` for one `const` holder binding `P` whose class
/// declares `k` as a data field, and the census proves `k` constant. While
/// neither `o` nor `P` carries a shadow bit, `o` has no own `k` and its
/// [[Prototype]] is `P`, so `o.k` is `P.k`: the holder read replaces the
/// receiver's lookup. Returns the lowered read, or `None` when the site is
/// not one.
pub(crate) fn lower_known_holder_read(
    ctx: &mut crate::expr::FnCtx<'_>,
    object: &perry_hir::Expr,
    property: &str,
    read: &perry_hir::Expr,
) -> Option<anyhow::Result<String>> {
    let holder = ctx.known_holder_binding(object)?;
    if !method_is_constant(property) || property == "__proto__" {
        return None;
    }
    let class_name = ctx
        .native_facts
        .shape_stability
        .exact_class_bindings
        .get(&holder)?
        .clone();
    let class = ctx.classes.get(&class_name)?;
    let declares_data = class
        .fields
        .iter()
        .any(|f| !f.is_private && f.key_expr.is_none() && f.name == property)
        && !class.getters.iter().any(|(n, _)| n == property)
        && !class.setters.iter().any(|(n, _)| n == property)
        && class.extends_name.is_none();
    if !declares_data {
        return None;
    }
    let perry_hir::Expr::PropertyGet { byte_offset, .. } = read else {
        return None;
    };
    let holder_expr = perry_hir::Expr::LocalGet(holder);
    let holder_read = perry_hir::Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(holder_expr.clone()),
        property: property.to_string(),
        byte_offset: *byte_offset,
    };
    Some((|| {
        let tests = shadow_tests();
        if !tests && !crate::codegen::helpers::check_contracts() {
            return crate::expr::property_get::lower(ctx, &holder_read);
        }
        let recv = crate::expr::lower_expr(ctx, object)?;
        let held = crate::expr::lower_expr(ctx, &holder_expr)?;
        let ok = if tests {
            let r = emit_value_unshadowed(ctx, &recv);
            let h = emit_value_unshadowed(ctx, &held);
            ctx.block().and(I1, &r, &h)
        } else {
            "true".to_string()
        };
        if crate::codegen::helpers::check_contracts() {
            emit_shadow_contract(ctx, &recv, &ok, property);
        }
        if !tests {
            emit_holder_contract(ctx, &recv, &held, property);
            return crate::expr::property_get::lower(ctx, &holder_read);
        }
        let fast = ctx.new_block("pconst.holder.read");
        let slow = ctx.new_block("pconst.holder.generic");
        let merge = ctx.new_block("pconst.holder.merge");
        let fl = ctx.block_label(fast);
        let sl = ctx.block_label(slow);
        let ml = ctx.block_label(merge);
        ctx.block().cond_br(&ok, &fl, &sl);
        ctx.current_block = fast;
        if crate::codegen::helpers::check_contracts() {
            emit_holder_contract(ctx, &recv, &held, property);
        }
        let v_fast = crate::expr::property_get::lower(ctx, &holder_read)?;
        let fast_end = ctx.block().label.clone();
        ctx.block().br(&ml);
        ctx.current_block = slow;
        let v_slow = crate::expr::property_get::lower(ctx, read)?;
        let slow_end = ctx.block().label.clone();
        ctx.block().br(&ml);
        ctx.current_block = merge;
        Ok(ctx
            .block()
            .phi(DOUBLE, &[(&v_fast, &fast_end), (&v_slow, &slow_end)]))
    })())
}

fn emit_holder_contract(ctx: &mut crate::expr::FnCtx<'_>, recv: &str, held: &str, property: &str) {
    let key = ctx.strings.intern(property);
    let dispatch = ctx.strings.static_dispatch_global(key);
    let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch);
    ctx.block().call_void(
        "js_proto_holder_contract_check",
        &[(DOUBLE, recv), (DOUBLE, held), (I64, &method_id)],
    );
}

/// The most classes one class-holder read dispatches over: a constructor
/// of a class with more descendants reads through the ordinary read.
const MAX_HOLDER_CLASSES: usize = 64;

/// The classes `o` may be an instance of when `o.m` reads through a class
/// family's layout ([`lower_class_holder_read`]): the leaf class `C` for
/// `this` in an inlined `new C(…)` constructor body (the leaf's, also inside
/// an inherited constructor's body) or a binding constructed by `new C(…)`;
/// `C` and every class descending from it for `this` in `C`'s standalone
/// constructor, which `new C(…)` and every subclass's `super(…)` reach.
/// Only a guess: the site compares the receiver's layout word with each
/// class's family layout before it trusts it.
fn holder_read_classes(
    ctx: &crate::expr::FnCtx<'_>,
    object: &perry_hir::Expr,
) -> Option<Vec<String>> {
    match object {
        perry_hir::Expr::This => {
            let top = ctx.this_stack.last()?;
            if let Some((_, class)) = ctx
                .inline_new_leaf
                .iter()
                .rev()
                .find(|(slot, _)| slot == top)
            {
                return Some(vec![class.clone()]);
            }
            let (slot, class) = ctx.ctor_receiver.as_ref()?;
            if slot != top {
                return None;
            }
            let mut below: Vec<String> = ctx
                .class_hierarchy
                .descendants(class, None)
                .into_iter()
                .map(str::to_string)
                .collect();
            below.sort();
            let mut classes = vec![class.clone()];
            classes.extend(below);
            (classes.len() <= MAX_HOLDER_CLASSES).then_some(classes)
        }
        _ => ctx.exact_receiver_class(object).map(|class| vec![class]),
    }
}

/// One class a class-holder read may find its receiver an instance of: the
/// class's family (`cid`), the key's slot in that layout, the class's static
/// prototype chain to the holder of the key, and the holder's slot.
struct HolderClass {
    /// The class's position in [`holder_read_classes`]' list (the
    /// constructor's receiver selector names it).
    position: usize,
    cid: u32,
    own_slot: Option<u32>,
    chain: Vec<(u32, u32)>,
    holder_slot: i64,
}

fn holder_class(ctx: &crate::expr::FnCtx<'_>, class: &str, property: &str) -> Option<HolderClass> {
    let cid = *ctx.class_ids.get(class)?;
    let own_slot = ctx
        .program_layouts
        .families
        .get(&cid)?
        .keys
        .iter()
        .find(|(k, _)| k == property)
        .map(|(_, slot)| *slot);
    let chain = crate::codegen::static_prototype::chain(ctx, class, property)?;
    let &(holder_cid, _) = chain.last()?;
    let (_, shape) = crate::codegen::static_shape_ids::static_prototype_layout(holder_cid)?;
    let holder_slot = shape
        .keys
        .split(|&b| b == 0)
        .take(shape.key_count as usize)
        .position(|k| k == property.as_bytes())? as i64;
    Some(HolderClass {
        position: 0,
        cid,
        own_slot,
        chain,
        holder_slot,
    })
}

/// `i64`: the raw address of class `cid`'s prototype object, or 0 while it
/// is not materialized (a prototype is created on first demand through the
/// agent's class-value directory; see [`holder_unshadowed`]).
fn emit_class_prototype_ptr(ctx: &mut crate::expr::FnCtx<'_>, dir: &str, cid: &str) -> String {
    let page = ctx.new_block("cholder.page");
    let ctor = ctx.new_block("cholder.constructor");
    let link = ctx.new_block("cholder.link");
    let found = ctx.new_block("cholder.found");
    let merge = ctx.new_block("cholder.merge");
    let pl = ctx.block_label(page);
    let cl = ctx.block_label(ctor);
    let ll = ctx.block_label(link);
    let fl = ctx.block_label(found);
    let ml = ctx.block_label(merge);
    // `cid` is an `i64` class id: a constant, or a class chosen at the site
    // (`lower_class_holder_read`).
    let (page_index, slot_index) = match cid.parse::<u64>() {
        Ok(cid) => ((cid >> 8).to_string(), (cid & 255).to_string()),
        Err(_) => {
            let blk = ctx.block();
            (blk.lshr(I64, cid, "8"), blk.and(I64, cid, "255"))
        }
    };
    let start = ctx.block().label.clone();
    let pages = ctx.block().load(PTR, dir);
    let at = ctx.block().gep(I8, dir, &[(I64, "8")]);
    let len = ctx.block().load(I64, &at);
    let present = ctx.block().icmp_ult(I64, &page_index, &len);
    ctx.block().cond_br(&present, &pl, &ml);
    ctx.current_block = page;
    let at = ctx.block().gep(PTR, &pages, &[(I64, &page_index)]);
    let p = ctx.block().load(PTR, &at);
    let present = ctx.block().icmp_ne(PTR, &p, "null");
    ctx.block().cond_br(&present, &cl, &ml);
    ctx.current_block = ctor;
    let at = ctx.block().gep(PTR, &p, &[(I64, &slot_index)]);
    let c = ctx.block().load(PTR, &at);
    let present = ctx.block().icmp_ne(PTR, &c, "null");
    ctx.block().cond_br(&present, &ll, &ml);
    ctx.current_block = link;
    let off = crate::runtime_abi::CLOSURE_HEADER_SIZE
        + crate::runtime_abi::CLASS_PROTOTYPE_LINK_CAPTURE * 8;
    let at = ctx.block().gep(I8, &c, &[(I64, &off.to_string())]);
    let bits = ctx.block().load(I64, &at);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let present = ctx
        .block()
        .icmp_eq(I64, &tag, crate::nanbox::POINTER_TAG_TOP16_I64);
    ctx.block().cond_br(&present, &fl, &ml);
    ctx.current_block = found;
    let raw = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    ctx.block().br(&ml);
    ctx.current_block = merge;
    ctx.block().phi(
        I64,
        &[
            ("0", &start),
            ("0", &pl),
            ("0", &cl),
            ("0", &ll),
            (&raw, &fl),
        ],
    )
}

/// An inherited method read through a class family's layout (§3.6): `o.m`
/// where `o` is an instance of one of the classes [`holder_read_classes`]
/// names, each born on its family layout with a static prototype chain that
/// names the holder of `m` (a ConstFn method slot, `static_prototype::chain`).
/// The site:
///
/// ```text
///   o's layout word == the family layout id of one class C of the set,
///       no meta record (every own key is in the layout; [[Prototype]] is
///       C.prototype)
///   m in the layout: its slot, unless the hole        -> the own value
///   each prototype of C's chain materialized and on its static shape
///       (word == ShapeId: no key added, deleted or redefined, so the
///       holder lists m as data at its slot and no prototype before it
///       holds m)
///   the holder's slot, unless the hole                -> the value
/// ```
///
/// Classes whose chains end in the same prototypes share that part of the
/// read (a standalone constructor's subclasses all reach its class's
/// prototype). Every word read is the receiver's or a prototype's own data
/// compared with an immutable layout id; anything else takes the ordinary
/// read. Returns `None` when the site is not one.
pub(crate) fn lower_class_holder_read(
    ctx: &mut crate::expr::FnCtx<'_>,
    object: &perry_hir::Expr,
    property: &str,
    read: &perry_hir::Expr,
) -> Option<anyhow::Result<String>> {
    if crate::target_layout::target_is_ilp32(ctx.target_triple)
        || ctx.target_triple.contains("wasm")
        || property == "__proto__"
        || !crate::layout_desc::layouts_enabled("executable", ctx.target_triple)
    {
        return None;
    }
    let classes: Vec<HolderClass> = holder_read_classes(ctx, object)?
        .iter()
        .enumerate()
        .filter_map(|(position, class)| {
            holder_class(ctx, class, property).map(|h| HolderClass { position, ..h })
        })
        .collect();
    if classes.is_empty() {
        return None;
    }
    Some((|| {
        let header = crate::target_layout::object_slot_base_bytes(
            ctx.target_triple,
            crate::target_layout::HeaderForm::Meta,
        ) as i64;
        let recv = crate::expr::lower_expr(ctx, object)?;
        let check = ctx.new_block("cholder.layout");
        let slow = ctx.new_block("cholder.generic");
        let merge = ctx.new_block("cholder.merge");
        let check_l = ctx.block_label(check);
        let slow_l = ctx.block_label(slow);
        let merge_l = ctx.block_label(merge);
        let mut hits: Vec<(String, String)> = Vec::new();
        let biased = {
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(&recv);
            let fused = crate::expr::receiver_range::emit_fused_receiver_test(blk, &bits);
            blk.cond_br(&fused.is_object_pointer, &check_l, &slow_l);
            fused.biased
        };
        ctx.current_block = check;
        let first = ctx.new_block("cholder.class");
        let first_l = ctx.block_label(first);
        let word = {
            let blk = ctx.block();
            let wp = crate::expr::receiver_range::emit_field_ptr(blk, &biased, 4);
            let word = blk.load(I32, &wp);
            let mp = crate::expr::receiver_range::emit_field_ptr(blk, &biased, header - 8);
            let meta = blk.load(PTR, &mp);
            let no_meta = blk.icmp_eq(PTR, &meta, "null");
            blk.cond_br(&no_meta, &first_l, &slow_l);
            word
        };
        ctx.current_block = first;
        let known = ctx.new_block("cholder.known");
        let picks: Vec<Pick> = classes
            .iter()
            .map(|c| Pick {
                position: c.position,
                cid: c.cid,
            })
            .collect();
        let index = emit_receiver_class(ctx, &word, &picks, known, &slow_l);
        ctx.current_block = known;
        // The receiver is a family object of `classes[index]`: its own slot,
        // then that class's chain.
        let chains: Vec<usize> = classes
            .iter()
            .map(|_| ctx.new_block("cholder.chain"))
            .collect();
        let shared_own = classes[0].own_slot;
        if classes.iter().all(|class| class.own_slot == shared_own) {
            // Every family descending from a class holds its keys at the
            // same slots, so a constructor's classes share the own read.
            let dispatch = ctx.new_block("cholder.dispatch");
            if let Some(slot) = shared_own {
                emit_own_hit(
                    ctx,
                    &biased,
                    header + 8 * i64::from(slot),
                    dispatch,
                    &merge_l,
                    &mut hits,
                );
            } else {
                let l = ctx.block_label(dispatch);
                ctx.block().br(&l);
            }
            ctx.current_block = dispatch;
            emit_class_switch(ctx, &index, &picks, &chains, &slow_l);
        } else {
            let bodies: Vec<usize> = classes
                .iter()
                .map(|_| ctx.new_block("cholder.own"))
                .collect();
            emit_class_switch(ctx, &index, &picks, &bodies, &slow_l);
            for ((class, &body), &chain) in classes.iter().zip(&bodies).zip(&chains) {
                ctx.current_block = body;
                if let Some(slot) = class.own_slot {
                    emit_own_hit(
                        ctx,
                        &biased,
                        header + 8 * i64::from(slot),
                        chain,
                        &merge_l,
                        &mut hits,
                    );
                } else {
                    let l = ctx.block_label(chain);
                    ctx.block().br(&l);
                }
            }
        }
        // Classes whose chains continue through the same prototypes to the
        // same holder slot share one read: each class's block passes its
        // first prototype (class id, static ShapeId) to the group's.
        struct Group {
            rest: Vec<(u32, u32)>,
            holder_slot: i64,
            block: usize,
            firsts: Vec<(String, u32, u32)>,
        }
        let mut groups: Vec<Group> = Vec::new();
        for (class, &chain) in classes.iter().zip(&chains) {
            ctx.current_block = chain;
            let (&(pcid, shape_id), rest) = class.chain.split_first().expect("a holder chain");
            let at = match groups
                .iter()
                .position(|g| g.rest == rest && g.holder_slot == class.holder_slot)
            {
                Some(at) => at,
                None => {
                    let block = ctx.new_block("cholder.group");
                    groups.push(Group {
                        rest: rest.to_vec(),
                        holder_slot: class.holder_slot,
                        block,
                        firsts: Vec::new(),
                    });
                    groups.len() - 1
                }
            };
            let label = ctx.block().label.clone();
            groups[at].firsts.push((label, pcid, shape_id));
            let l = ctx.block_label(groups[at].block);
            ctx.block().br(&l);
        }
        for group in groups {
            ctx.current_block = group.block;
            let (cid, shape_id) = match group.firsts.as_slice() {
                [(_, cid, shape_id)] => (cid.to_string(), (*shape_id as i32).to_string()),
                firsts => {
                    let cids: Vec<(String, &str)> = firsts
                        .iter()
                        .map(|(l, cid, _)| (cid.to_string(), l.as_str()))
                        .collect();
                    let shapes: Vec<(String, &str)> = firsts
                        .iter()
                        .map(|(l, _, shape_id)| ((*shape_id as i32).to_string(), l.as_str()))
                        .collect();
                    let blk = ctx.block();
                    let cid = blk.phi(
                        I64,
                        &cids
                            .iter()
                            .map(|(v, l)| (v.as_str(), *l))
                            .collect::<Vec<_>>(),
                    );
                    let shape_id = blk.phi(
                        I32,
                        &shapes
                            .iter()
                            .map(|(v, l)| (v.as_str(), *l))
                            .collect::<Vec<_>>(),
                    );
                    (cid, shape_id)
                }
            };
            let dir = emit_class_value_dir(ctx);
            let mut holder = emit_prototype_on_shape(ctx, &dir, &cid, &shape_id, &slow_l);
            for &(pcid, shape_id) in &group.rest {
                holder = emit_prototype_on_shape(
                    ctx,
                    &dir,
                    &pcid.to_string(),
                    &(shape_id as i32).to_string(),
                    &slow_l,
                );
            }
            emit_holder_hit(
                ctx,
                &holder,
                header + 8 * group.holder_slot,
                &slow_l,
                &merge_l,
                &mut hits,
            );
        }
        ctx.current_block = slow;
        let generic = crate::expr::property_get::lower(ctx, read)?;
        hits.push((generic, ctx.block().label.clone()));
        ctx.block().br(&merge_l);
        ctx.current_block = merge;
        let incoming: Vec<(&str, &str)> =
            hits.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
        Ok(ctx.block().phi(DOUBLE, &incoming))
    })())
}

/// `i1`: the layout word `word` is class `cid`'s family layout (its
/// descriptor's registered id word, written once at startup before any user
/// code; a class family's descriptor may live in another unit, so it is
/// addressed, not aliased).
fn emit_family_word_is(ctx: &mut crate::expr::FnCtx<'_>, word: &str, cid: u32) -> String {
    let desc = crate::layout_desc::note_family_desc_use(cid);
    let blk = ctx.block();
    let at = blk.gep(
        I8,
        &desc,
        &[(I64, &crate::layout_desc::DESC_LAYOUT_ID_OFFSET.to_string())],
    );
    let id = blk.load(I32, &at);
    let registered = blk.icmp_ne(I32, &id, "0");
    let same = blk.icmp_eq(I32, word, &id);
    blk.and(I1, &registered, &same)
}

/// One class a receiver may be a family object of: its position in
/// [`holder_read_classes`]' list (the constructor's receiver selector names
/// it) and its family.
#[derive(Clone, Copy)]
struct Pick {
    position: usize,
    cid: u32,
}

/// Which of `classes` the receiver at layout word `word` is a family object
/// of, branching to `known` when one (the returned `i32` names its
/// [`Pick::position`] there) and to `slow` otherwise.
///
/// One class is one compare. A standalone constructor's classes are found
/// once per activation: the first read searches them and keeps the position
/// it found in the constructor's receiver selector (an `i32` local, 0 until
/// a read found one); each later read jumps to that class's compare, and
/// searches again when it fails. The selector only orders the compares:
/// every read proves its receiver by its own word compare.
fn emit_receiver_class(
    ctx: &mut crate::expr::FnCtx<'_>,
    word: &str,
    classes: &[Pick],
    known: usize,
    slow_l: &str,
) -> String {
    let known_l = ctx.block_label(known);
    if let [class] = classes {
        let ok = emit_family_word_is(ctx, word, class.cid);
        ctx.block().cond_br(&ok, &known_l, slow_l);
        return "0".into();
    }
    let selector = ctor_receiver_selector(ctx);
    let search = ctx.new_block("cholder.search");
    let search_l = ctx.block_label(search);
    let cases: Vec<usize> = classes
        .iter()
        .map(|_| ctx.new_block("cholder.kept"))
        .collect();
    let kept = ctx.block().load(I32, &selector);
    let mut ir = format!("switch i32 {kept}, label %{search_l} [");
    for (class, &case) in classes.iter().zip(&cases) {
        ir.push_str(&format!(
            " i32 {}, label %{}",
            class.position + 1,
            ctx.block_label(case)
        ));
    }
    ir.push_str(" ]");
    ctx.block().emit_raw(ir);
    ctx.block().mark_terminated();
    for (class, &case) in classes.iter().zip(&cases) {
        ctx.current_block = case;
        let ok = emit_family_word_is(ctx, word, class.cid);
        ctx.block().cond_br(&ok, &known_l, &search_l);
    }
    ctx.current_block = search;
    for class in classes {
        let found = ctx.new_block("cholder.found");
        let found_l = ctx.block_label(found);
        let next = ctx.new_block("cholder.search");
        let next_l = ctx.block_label(next);
        let ok = emit_family_word_is(ctx, word, class.cid);
        ctx.block().cond_br(&ok, &found_l, &next_l);
        ctx.current_block = found;
        ctx.block()
            .store(I32, &(class.position + 1).to_string(), &selector);
        ctx.block().br(&known_l);
        ctx.current_block = next;
    }
    ctx.block().br(slow_l);
    ctx.current_block = known;
    let kept = ctx.block().load(I32, &selector);
    ctx.block().sub(I32, &kept, "1")
}

/// The standalone constructor's receiver selector ([`emit_receiver_class`]),
/// made on first use.
fn ctor_receiver_selector(ctx: &mut crate::expr::FnCtx<'_>) -> String {
    if let Some(slot) = &ctx.ctor_receiver_selector {
        return slot.clone();
    }
    let slot = ctx.func.alloca_entry(I32);
    ctx.func.entry_allocas_push_store(I32, "0", &slot);
    ctx.ctor_receiver_selector = Some(slot.clone());
    slot
}

/// `switch` on `index` (an `i32` [`Pick::position`]) to the block of
/// that class in `targets` (a single target needs no test), anything else to
/// `default_l`.
fn emit_class_switch(
    ctx: &mut crate::expr::FnCtx<'_>,
    index: &str,
    classes: &[Pick],
    targets: &[usize],
    default_l: &str,
) {
    if let [target] = targets {
        let l = ctx.block_label(*target);
        ctx.block().br(&l);
        return;
    }
    let mut ir = format!("switch i32 {index}, label %{default_l} [");
    for (class, &target) in classes.iter().zip(targets) {
        ir.push_str(&format!(
            " i32 {}, label %{}",
            class.position,
            ctx.block_label(target)
        ));
    }
    ir.push_str(" ]");
    ctx.block().emit_raw(ir);
    ctx.block().mark_terminated();
}

/// The receiver's own slot at byte `offset` of `biased`: a hit into `merge`
/// unless the hole, which continues in block `absent`.
fn emit_own_hit(
    ctx: &mut crate::expr::FnCtx<'_>,
    biased: &str,
    offset: i64,
    absent: usize,
    merge_l: &str,
    hits: &mut Vec<(String, String)>,
) {
    let absent_l = ctx.block_label(absent);
    let hit = ctx.new_block("cholder.own_hit");
    let hit_l = ctx.block_label(hit);
    let v = {
        let blk = ctx.block();
        let sp = crate::expr::receiver_range::emit_field_ptr(blk, biased, offset);
        let v = blk.load(I64, &sp);
        let hole = blk.icmp_eq(I64, &v, crate::nanbox::TAG_HOLE_I64);
        blk.cond_br(&hole, &absent_l, &hit_l);
        v
    };
    ctx.current_block = hit;
    let d = ctx.block().bitcast_i64_to_double(&v);
    hits.push((d, ctx.block().label.clone()));
    ctx.block().br(merge_l);
}

/// The agent's class-value directory ([`emit_class_prototype_ptr`]).
fn emit_class_value_dir(ctx: &mut crate::expr::FnCtx<'_>) -> String {
    crate::expr::agent_ptr::emit_agent_ptr(
        ctx,
        crate::runtime_abi::AGENT_PTR_CLASS_VALUES,
        "perry_class_value_dir_cell",
    )
}

/// `i64`: class `cid`'s prototype (`cid` an `i64`, `shape_id` an `i32`:
/// constants or values), continuing in a new block when it is materialized
/// and on its static shape (word == `shape_id`), else to `slow`.
fn emit_prototype_on_shape(
    ctx: &mut crate::expr::FnCtx<'_>,
    dir: &str,
    cid: &str,
    shape_id: &str,
    slow_l: &str,
) -> String {
    let p = emit_class_prototype_ptr(ctx, dir, cid);
    let on = ctx.new_block("cholder.proto");
    let on_l = ctx.block_label(on);
    let present = ctx.block().icmp_ne(I64, &p, "0");
    ctx.block().cond_br(&present, &on_l, slow_l);
    ctx.current_block = on;
    let next = ctx.new_block("cholder.proto_ok");
    let next_l = ctx.block_label(next);
    let blk = ctx.block();
    let at = blk.add(I64, &p, "4");
    let wp = blk.inttoptr(I64, &at);
    let word = blk.load(I32, &wp);
    let same = blk.icmp_eq(I32, &word, shape_id);
    blk.cond_br(&same, &next_l, slow_l);
    ctx.current_block = next;
    p
}

/// The holder's slot at byte `offset` of `holder` (an `i64` address): a hit
/// into `merge` unless the hole, which takes `slow`.
fn emit_holder_hit(
    ctx: &mut crate::expr::FnCtx<'_>,
    holder: &str,
    offset: i64,
    slow_l: &str,
    merge_l: &str,
    hits: &mut Vec<(String, String)>,
) {
    let hit = ctx.new_block("cholder.hit");
    let hit_l = ctx.block_label(hit);
    let v = {
        let blk = ctx.block();
        let at = blk.add(I64, holder, &offset.to_string());
        let sp = blk.inttoptr(I64, &at);
        let v = blk.load(I64, &sp);
        let hole = blk.icmp_eq(I64, &v, crate::nanbox::TAG_HOLE_I64);
        blk.cond_br(&hole, slow_l, &hit_l);
        v
    };
    ctx.current_block = hit;
    let d = ctx.block().bitcast_i64_to_double(&v);
    hits.push((d, ctx.block().label.clone()));
    ctx.block().br(merge_l);
}

/// One class a constructor run's receiver may be a family object of
/// ([`emit_family_run_proof`]): its pick, its family's slot count, the slots
/// of the keys after the run's (each must still hold the hole) and its static
/// prototype chain to `%Object.prototype%`.
struct RunClass {
    pick: Pick,
    slot_count: u32,
    later: Vec<u32>,
    chain: Vec<(u32, u32)>,
}

fn run_class(
    ctx: &crate::expr::FnCtx<'_>,
    pick: Pick,
    name: &str,
    property: &str,
    pos: u32,
    order_known: bool,
    class_id: u32,
) -> Option<RunClass> {
    let families = &ctx.program_layouts.families;
    let family = families.get(&pick.cid)?;
    // A family of the run's class or of one descending from it.
    let mut c = pick.cid;
    for _ in 0..64 {
        if c == class_id {
            break;
        }
        c = families
            .get(&c)
            .map(|f| f.parent_class_id)
            .filter(|&p| p != 0)?;
    }
    if c != class_id {
        return None;
    }
    if family.keys.get(pos as usize).and_then(|(k, _)| k.name()) != Some(property) {
        return None;
    }
    // The properties the run may add (a private element is never one).
    let keys: Vec<&str> = family.keys.iter().filter_map(|(k, _)| k.name()).collect();
    let chain = crate::codegen::static_prototype::add_chain(ctx, name, &keys)?;
    let later = if order_known {
        Vec::new()
    } else {
        family.keys[pos as usize..]
            .iter()
            .map(|(_, slot)| *slot)
            .collect()
    };
    Some(RunClass {
        pick,
        slot_count: family.slot_count,
        later,
        chain,
    })
}

/// Object model step 2c, the run check (PLAN 3c) proved at the site: a
/// constructor run's stores, from key position `pos` (`property`) on, fill
/// `this`'s hole slots in place when
///
/// ```text
///   this is a family object of one class C of the constructor's classes
///       ([`holder_read_classes`], found as a class-holder read finds them):
///       layout word == C's family layout id (no shadow bit, so no
///       overflow), no meta record (not a prototype)
///   no integrity flag (frozen, sealed, non-extensible, stable tombstones)
///   C's slots inline, and from `pos` on no key present after a hole
///       (unless `order_known`)
///   each prototype of C's static chain materialized and on its static
///       shape, none of which holds an accessor, a non-writable or a private
///       property named like a family key (checked at compile time), and
///       %Object.prototype% unwritten (its word unshadowed)
/// ```
///
/// which is what `perry_family_run_admits` answers with one record read per
/// hop. Returns the blocks the proof ends in, `(proved, refused)` (the
/// caller asks the runtime in `refused`), or `None` when no class of the
/// site can be proved here.
pub(crate) fn emit_family_run_proof(
    ctx: &mut crate::expr::FnCtx<'_>,
    this_bits: &str,
    dir: &str,
    property: &str,
    pos: u32,
    order_known: bool,
    class_id: u32,
) -> Option<(usize, usize)> {
    if crate::target_layout::target_is_ilp32(ctx.target_triple)
        || ctx.target_triple.contains("wasm")
        || !crate::layout_desc::layouts_enabled("executable", ctx.target_triple)
    {
        return None;
    }
    let names = holder_read_classes(ctx, &perry_hir::Expr::This)?;
    let classes: Vec<RunClass> = names
        .iter()
        .enumerate()
        .filter_map(|(position, name)| {
            let cid = *ctx.class_ids.get(name)?;
            run_class(
                ctx,
                Pick { position, cid },
                name,
                property,
                pos,
                order_known,
                class_id,
            )
        })
        .collect();
    if classes.is_empty() {
        return None;
    }
    let header = crate::target_layout::object_slot_base_bytes(
        ctx.target_triple,
        crate::target_layout::HeaderForm::Meta,
    ) as i64;
    let gc = crate::runtime_abi::GC_HEADER_SIZE as i64;
    let proved = ctx.new_block("crun.proved");
    let refused = ctx.new_block("crun.refused");
    let proved_l = ctx.block_label(proved);
    let refused_l = ctx.block_label(refused);
    let object = ctx.new_block("crun.object");
    let object_l = ctx.block_label(object);
    let biased = {
        let blk = ctx.block();
        let fused = crate::expr::receiver_range::emit_fused_receiver_test(blk, this_bits);
        blk.cond_br(&fused.is_object_pointer, &object_l, &refused_l);
        fused.biased
    };
    ctx.current_block = object;
    let realm = ctx.new_block("crun.realm");
    let realm_l = ctx.block_label(realm);
    let (word, size) = {
        let blk = ctx.block();
        let wp = crate::expr::receiver_range::emit_field_ptr(blk, &biased, 4);
        let word = blk.load(I32, &wp);
        let mp = crate::expr::receiver_range::emit_field_ptr(blk, &biased, header - 8);
        let meta = blk.load(PTR, &mp);
        let no_meta = blk.icmp_eq(PTR, &meta, "null");
        // `GcHeader { obj_type: u8, gc_flags: u8, _reserved: u16, size: u32 }`
        // right before the object.
        let fp = crate::expr::receiver_range::emit_field_ptr(blk, &biased, 2 - gc);
        let flags = blk.load(crate::types::I16, &fp);
        let refusing = blk.and(
            crate::types::I16,
            &flags,
            &crate::runtime_abi::OBJ_FLAGS_REFUSE_FAMILY_RUN.to_string(),
        );
        let plain = blk.icmp_eq(crate::types::I16, &refusing, "0");
        let sp = crate::expr::receiver_range::emit_field_ptr(blk, &biased, 4 - gc);
        let size = blk.load(I32, &sp);
        let ok = blk.and(I1, &no_meta, &plain);
        blk.cond_br(&ok, &realm_l, &refused_l);
        (word, size)
    };
    ctx.current_block = realm;
    emit_object_prototype_unwritten(ctx, dir, &refused_l);
    let known = ctx.new_block("crun.known");
    let picks: Vec<Pick> = classes.iter().map(|c| c.pick).collect();
    let index = emit_receiver_class(ctx, &word, &picks, known, &refused_l);
    ctx.current_block = known;
    // Classes with the same slot count, later slots and chain after the
    // first prototype share one proof, given the first prototype.
    struct Group<'c> {
        class: &'c RunClass,
        block: usize,
        firsts: Vec<(String, u32, u32)>,
    }
    let edges: Vec<usize> = classes
        .iter()
        .map(|_| ctx.new_block("crun.class"))
        .collect();
    emit_class_switch(ctx, &index, &picks, &edges, &refused_l);
    let mut groups: Vec<Group<'_>> = Vec::new();
    for (class, &edge) in classes.iter().zip(&edges) {
        ctx.current_block = edge;
        let at = match groups.iter().position(|g| {
            g.class.slot_count == class.slot_count
                && g.class.later == class.later
                && g.class.chain[1..] == class.chain[1..]
        }) {
            Some(at) => at,
            None => {
                let block = ctx.new_block("crun.group");
                groups.push(Group {
                    class,
                    block,
                    firsts: Vec::new(),
                });
                groups.len() - 1
            }
        };
        let (pcid, shape_id) = class.chain[0];
        let label = ctx.block().label.clone();
        groups[at].firsts.push((label, pcid, shape_id));
        let l = ctx.block_label(groups[at].block);
        ctx.block().br(&l);
    }
    for group in groups {
        ctx.current_block = group.block;
        let (cid, shape_id) = emit_first_prototype(ctx, &group.firsts);
        let scan = ctx.new_block("crun.scan");
        let scan_l = ctx.block_label(scan);
        {
            let blk = ctx.block();
            let need = (gc + header) as u64 + 8 * u64::from(group.class.slot_count);
            let inline = blk.icmp_uge(I32, &size, &need.to_string());
            blk.cond_br(&inline, &scan_l, &refused_l);
        }
        ctx.current_block = scan;
        if !group.class.later.is_empty() {
            let next = ctx.new_block("crun.scan");
            let next_l = ctx.block_label(next);
            let blk = ctx.block();
            let mut hole_seen = "false".to_string();
            let mut disordered = "false".to_string();
            for &slot in &group.class.later {
                let sp = crate::expr::receiver_range::emit_field_ptr(
                    blk,
                    &biased,
                    header + 8 * i64::from(slot),
                );
                let v = blk.load(I64, &sp);
                let present = blk.icmp_ne(I64, &v, crate::nanbox::TAG_HOLE_I64);
                let after_hole = blk.and(I1, &present, &hole_seen);
                disordered = blk.or(I1, &disordered, &after_hole);
                let hole = blk.xor(I1, &present, "true");
                hole_seen = blk.or(I1, &hole_seen, &hole);
            }
            blk.cond_br(&disordered, &refused_l, &next_l);
            ctx.current_block = next;
        }
        let cdir = emit_class_value_dir(ctx);
        emit_prototype_on_shape(ctx, &cdir, &cid, &shape_id, &refused_l);
        for &(pcid, shape_id) in &group.class.chain[1..] {
            emit_prototype_on_shape(
                ctx,
                &cdir,
                &pcid.to_string(),
                &(shape_id as i32).to_string(),
                &refused_l,
            );
        }
        ctx.block().br(&proved_l);
    }
    Some((proved, refused))
}

/// `(i64 class id, i32 ShapeId)` of the first prototype each of `firsts`'
/// predecessor blocks passes (`(label, class id, ShapeId)`): constants for
/// one, phis for several.
fn emit_first_prototype(
    ctx: &mut crate::expr::FnCtx<'_>,
    firsts: &[(String, u32, u32)],
) -> (String, String) {
    if let [(_, cid, shape_id)] = firsts {
        return (cid.to_string(), (*shape_id as i32).to_string());
    }
    let cids: Vec<(String, &str)> = firsts
        .iter()
        .map(|(l, cid, _)| (cid.to_string(), l.as_str()))
        .collect();
    let shapes: Vec<(String, &str)> = firsts
        .iter()
        .map(|(l, _, shape_id)| ((*shape_id as i32).to_string(), l.as_str()))
        .collect();
    let blk = ctx.block();
    let cid = blk.phi(
        I64,
        &cids
            .iter()
            .map(|(v, l)| (v.as_str(), *l))
            .collect::<Vec<_>>(),
    );
    let shape_id = blk.phi(
        I32,
        &shapes
            .iter()
            .map(|(v, l)| (v.as_str(), *l))
            .collect::<Vec<_>>(),
    );
    (cid, shape_id)
}

/// Continue in a new block when the agent's `%Object.prototype%` (the cell at
/// `dir`'s `proto_cell`; `dir` the agent's shape directory) is a live object
/// whose layout word is unshadowed (no key added, deleted or redefined since
/// its birth: its one accessor is `__proto__`), else to `refused`.
fn emit_object_prototype_unwritten(ctx: &mut crate::expr::FnCtx<'_>, dir: &str, refused_l: &str) {
    let cell_b = ctx.new_block("crun.oproto");
    let live_b = ctx.new_block("crun.oproto_live");
    let word_b = ctx.new_block("crun.oproto_word");
    let done = ctx.new_block("crun.oproto_ok");
    let cell_l = ctx.block_label(cell_b);
    let live_l = ctx.block_label(live_b);
    let word_l = ctx.block_label(word_b);
    let done_l = ctx.block_label(done);
    let blk = ctx.block();
    let at = blk.gep(
        I8,
        dir,
        &[(
            I64,
            &(crate::runtime_abi::SHAPE_DIR_FLAT_BASE + 16).to_string(),
        )],
    );
    let cell = blk.load(PTR, &at);
    let present = blk.icmp_ne(PTR, &cell, "null");
    blk.cond_br(&present, &cell_l, refused_l);
    ctx.current_block = cell_b;
    let blk = ctx.block();
    let p = blk.load(I64, &cell);
    // A pointer above the handle band (the empty cell is `usize::MAX`).
    let floor = crate::runtime_abi::RECEIVER_HANDLE_FLOOR as u64;
    let rel = blk.sub(I64, &p, &floor.to_string());
    let ordinary = blk.icmp_ult(I64, &rel, &((1u64 << 47) - floor).to_string());
    blk.cond_br(&ordinary, &live_l, refused_l);
    ctx.current_block = live_b;
    let blk = ctx.block();
    // Not a forwarded copy (`GcHeader::gc_flags & GC_FLAG_FORWARDED`).
    let fa = blk.sub(
        I64,
        &p,
        &(crate::runtime_abi::GC_HEADER_SIZE as i64 - 1).to_string(),
    );
    let fp = blk.inttoptr(I64, &fa);
    let gc_flags = blk.load(I8, &fp);
    let forwarded = blk.and(
        I8,
        &gc_flags,
        &crate::runtime_abi::GC_FLAG_FORWARDED.to_string(),
    );
    let live = blk.icmp_eq(I8, &forwarded, "0");
    blk.cond_br(&live, &word_l, refused_l);
    ctx.current_block = word_b;
    let blk = ctx.block();
    let wa = blk.add(I64, &p, "4");
    let wp = blk.inttoptr(I64, &wa);
    let word = blk.load(I32, &wp);
    let unshadowed = emit_word_unshadowed(blk, &word);
    blk.cond_br(&unshadowed, &done_l, refused_l);
    ctx.current_block = done;
}
