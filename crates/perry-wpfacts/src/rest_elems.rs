//! I32 at rest, PR B (representation RFC §10.7; DESIGN-n1-at-rest §3): the
//! I32 element storage kind of an Array template.
//!
//! An Array template is **at rest** when every element slot of every Array
//! it births holds its element as an I32 word, `INT32_TAG | (u32)v`, for the
//! Array's whole life. The slot stores its node's representation (owner
//! ruling 2026-11-10): the element node is I32 (`ElemRep::I32`, int32
//! Numbers with int demand), so the code that reads the slot wants an i32,
//! and a double slot would cost a conversion at every read.
//!
//! **Admission** (`admit`, a greatest fixpoint over the solved graph). A
//! template is at rest when:
//! - it is a fused fill birth `new Array(k).fill(v)` (`Model::rest_fills`):
//!   the one birth whose element words codegen encodes, right after the
//!   native fill returns (`codegen expr::rest_elems`);
//! - every token is a plain program Array (`bindings::arr_rep`'s refusals:
//!   no marker, not foreign, escaped or hard, no metadata write, not in a
//!   unit's cell), with an I32 element rep, a fixed length, dense below a
//!   floor > 0 (its storage never grows or moves and has no hole), and not
//!   immutable (an immutable Array is N3's, read as constants);
//! - no generic reader touches a token (**the use record**, `uses`): every
//!   constraint on a node that holds one, and every constraint naming such a
//!   node as the object it stores into, is one codegen lowers with the
//!   encoding (an element read `a[k]`, whose result node is an `IndexGet`'s
//!   own, `Gen::element_values`; an element store `a[k] = v`, whose store
//!   site is an `IndexSet`'s, desc `[]=`; `length`; the fused fill on the
//!   birth's own receiver), or one that moves no element word (an identity
//!   test, a filter, a guard, a result relay, a throw region, the hole and
//!   length bookkeeping of an index store). Every other constraint is a use:
//!   a builtin method, a protocol touch (JSON, inspect, ToPrimitive,
//!   iteration, for-of), spread, own values, a native argument, a destructuring
//!   read, `Object.*`, an own-property guard, a call. That is the inverted
//!   default: an operation nobody listed is a use. The solver also records
//!   the uses it makes itself (`Model::rest_used`: `implicit`,
//!   `array_method`, `spread_into`, `own_values_of`);
//! - **co-occurrence**: every element site (every copy of it, and every site
//!   at the same `element_offset`, which codegen keys by) whose receiver may
//!   hold a token of the template holds only tokens of at-rest templates
//!   (no primitive, no ⊤, no other Array). A site is then lowered with the
//!   encoding or not at all, never with a runtime kind test.
//!
//! A refusal of one template can refuse another (a site shared with it), so
//! the co-occurrence step runs to its fixpoint. A non-int write cannot
//! exist: it would join the element node, the elem rep would leave I32 and
//! the template the fixpoint; there is no runtime transition on proven code.
//!
//! **Export** (`export`): per module, the element reads and stores by
//! `element_offset` whose receivers hold only at-rest Arrays, with the least
//! floor; the `fill` call offsets of at-rest births. A field read call (b1)
//! carries its body read's own verdict (`site_floor`).
//!
//! `PERRY_REST_ELEMS=0` (also `off`, `false`) admits nothing: main's facts.
//! Negative control `PERRY_FACTS_SABOTAGE=restelem`: a builtin method and the
//! stores it makes are no use (`builtins::collections::array_method`,
//! `Cons::MCall`, `Cons::Store`), so an Array a native `fill` writes with
//! plain doubles may be admitted.

use crate::bindings::{owner_metadata_written, tok_arr_rep, ElemRep, ModuleBindings};
use crate::gen::Gen;
use crate::model::{Cons, Model, NodeId, SiteId, ThisArg, TmplId, TmplKind, Tok, TOP};
use crate::pts::{FxHashMap, FxHashSet};

/// `PERRY_REST_ELEMS=0` turns the storage kind off.
pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_REST_ELEMS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// A fused fill birth `new Array(k).fill(v)` (`Gen::call_with`): the
/// module, the `fill` call's offset (codegen's key for the call), the
/// fill's receiver node and the `new Array` call's source site.
#[derive(Clone, Copy, Debug)]
pub struct RestFill {
    pub module: u32,
    pub offset: u32,
    pub recv: NodeId,
    pub site: SiteId,
}

