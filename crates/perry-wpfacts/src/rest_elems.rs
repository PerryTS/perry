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
//!   unit's cell), with an I32 element rep, a fixed length or a reserved
//!   capacity (below), dense below a floor > 0 (its storage never grows or
//!   moves and has no hole below the floor), and not immutable (an immutable
//!   Array is N3's, read as constants);
//! - no generic reader touches a token (**the use record**, `uses`): every
//!   constraint on a node that holds one, and every constraint naming such a
//!   node as the object it stores into, is one codegen lowers with the
//!   encoding (an element read `a[k]`, whose result node is an `IndexGet`'s
//!   own, `Gen::element_values`; an element store `a[k] = v`, whose store
//!   site is an `IndexSet`'s or a `PutValueSet`'s; `length`; the fused fill on the
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
//! **The capacity reservation** (`reserve`; owner decision 2026-11-11). A
//! length that moves is still admitted when its hull over the template's
//! Arrays is finite and small: `0 < lo`, `hi <= 2 * lo` and `hi <=
//! REST_CAP`. The birth then reserves `hi` slots (`rest_reserves`, the
//! runtime's `js_array_rest_reserve`), so every index store the facts allow
//! (its key is below `hi`, `solve::arr_len`) lands below the capacity and
//! the runtime lengthens the Array where it stands: the storage never grows,
//! so it is never reallocated or forwarded, and a slot below the floor `lo`
//! keeps its address. Slots in `[lo, hi)` may be holes (`[length, capacity)`
//! is hole-filled, and a store past the length leaves a gap), so only the
//! floor is dense, as for any Array. The two bounds: the slack is at most the
//! dense part (one runtime growth step doubles as well), and the reserved
//! cell stays a small young cell (at most 8 KiB of slots), never a large
//! object. `ArrRep::fixed` is unchanged; codegen reads "never moves" as
//! `fixed || at_rest`. `PERRY_REST_CAPACITY=0` (also `off`, `false`) admits
//! no moving length: main's facts.
//!
//! `PERRY_REST_ELEMS=0` (also `off`, `false`) admits nothing: main's facts.
//! Negative control `PERRY_FACTS_SABOTAGE=restelem`: a builtin method and the
//! stores it makes are no use (`builtins::collections::array_method`,
//! `Cons::MCall`, `Cons::Store`), so an Array a native `fill` writes with
//! plain doubles may be admitted.

use crate::bindings::{owner_metadata_written, tok_arr_rep, ElemRep, ModuleBindings};
use crate::gen::Gen;
use crate::model::{Cons, Model, NodeId, SiteId, TmplId, TmplKind, Tok, TOP};
use crate::pts::{FxHashMap, FxHashSet};

/// `PERRY_REST_ELEMS=0` turns the storage kind off.
pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_REST_ELEMS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `PERRY_REST_CAPACITY=0` turns the capacity reservation off.
pub(crate) fn capacity_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_REST_CAPACITY").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// The most slots an at-rest birth reserves (module docs).
pub(crate) const REST_CAP: i64 = 1024;

/// The capacity the births of template `tm` reserve (module docs):
/// `Ok(None)` when every Array of it keeps its length (nothing to reserve),
/// `Ok(Some(hi))` when the length hull allows the reservation, else the
/// refusal.
pub(crate) fn reserve(m: &Model, tm: TmplId) -> Result<Option<u32>, &'static str> {
    let mut hull = crate::model::Iv::EMPTY;
    let mut fixed = true;
    for t in m.tmpl_toks(tm) {
        if !m.all_fields.contains_key(&t) {
            continue;
        }
        let len = match m.arr_len.get(&t) {
            Some(&n) => m.nodes[n as usize].range,
            None => match m.arr_birth_len.get(&tm) {
                Some(&k) => crate::model::Iv::int(k, k),
                None => crate::model::Iv::TOP,
            },
        };
        if len.is_top() || len.is_empty() {
            return Err("length moves");
        }
        fixed &= len.lo == len.hi;
        hull = hull.join(len);
    }
    if fixed {
        return Ok(None);
    }
    if !m.rest_capacity || hull.is_top() || hull.is_empty() {
        return Err("length moves");
    }
    if hull.lo > 0 && hull.hi <= hull.lo.saturating_mul(2) && hull.hi <= REST_CAP {
        Ok(Some(hull.hi as u32))
    } else {
        Err("length over cap")
    }
}

/// A fused fill birth `new Array(k).fill(v)` (`Gen::call_with`): the
/// module, the `fill` call's offset (codegen's key for the call), the
/// fill's receiver node, the `new Array` call's source site and the `fill`
/// call's (every call the solver derives from one shares its site).
#[derive(Clone, Copy, Debug)]
pub struct RestFill {
    pub module: u32,
    pub offset: u32,
    pub recv: NodeId,
    pub site: SiteId,
    pub fill_site: SiteId,
}

