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

/// Must a binding site test the shadow bits (a dynamic write or a relink
/// exists somewhere in the program)?
pub(crate) fn shadow_tests() -> bool {
    with_census(perry_hir::ProtoCensus::shadow_reachable).unwrap_or(true)
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
    let end = crate::runtime_abi::UNSHADOWED_WORD_END as i32;
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
pub(crate) fn holder_constant(ctx: &mut crate::expr::FnCtx<'_>, class: &str, method: &str) -> String {
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
    let ok32 = if ok == "true" { "1".to_string() } else { blk.zext(I1, ok, I32) };
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