impl Model {
    /// A generic reader may touch the elements of `t` undecoded.
    pub(crate) fn rest_used(&mut self, t: Tok) {
        if self.rest_elems && t != TOP && self.is_arr(t) {
            self.arr_used.insert(t);
        }
    }
}

/// `call` is the `fill` of a fused fill birth, on the birth's receiver.
pub(crate) fn fused_fill(m: &Model, call: crate::model::CallId) -> bool {
    match m.calls.get(call as usize).map(|c| &c.this) {
        Some(ThisArg::Node(n)) => m.rest_fills.iter().any(|f| f.recv == *n),
        _ => false,
    }
}

/// Every copy a summarized body's node has (`Inst::fresh`), by original.
fn node_copies(m: &Model) -> FxHashMap<NodeId, Vec<NodeId>> {
    let mut copies: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
    for inst in 0..m.sum.insts.len() as u32 {
        m.inst_copies(inst, |orig, copy| {
            copies.entry(orig).or_default().push(copy)
        });
    }
    copies
}

/// Why token `t` of template `tm` cannot be stored at rest, if it cannot.
fn tok_refusal(m: &Model, t: Tok, tm: TmplId) -> Option<&'static str> {
    if m.is_marker(t) || m.bare_marker(t) || m.beside_values(t) {
        return Some("marker");
    }
    if m.foreign.contains_key(&tm) || m.is_escaped(t) || m.hard.contains_key(&t) {
        return Some("escaped");
    }
    if owner_metadata_written(m, tm) {
        return Some("metadata");
    }
    if m.cell_arrs.contains_key(&t) {
        return Some("unit cell");
    }
    if m.arr_used.contains(&t) {
        return Some("generic reader");
    }
    let r = tok_arr_rep(m, t, tm, true);
    if r.elem != ElemRep::I32 {
        let Some(&e) = m.all_fields.get(&t) else {
            return Some("elem not a number");
        };
        let e = &m.nodes[e as usize];
        return Some(if e.prim != crate::model::prim::NUM || !e.pts.is_empty() {
            "elem not a number"
        } else if !e.int_demand {
            "elem without demand"
        } else {
            "elem not int32"
        });
    }
    if !r.fixed {
        return Some("length moves");
    }
    if !r.dense || r.floor == 0 {
        return Some("not dense");
    }
    if r.im {
        return Some("immutable");
    }
    None
}

/// What decides an element site: every receiver node (with its copies).
type Sites = std::collections::BTreeMap<(u32, u32, bool), Vec<NodeId>>;

fn sites(g: &Gen<'_>, copies: &FxHashMap<NodeId, Vec<NodeId>>) -> Sites {
    let mut out = Sites::new();
    for s in &g.element_sites {
        let write = s.op == crate::derive::ElementOp::Write;
        let v = out.entry((s.module, s.byte_offset, write)).or_default();
        v.push(s.recv);
        v.extend(copies.get(&s.recv).into_iter().flatten().copied());
    }
    out
}

/// An `IndexSet`'s element store (`Gen::store_elem` desc `[]=`).
fn index_store(m: &Model, site: SiteId) -> bool {
    m.stores.get(site as usize).is_some_and(|s| s.desc == "[]=")
}

/// The constraint `c` on a node holding an Array, when it is a use
/// (module docs): its kind.
fn use_of(m: &Model, c: &Cons, fill_recv: bool, reads: &FxHashSet<NodeId>) -> Option<&'static str> {
    let ok = match *c {
        Cons::Load { key, dst } => {
            matches!(key, crate::model::Key::Named(a) if m.atoms[a as usize] == "length")
                || reads.contains(&dst)
        }
        Cons::IdxLoad { dst, .. } | Cons::IdxOwnLoad { dst, .. } => reads.contains(&dst),
        Cons::Store { site, .. } => index_store(m, site) || m.rest_elem_sabotage,
        Cons::MCall { .. } => fill_recv || m.rest_elem_sabotage,
        Cons::Identity { .. }
        | Cons::Raise { .. }
        | Cons::IndexHole
        | Cons::LenIndex { .. }
        | Cons::Guard { .. }
        | Cons::Defined { .. }
        | Cons::NotNullish { .. }
        | Cons::NotUndefined { .. }
        | Cons::Falsy { .. }
        | Cons::RetInto { .. }
        | Cons::ArgInto { .. } => true,
        // The fused fill's receiver: the birth and its native fill.
        _ => fill_recv,
    };
    (!ok).then(|| c.kind_name())
}