impl Model {
    /// A generic reader may touch the elements of `t` undecoded.
    pub(crate) fn rest_used(&mut self, t: Tok) {
        if self.rest_elems && t != TOP && self.is_arr(t) {
            self.arr_used.insert(t);
        }
    }
}

/// `call` is (derived from) the `fill` call of a fused fill birth.
pub(crate) fn fused_fill(m: &Model, call: crate::model::CallId) -> bool {
    m.calls
        .get(call as usize)
        .is_some_and(|c| m.rest_fills.iter().any(|f| f.fill_site == c.site))
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
        return Some(if r.elem == ElemRep::Boxed {
            "elem not a number"
        } else if !crate::bindings::elem_demand(m, t, e) {
            "elem without demand"
        } else {
            "elem not int32"
        });
    }
    if !r.fixed {
        // A moving length needs the capacity reservation (module docs).
        if let Err(why) = reserve(m, tm) {
            return Some(why);
        }
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

/// An element store codegen lowers at its site: an `IndexSet`'s
/// (`Gen::store_elem` desc `[]=`) or a `PutValueSet`'s (into its target and
/// its receiver, one evaluation of the same base).
fn index_store(m: &Model, site: SiteId) -> bool {
    m.stores
        .get(site as usize)
        .is_some_and(|s| matches!(s.desc.as_str(), "[]=" | "putvalue" | "putvalue-receiver"))
}

/// The constraint `c` on a node holding an Array, when it is a use
/// (module docs): its kind.
fn use_of(m: &Model, c: &Cons, fill_recv: bool, reads: &FxHashSet<NodeId>) -> Option<String> {
    let ok = match *c {
        Cons::Load { key, dst } => {
            matches!(key, crate::model::Key::Named(a) if m.atoms[a as usize] == "length")
                || reads.contains(&dst)
        }
        Cons::IdxLoad { dst, .. } | Cons::IdxOwnLoad { dst, .. } => reads.contains(&dst),
        Cons::Store { site, .. } => index_store(m, site) || m.rest_elem_sabotage,
        Cons::MCall { call, .. } => fill_recv || fused_fill(m, call) || m.rest_elem_sabotage,
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
        // A callee: calling or constructing an Array throws before any
        // element is read (the `new Array` holder holds what it births).
        Cons::Call { .. } | Cons::New { .. } | Cons::NewNamed { .. } => true,
        // The prototype lookup a load, a method call or a protocol touch
        // continues on: the root on the holder is what decides (and a
        // builtin method or protocol records its use, `Model::rest_used`).
        Cons::Chain { .. }
        | Cons::ChainUnion { .. }
        | Cons::PlainKey { .. }
        | Cons::ProtoMethods { .. }
        | Cons::AnyAccessors { .. }
        | Cons::HolderStep { .. }
        | Cons::VisitHolders { .. }
        | Cons::VisitStop { .. }
        | Cons::ProtoClosure { .. } => true,
        // The fused fill's receiver: the birth and its native fill.
        _ => fill_recv,
    };
    (!ok).then(|| match *c {
        Cons::Store { site, .. } => {
            format!(
                "store {}",
                m.stores.get(site as usize).map_or("", |s| s.desc.as_str())
            )
        }
        Cons::MCall { key, .. } => format!("mcall {}", m.key_name(key)),
        _ => c.kind_name().to_string(),
    })
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
    buf.clear();
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
    buf.clear();
    node.pts.sorted_into(buf);
    buf.iter()
        .all(|&t| t != TOP && m.tmpl_of(t).is_some_and(|tm| cand.contains(&tm)))
}

/// The templates admitted at rest, and why each other candidate was refused
/// (module docs). `interpreted`: interpreted code can run.
pub(crate) fn admit(
    g: &Gen<'_>,
    interpreted: bool,
) -> (FxHashSet<TmplId>, FxHashMap<TmplId, String>) {
    let m = &g.m;
    let mut refused: FxHashMap<TmplId, String> = FxHashMap::default();
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
            refused.insert(tm, "no offset".to_string());
            continue;
        }
        // A token the fill wrote has an element node; one with none (the
        // template's own token beside its birth-site tokens) holds no Array.
        let mut toks = m.tmpl_toks(tm);
        toks.retain(|t| m.all_fields.contains_key(t));
        let refusal = if interpreted {
            Some("interpreter")
        } else if toks.is_empty() {
            Some("no token")
        } else {
            toks.iter().find_map(|&t| tok_refusal(m, t, tm))
        };
        match refusal {
            Some(w) => {
                refused.insert(tm, w.to_string());
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
    fill_recvs.extend(m.rest_fill_relays.iter().copied());
    let mut reads: FxHashSet<NodeId> = FxHashSet::default();
    for &(_, offset, n) in &g.element_values {
        if offset != 0 {
            reads.insert(n);
            reads.extend(copies.get(&n).into_iter().flatten().copied());
        }
    }
    let mut buf = Vec::new();
    let mut refuse = |cand: &mut FxHashSet<TmplId>, tms: Vec<TmplId>, why: &str| {
        for tm in tms {
            if cand.remove(&tm) {
                refused.insert(tm, why.to_string());
            }
        }
    };
    // The use record (module docs).
    for (n, node) in m.nodes.iter().enumerate() {
        let n = n as NodeId;
        for c in &node.cons {
            if let Some(o) = object_use(m, c).filter(|o| !fill_recvs.contains(o)) {
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
        if let Some(kind) = node
            .cons
            .iter()
            .find_map(|c| use_of(m, c, fill_recv, &reads))
        {
            let on = m.labels.get(n as usize).copied().unwrap_or("?");
            refuse(&mut cand, hit, &format!("{kind} on {on}"));
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
        buf.clear();
        m.nodes[r as usize].pts.sorted_into(&mut buf);
        for &t in &buf {
            let tm = m.tmpl_of(t)?;
            let f = tok_arr_rep(m, t, tm, true).floor;
            floor = Some(floor.map_or(f, |x| x.min(f)));
        }
    }
    floor
}

/// I32 at rest B2: `derive` gives each field its representation before the
/// at-rest admission runs (the admission reads element reps the demand
/// settles), so an `Arr` field rep learns its storage kind here. It is at
/// rest iff every Array token the field's nodes hold is of an at-rest
/// template, as `tok_arr_rep` joins it for a binding (co-occurrence makes a
/// node all-or-none). Codegen reads it as `FactsArrRep::at_rest`: the
/// rep-keyed loads and the held bases of a field decode the I32 word.
pub(crate) fn mark_fields(m: &Model, templates: &mut [crate::derive::TemplateFacts]) {
    // `PERRY_REST_TIERS=0` (codegen `expr::rest_elems::tiers_enabled`) keeps
    // PR B's field reps.
    let off = matches!(
        std::env::var("PERRY_REST_TIERS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    );
    if m.rest_tmpls.is_empty() || off {
        return;
    }
    for tf in templates.iter_mut() {
        let ts = m.tmpl_toks(tf.id);
        for f in tf.fields.iter_mut() {
            let crate::bindings::Rep::Arr(r) = &mut f.rep else {
                continue;
            };
            let atoms = if f.private {
                &m.private_atoms
            } else {
                &m.atom_index
            };
            let Some(&a) = atoms.get(&f.key) else {
                continue;
            };
            let (mut any, mut all) = (false, true);
            for n in ts.iter().filter_map(|t| m.fields.get(&(*t, a))) {
                for t in m.nodes[*n as usize].pts.to_sorted() {
                    match m.toks[t as usize] {
                        crate::model::TokKind::Obj(tm, _) if m.rest_tmpls.contains(&tm) => {
                            any = true
                        }
                        _ => all = false,
                    }
                }
            }
            r.at_rest = any && all;
        }
    }
}

/// Fill `ModuleBindings::rest_reads`, `rest_writes`, `rest_births` and
/// `rest_reserves` (module docs).
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
                // The capacity this birth reserves: its own template's.
                let cap = m
                    .site_tmpls
                    .get(&(f.site, "Array", true))
                    .and_then(|&t| m.tmpl_of(t))
                    .filter(|tm| m.rest_tmpls.contains(tm))
                    .and_then(|tm| reserve(m, tm).ok().flatten());
                if let Some(cap) = cap {
                    mb.rest_reserves.push((f.offset, cap));
                }
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
        mb.rest_reserves.sort_unstable();
        mb.rest_reserves.dedup();
    }
}

/// The probe (`PERRY_FACTS_DUMP`): one `# rest arr` line per fused fill
/// template, at rest (with the capacity its births reserve, if any) or with
/// the reason it is not.
pub(crate) fn probe(m: &Model) -> Vec<String> {
    let mut out = Vec::new();
    for (tm, tpl) in m.tmpls.iter().enumerate() {
        let tm = tm as TmplId;
        let what = tpl.display.replace(' ', "_");
        if m.rest_tmpls.contains(&tm) {
            match reserve(m, tm) {
                Ok(Some(cap)) => out.push(format!("# rest arr {what} at_rest=1 cap={cap}")),
                _ => out.push(format!("# rest arr {what} at_rest=1")),
            }
        } else if let Some(refusal) = m.rest_refused.get(&tm) {
            // The length interval over the template's tokens (`solve::arr_len`).
            let mut len = crate::model::Iv::EMPTY;
            for t in m.tmpl_toks(tm) {
                if let Some(&n) = m.arr_len.get(&t) {
                    len = len.join(m.nodes[n as usize].range);
                }
            }
            let len = if len.is_top() {
                "top".to_string()
            } else if len.is_empty() {
                "birth".to_string()
            } else {
                format!("[{},{}]", len.lo, len.hi)
            };
            out.push(format!(
                "# rest arr {what} at_rest=0 why={} len={len}",
                refusal.replace(' ', "_")
            ));
        }
    }
    out
}