/// The node a constraint elsewhere names as the object it writes or guards,
/// when that is a use (an `IndexSet`'s indexed store is none).
fn object_use(m: &Model, c: &Cons) -> Option<NodeId> {
    match *c {
        Cons::StoreIndexed { o, site, .. } => (!index_store(m, site)).then_some(o),
        Cons::StoreKeyed { o, .. }
        | Cons::DefineKeyed { o, .. }
        | Cons::DefineAll { o, .. }
        | Cons::DefineDesc { o, .. } => Some(o),
        Cons::OwnGate { obj, .. } => Some(obj),
        Cons::Accessor { holder, .. } | Cons::AccessorKeyed { holder, .. } => Some(holder),
        Cons::SuperFrom { recv, .. } | Cons::ThisFrom { recv } => Some(recv),
        _ => None,
    }
}

/// The templates of `cand` node `n` holds.
fn held(m: &Model, n: NodeId, cand: &FxHashSet<TmplId>, buf: &mut Vec<Tok>) -> Vec<TmplId> {
    let node = &m.nodes[n as usize];
    if node.pts.is_empty() {
        return Vec::new();
    }
    node.pts.sorted_into(buf);
    let mut out: Vec<TmplId> = buf
        .iter()
        .filter_map(|&t| m.tmpl_of(t).filter(|tm| cand.contains(tm)))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Node `n` holds only tokens of `cand` (or nothing).
fn only(m: &Model, n: NodeId, cand: &FxHashSet<TmplId>, buf: &mut Vec<Tok>) -> bool {
    let node = &m.nodes[n as usize];
    if node.prim != 0 {
        return false;
    }
    node.pts.sorted_into(buf);
    buf.iter()
        .all(|&t| t != TOP && m.tmpl_of(t).is_some_and(|tm| cand.contains(&tm)))
}

/// The templates admitted at rest, and why each other candidate was refused
/// (module docs). `interpreted`: interpreted code can run.
pub(crate) fn admit(
    g: &Gen<'_>,
    interpreted: bool,
) -> (FxHashSet<TmplId>, FxHashMap<TmplId, &'static str>) {
    let m = &g.m;
    let mut refused: FxHashMap<TmplId, &'static str> = FxHashMap::default();
    let mut cand: FxHashSet<TmplId> = FxHashSet::default();
    if !m.rest_elems || !m.array_reps || m.aborted || m.unit_mode || m.rest_fills.is_empty() {
        return (cand, refused);
    }
    // The plain `new Array` template of each fill's site (`builtins::
    // site_tmpl_born`); a fill without an offset codegen can key is none.
    let mut births: Vec<TmplId> = m
        .rest_fills
        .iter()
        .filter_map(|f| m.site_tmpls.get(&(f.site, "Array", true)))
        .filter_map(|&t| m.tmpl_of(t))
        .collect();
    births.sort_unstable();
    births.dedup();
    for tm in births {
        if !matches!(m.tmpls[tm as usize].kind, TmplKind::Arr { plain: true, .. }) {
            continue;
        }
        if m.rest_fills.iter().any(|f| {
            f.offset == 0
                && m.site_tmpls
                    .get(&(f.site, "Array", true))
                    .and_then(|&t| m.tmpl_of(t))
                    == Some(tm)
        }) {
            refused.insert(tm, "no offset");
            continue;
        }
        // A token the fill wrote has an element node; one with none (the
        // template's own token beside its birth-site tokens) holds no Array.
        let mut toks = m.tmpl_toks(tm);
        toks.retain(|t| m.all_fields.contains_key(t));
        let why = if interpreted {
            Some("interpreter")
        } else if toks.is_empty() {
            Some("no token")
        } else {
            toks.iter().find_map(|&t| tok_refusal(m, t, tm))
        };
        match why {
            Some(w) => {
                refused.insert(tm, w);
            }
            None => {
                cand.insert(tm);
            }
        }
    }
    if cand.is_empty() {
        return (cand, refused);
    }
    let copies = node_copies(m);
    let mut fill_recvs: FxHashSet<NodeId> = FxHashSet::default();
    for f in &m.rest_fills {
        fill_recvs.insert(f.recv);
        fill_recvs.extend(copies.get(&f.recv).into_iter().flatten().copied());
    }
    let mut reads: FxHashSet<NodeId> = FxHashSet::default();
    for &(_, offset, n) in &g.element_values {
        if offset != 0 {
            reads.insert(n);
            reads.extend(copies.get(&n).into_iter().flatten().copied());
        }
    }
    let mut buf = Vec::new();
    let mut refuse = |cand: &mut FxHashSet<TmplId>, tms: Vec<TmplId>, why: &'static str| {
        for tm in tms {
            if cand.remove(&tm) {
                refused.insert(tm, why);
            }
        }
    };
    // The use record (module docs).
    for (n, node) in m.nodes.iter().enumerate() {
        let n = n as NodeId;
        for c in &node.cons {
            if let Some(o) = object_use(m, c) {
                let hit = held(m, o, &cand, &mut buf);
                refuse(&mut cand, hit, c.kind_name());
            }
        }
        if node.cons.is_empty() {
            continue;
        }
        let hit = held(m, n, &cand, &mut buf);
        if hit.is_empty() {
            continue;
        }
        let fill_recv = fill_recvs.contains(&n);
        if let Some(why) = node
            .cons
            .iter()
            .find_map(|c| use_of(m, c, fill_recv, &reads))
        {
            refuse(&mut cand, hit, why);
        }
    }
    // Co-occurrence, to the fixpoint.
    let sites = sites(g, &copies);
    loop {
        let mut changed = false;
        for (&(_, offset, _), recvs) in &sites {
            let hit: Vec<TmplId> = recvs
                .iter()
                .flat_map(|&r| held(m, r, &cand, &mut buf))
                .collect();
            if hit.is_empty() {
                continue;
            }
            if offset != 0 && recvs.iter().all(|&r| only(m, r, &cand, &mut buf)) {
                continue;
            }
            refuse(&mut cand, hit, "mixed site");
            changed = true;
        }
        if !changed {
            break;
        }
    }
    (cand, refused)
}

/// The least floor of what `recvs` hold, when every one holds only Arrays
/// of at-rest templates and one holds one (`Model::rest_tmpls`).
pub(crate) fn site_floor(m: &Model, recvs: &[NodeId]) -> Option<u32> {
    if m.rest_tmpls.is_empty() {
        return None;
    }
    let mut buf = Vec::new();
    let mut floor: Option<u32> = None;
    for &r in recvs {
        if !only(m, r, &m.rest_tmpls, &mut buf) {
            return None;
        }
        m.nodes[r as usize].pts.sorted_into(&mut buf);
        for &t in &buf {
            let tm = m.tmpl_of(t)?;
            let f = tok_arr_rep(m, t, tm, true).floor;
            floor = Some(floor.map_or(f, |x| x.min(f)));
        }
    }
    floor
}

/// Fill `ModuleBindings::rest_reads`, `rest_writes` and `rest_births`
/// (module docs).
pub(crate) fn export(g: &Gen<'_>, bindings: &mut [ModuleBindings]) {
    let m = &g.m;
    if m.rest_tmpls.is_empty() {
        return;
    }
    let copies = node_copies(m);
    for ((mi, offset, write), recvs) in sites(g, &copies) {
        let Some(mb) = bindings.get_mut(mi as usize) else {
            continue;
        };
        if offset == 0 {
            continue;
        }
        if let Some(floor) = site_floor(m, &recvs) {
            if write {
                mb.rest_writes.push((offset, floor));
            } else {
                mb.rest_reads.push((offset, floor));
            }
        }
    }
    for f in &m.rest_fills {
        let mut recvs = vec![f.recv];
        recvs.extend(copies.get(&f.recv).into_iter().flatten().copied());
        if site_floor(m, &recvs).is_some() {
            if let Some(mb) = bindings.get_mut(f.module as usize) {
                mb.rest_births.push(f.offset);
            }
        }
    }
    for mb in bindings.iter_mut() {
        mb.rest_reads.sort_unstable();
        mb.rest_reads.dedup();
        mb.rest_writes.sort_unstable();
        mb.rest_writes.dedup();
        mb.rest_births.sort_unstable();
        mb.rest_births.dedup();
    }
}

/// The probe (`PERRY_FACTS_DUMP`): one `# rest arr` line per fused fill
/// template, at rest or with the reason it is not.
pub(crate) fn probe(m: &Model) -> Vec<String> {
    let mut out = Vec::new();
    for (tm, tpl) in m.tmpls.iter().enumerate() {
        let tm = tm as TmplId;
        let what = tpl.display.replace(' ', "_");
        if m.rest_tmpls.contains(&tm) {
            out.push(format!("# rest arr {what} at_rest=1"));
        } else if let Some(why) = m.rest_refused.get(&tm) {
            out.push(format!(
                "# rest arr {what} at_rest=0 why={}",
                why.replace(' ', "_")
            ));
        }
    }
    out
}
