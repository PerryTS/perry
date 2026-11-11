//! I32 at rest, PR C (representation RFC §10.7; DESIGN-n1-at-rest §4): the
//! I32 field lanes of a class.
//!
//! A class field whose representation is I32 (`FieldFacts::rep`, PR A: a
//! whole-life definite data field holding only int32 Numbers with int
//! demand) stores its value in its slot as an I32 word, `INT32_TAG |
//! (u32)v`, when the class is **admitted** here: the slot stores its node's
//! representation (owner ruling 2026-11-10), so the code that reads the slot
//! takes the i32 as it is and a double slot would cost a conversion at every
//! read and write. Codegen makes the slot an I32 lane of the class's shape
//! (`perry-codegen` `expr::rest_fields`), the runtime reads and writes it as a
//! JS value only through the layout accessor (`perry-runtime` `layout_slot`).
//!
//! **Admission** (`admit`), per class template:
//! - the switch is on (`PERRY_REST_FIELDS`), the solve is whole-program (not
//!   unit mode) and no interpreted code can run;
//! - the template is a class (or object-literal class) that neither escapes
//!   nor is hard, keeps a fixed layout, adds no key after birth, is not a
//!   per-evaluation class, and has no heritage either way: no parent (its
//!   field definitions are the first code that runs on an instance, so the
//!   lanes exist before anything can read them) and no subclass (no other
//!   class's instance runs its definitions);
//! - no generic reader can reach an instance (**the use record**, the
//!   inverted default of PR B's arrays): every constraint on a node that
//!   holds one of the template's tokens is a named read codegen lowers as a
//!   property read (`Gen::field_reads`: a `PropertyGet` or a
//!   `PropertyUpdate`'s read), a named store it lowers as a property store
//!   (`.k=`, `.k++`, a constant-key `PutValueSet`, a field definition, an
//!   object-literal birth), a method call, or one that moves no slot word
//!   (identity, filters, guards, relays, callee position, `this` bindings,
//!   the prototype lookup behind a checked root). Anything else is a use:
//!   a dynamic key, `in`, `Object.*`, spread, JSON, inspect, for-in, a
//!   native, a destructuring read. The solver also records the uses it makes
//!   itself (`Model::rest_used`: `implicit`, `spread_into`, `own_values_of`);
//! - per field: the I32 rep, a definite data field (not declared, not a
//!   reservation, not private) at a birth slot below 32 (the lane word).
//!
//! The admitted fields carry `FieldFacts::rest`; codegen gives them the lane
//! and lowers every site of their keys through `expr::rest_fields`. Proven
//! sites load and store the i32 directly; a site wpfacts does not prove reads
//! and writes through the runtime's complete operations, which decode and
//! encode through the accessor, so no unproven reader sees an I32 word.
//!
//! `PERRY_REST_FIELDS=0` (also `off`, `false`) admits nothing: main's facts.
//! Negative control `PERRY_FACTS_SABOTAGE=restlane`: a field whose rep is
//! F64 (it holds a Number past int32) is admitted as an I32 lane too; check
//! mode's witness at the proven store (`perry-codegen` `expr::rest_fields`)
//! reports the value that is no int32.

use crate::bindings::Rep;
use crate::derive::TemplateFacts;
use crate::gen::Gen;
use crate::model::{Cons, Key, Model, NodeId, TmplId, Tok, TOP};
use crate::pts::{FxHashMap, FxHashSet};

/// `PERRY_REST_FIELDS=0` turns the field lanes off.
pub(crate) fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_REST_FIELDS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

impl Model {
    /// A generic reader may touch the slots of object token `t` undecoded.
    pub(crate) fn rest_obj_used(&mut self, t: Tok) {
        if self.rest_fields && t != TOP && !self.is_arr(t) {
            self.obj_used.insert(t);
        }
    }
}

/// A store whose site codegen lowers as a named property store or birth
/// (module docs), by its descriptor.
fn named_store(m: &Model, site: crate::model::SiteId) -> bool {
    m.stores.get(site as usize).is_some_and(|s| {
        let d = s.desc.as_str();
        d.starts_with("field ")
            || (d.starts_with('.') && (d.ends_with('=') || d.ends_with("++")))
            || (d.starts_with("lit.") && !d.starts_with("lit..."))
            || d == "putvalue"
            || d == "putvalue-receiver"
    })
}

/// The constraint `c` on a node holding an instance, when it is a use
/// (module docs): its kind.
fn use_of(m: &Model, c: &Cons, reads: &FxHashSet<NodeId>) -> Option<String> {
    let ok = match *c {
        Cons::Load { key, dst } => matches!(key, Key::Named(_)) && reads.contains(&dst),
        Cons::Store { key, site, .. } => matches!(key, Key::Named(_)) && named_store(m, site),
        Cons::MCall { .. }
        | Cons::Identity { .. }
        | Cons::Raise { .. }
        | Cons::Guard { .. }
        | Cons::Defined { .. }
        | Cons::NotNullish { .. }
        | Cons::NotUndefined { .. }
        | Cons::Falsy { .. }
        | Cons::RetInto { .. }
        | Cons::ArgInto { .. }
        | Cons::Call { .. }
        | Cons::CallThis { .. }
        | Cons::New { .. }
        | Cons::NewNamed { .. }
        | Cons::ThisFrom { .. }
        | Cons::ThisTo { .. }
        | Cons::ThisInst { .. }
        | Cons::BirthField { .. }
        | Cons::Chain { .. }
        | Cons::ChainUnion { .. }
        | Cons::PlainKey { .. }
        | Cons::ProtoMethods { .. }
        | Cons::AnyAccessors { .. }
        | Cons::HolderStep { .. }
        | Cons::VisitHolders { .. }
        | Cons::VisitStop { .. }
        | Cons::ProtoClosure { .. } => true,
        _ => false,
    };
    (!ok).then(|| match *c {
        Cons::Store { site, .. } => format!(
            "store {}",
            m.stores.get(site as usize).map_or("", |s| s.desc.as_str())
        ),
        Cons::Load { key, .. } => format!("load {}", m.key_name(key)),
        _ => c.kind_name().to_string(),
    })
}

/// The node a constraint elsewhere names as the object it writes or guards
/// (PR B's `object_use`, without the method `this` bindings: a class's own
/// methods run with its instances as `this`).
fn object_use(c: &Cons) -> Option<NodeId> {
    match *c {
        Cons::StoreIndexed { o, .. }
        | Cons::StoreKeyed { o, .. }
        | Cons::DefineKeyed { o, .. }
        | Cons::DefineAll { o, .. }
        | Cons::DefineDesc { o, .. } => Some(o),
        Cons::OwnGate { obj, .. } => Some(obj),
        Cons::Accessor { holder, .. } | Cons::AccessorKeyed { holder, .. } => Some(holder),
        Cons::SuperFrom { recv, .. } => Some(recv),
        _ => None,
    }
}

/// The candidate templates node `n` holds.
fn held(m: &Model, n: NodeId, cand: &FxHashMap<TmplId, ()>, buf: &mut Vec<Tok>) -> Vec<TmplId> {
    let node = &m.nodes[n as usize];
    if node.pts.is_empty() {
        return Vec::new();
    }
    buf.clear();
    node.pts.sorted_into(buf);
    let mut out: Vec<TmplId> = buf
        .iter()
        .filter_map(|&t| m.tmpl_of(t).filter(|tm| cand.contains_key(tm)))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// A field of rep `rep` is a lane candidate: I32, and under the negative
/// control `restlane` also F64 (module docs).
fn lane_rep(m: &Model, rep: Rep) -> bool {
    rep == Rep::I32 || (m.rest_lane_sabotage && rep == Rep::F64)
}

/// The class declaration of a class template.
fn hir_class<'a>(g: &Gen<'a>, t: &TemplateFacts) -> Option<&'a perry_hir::Class> {
    let (module, class_id) = match g.m.tmpls[t.id as usize].kind {
        crate::model::TmplKind::Class {
            module, class_id, ..
        } => (module, class_id),
        _ => return None,
    };
    let (_, hir) = g.modules.get(module as usize)?;
    hir.classes.iter().find(|c| c.id == class_id)
}

/// Could evaluating `e` observe `this` (a read, a call on it, a capture)?
fn observes_this(e: &perry_hir::Expr) -> bool {
    use perry_hir::Expr;
    if matches!(
        e,
        Expr::This
            | Expr::SuperCall(_)
            | Expr::SuperCallSpread(_)
            | Expr::SuperMethodCall { .. }
            | Expr::SuperMethodCallSpread { .. }
            | Expr::SuperPropertyGet { .. }
            | Expr::SuperPropertySet { .. }
            | Expr::Closure {
                captures_this: true,
                ..
            }
    ) {
        return true;
    }
    let mut found = false;
    perry_hir::walker::walk_expr_children(e, &mut |c: &Expr| found = found || observes_this(c));
    found
}

/// Why the declaration of class `c` keeps it off I32 lanes: codegen gives a
/// class its lanes at birth (an object-literal class, born with its keys) or
/// right after its own field definitions (any other class), so none of its
/// fields may be computed, private or decorated, the class may carry no
/// decorator, and its field initializers may not observe `this` (no code may
/// read a field before the lanes exist).
fn decl_refusal(c: &perry_hir::Class) -> Option<&'static str> {
    if c.is_imported_stub() || !c.decorators.is_empty() || c.native_extends.is_some() {
        return Some("declaration");
    }
    if c.fields
        .iter()
        .any(|f| f.key_expr.is_some() || f.is_private || !f.decorators.is_empty())
    {
        return Some("computed, private or decorated field");
    }
    if c.fields
        .iter()
        .any(|f| f.init.as_ref().is_some_and(observes_this))
    {
        return Some("initializer observes this");
    }
    None
}

/// Why template `t` cannot have I32 lanes, if it cannot (module docs).
fn tmpl_refusal(
    g: &Gen<'_>,
    t: &TemplateFacts,
    extended: &FxHashSet<u32>,
    interpreted: bool,
) -> Option<&'static str> {
    let m = &g.m;
    if interpreted {
        return Some("interpreter");
    }
    if t.escape.is_some() {
        return Some("escaped");
    }
    if t.hard.is_some() {
        return Some("hard");
    }
    if !t.prefix_stable {
        return Some("not prefix stable");
    }
    if !t.fixed_layout {
        return Some("layout moves");
    }
    if t.adds_keys != 0 {
        return Some("adds keys");
    }
    if m.tmpls[t.id as usize].per_evaluation {
        return Some("per evaluation");
    }
    if !t.ancestors.is_empty() {
        return Some("extends");
    }
    if t.class_id.is_some_and(|c| extended.contains(&c)) {
        return Some("extended");
    }
    match hir_class(g, t) {
        None => Some("no declaration"),
        Some(c) if c.extends.is_some() || c.extends_name.is_some() || c.extends_expr.is_some() => {
            Some("extends")
        }
        Some(c) => decl_refusal(c),
    }
}

/// Why I32-rep field `f` cannot be a lane of an admitted template.
fn field_refusal(f: &crate::derive::FieldFacts) -> Option<&'static str> {
    if f.private {
        return Some("private");
    }
    // Codegen decides every site of a lane key by the key (`expr::
    // rest_fields`): a key builtins have too would send their sites there.
    if crate::builtins::members::builtin_member_name(&f.key) {
        return Some("builtin member name");
    }
    if !f.definite || f.declared || f.reserved || !f.data_field {
        return Some("not a definite data field");
    }
    match f.slot {
        Some(s) if s < 32 => None,
        Some(_) => Some("slot past the lane word"),
        None => Some("no birth slot"),
    }
}

/// Admit the I32 field lanes (module docs): set `FieldFacts::rest` on every
/// admitted field. Returns the probe lines (`# rest lane`), one per I32-rep
/// field of a class template.
pub(crate) fn admit(
    g: &Gen<'_>,
    templates: &mut [TemplateFacts],
    interpreted: bool,
) -> Vec<String> {
    let m = &g.m;
    if !m.rest_fields || m.aborted || m.unit_mode {
        return Vec::new();
    }
    let extended: FxHashSet<u32> = templates
        .iter()
        .flat_map(|t| t.ancestors.iter().copied())
        .collect();
    let mut why: FxHashMap<TmplId, String> = FxHashMap::default();
    let mut cand: FxHashMap<TmplId, ()> = FxHashMap::default();
    for t in templates.iter() {
        if t.class_id.is_none() || !t.fields.iter().any(|f| lane_rep(m, f.rep)) {
            continue;
        }
        match tmpl_refusal(g, t, &extended, interpreted) {
            Some(w) => {
                why.insert(t.id, w.to_string());
            }
            None => {
                cand.insert(t.id, ());
            }
        }
    }
    if !cand.is_empty() {
        let copies = crate::rest_elems::node_copies(m);
        let mut reads: FxHashSet<NodeId> = FxHashSet::default();
        for &n in &g.field_reads {
            reads.insert(n);
            reads.extend(copies.get(&n).into_iter().flatten().copied());
        }
        let mut buf = Vec::new();
        let mut refuse = |cand: &mut FxHashMap<TmplId, ()>, tms: Vec<TmplId>, w: String| {
            for tm in tms {
                if cand.remove(&tm).is_some() {
                    why.insert(tm, w.clone());
                }
            }
        };
        for &tm in cand.clone().keys() {
            if m.tmpl_toks(tm).iter().any(|t| m.obj_used.contains(t)) {
                refuse(&mut cand, vec![tm], "generic reader".to_string());
            }
        }
        for (n, node) in m.nodes.iter().enumerate() {
            if cand.is_empty() {
                break;
            }
            for c in &node.cons {
                if let Some(o) = object_use(c) {
                    let hit = held(m, o, &cand, &mut buf);
                    if !hit.is_empty() {
                        refuse(&mut cand, hit, c.kind_name().to_string());
                    }
                }
            }
            if node.cons.is_empty() {
                continue;
            }
            let hit = held(m, n as NodeId, &cand, &mut buf);
            if hit.is_empty() {
                continue;
            }
            if let Some(kind) = node.cons.iter().find_map(|c| use_of(m, c, &reads)) {
                let on = m.labels.get(n).copied().unwrap_or("?");
                refuse(&mut cand, hit, format!("{kind} on {on}"));
            }
        }
    }
    let mixed = mixed_reads(g, templates, &cand);
    let mut out = Vec::new();
    for t in templates.iter_mut() {
        if t.class_id.is_none() {
            continue;
        }
        let admitted = cand.contains_key(&t.id);
        let what = t.display.replace(' ', "_");
        for f in t.fields.iter_mut().filter(|f| lane_rep(m, f.rep)) {
            let refusal = if admitted {
                field_refusal(f).map(str::to_string).or_else(|| {
                    mixed
                        .contains(&(t.id, f.key.clone()))
                        .then(|| "mixed read".to_string())
                })
            } else {
                why.get(&t.id).cloned()
            };
            match refusal {
                None => {
                    f.rest = true;
                    out.push(format!("# rest lane {what}.{} at_rest=1", f.key));
                }
                Some(w) => out.push(format!(
                    "# rest lane {what}.{} at_rest=0 why={}",
                    f.key,
                    w.replace(' ', "_")
                )),
            }
        }
    }
    out
}

/// Co-occurrence over the named read sites, to its fixpoint: a lane
/// candidate `(template, key)` is refused when some read of `key` whose
/// receiver may hold the template may also hold anything but instances of
/// lane candidates at the same slot, or nullish. Every read of a lane is then
/// the slot's i32 behind the nullish test (`export_reads`), never the
/// runtime's complete read through the accessor, which stays for the reads
/// without an offset.
fn mixed_reads(
    g: &Gen<'_>,
    templates: &[TemplateFacts],
    cand: &FxHashMap<TmplId, ()>,
) -> FxHashSet<(TmplId, String)> {
    let m = &g.m;
    let mut lanes: FxHashMap<(TmplId, String), u32> = FxHashMap::default();
    for t in templates.iter().filter(|t| cand.contains_key(&t.id)) {
        for f in t
            .fields
            .iter()
            .filter(|f| lane_rep(m, f.rep) && field_refusal(f).is_none())
        {
            if let Some(slot) = f.slot {
                lanes.insert((t.id, f.key.clone()), slot);
            }
        }
    }
    let mut refused: FxHashSet<(TmplId, String)> = FxHashSet::default();
    if lanes.is_empty() {
        return refused;
    }
    let copies = crate::rest_elems::node_copies(m);
    let mut buf = Vec::new();
    loop {
        let mut changed = false;
        for rs in &g.read_sites {
            if rs.byte_offset == 0 || rs.is_call || rs.value_call {
                continue;
            }
            let key = m.atoms[rs.key as usize].clone();
            let mut recvs = vec![rs.recv];
            recvs.extend(copies.get(&rs.recv).into_iter().flatten().copied());
            let mut hit: Vec<TmplId> = Vec::new();
            let mut slot: Option<u32> = None;
            let mut uniform = true;
            for &r in &recvs {
                let node = &m.nodes[r as usize];
                if node.prim & !(crate::model::prim::NULL | crate::model::prim::UNDEF) != 0 {
                    uniform = false;
                }
                buf.clear();
                node.pts.sorted_into(&mut buf);
                for &t in &buf {
                    let tm = m.tmpl_of(t);
                    let lane = tm
                        .map(|tm| (tm, key.clone()))
                        .filter(|k| !refused.contains(k))
                        .and_then(|k| lanes.get(&k).copied());
                    match (tm, lane) {
                        (Some(tm), Some(s)) => {
                            hit.push(tm);
                            uniform &= slot.is_none_or(|x| x == s);
                            slot = Some(s);
                        }
                        _ => uniform = false,
                    }
                }
            }
            if uniform || hit.is_empty() {
                continue;
            }
            for tm in hit {
                changed |= refused.insert((tm, key.clone()));
            }
        }
        if !changed {
            break;
        }
    }
    refused
}

/// Fill `ModuleBindings::rest_lane_reads`: every named read site, by byte
/// offset, whose receiver node (or one of its summary copies) may hold a
/// token of a template with an I32 lane at the site's key, with the lane's
/// slot when every value the receiver may hold is such an instance, all at
/// that slot, or null or undefined (`u32::MAX` otherwise). Such a site's read
/// is the slot's i32 behind the receiver's nullish test, whatever codegen can
/// prove about the binding (`perry-codegen` `expr::rest_fields`).
pub(crate) fn export_reads(
    g: &Gen<'_>,
    templates: &[TemplateFacts],
    bindings: &mut [crate::bindings::ModuleBindings],
) {
    let m = &g.m;
    let lanes: FxHashMap<(TmplId, &str), u32> = templates
        .iter()
        .flat_map(|t| {
            t.fields
                .iter()
                .filter(|f| f.rest)
                .filter_map(move |f| Some(((t.id, f.key.as_str()), f.slot?)))
        })
        .collect();
    if lanes.is_empty() {
        return;
    }
    let keys: FxHashSet<&str> = lanes.keys().map(|(_, k)| *k).collect();
    let copies = crate::rest_elems::node_copies(m);
    let mut buf = Vec::new();
    let mut found: FxHashMap<(u32, u32), u32> = FxHashMap::default();
    for rs in &g.read_sites {
        if rs.byte_offset == 0 || rs.is_call || rs.value_call {
            continue;
        }
        let key = m.atoms[rs.key as usize].as_str();
        if !keys.contains(key) {
            continue;
        }
        let mut recvs = vec![rs.recv];
        recvs.extend(copies.get(&rs.recv).into_iter().flatten().copied());
        let mut hit = false;
        let mut slot: Option<u32> = None;
        let mut uniform = true;
        for &r in &recvs {
            let node = &m.nodes[r as usize];
            if node.prim & !(crate::model::prim::NULL | crate::model::prim::UNDEF) != 0 {
                uniform = false;
            }
            buf.clear();
            node.pts.sorted_into(&mut buf);
            for &t in &buf {
                // An admitted instance never escapes, so `⊤` is none of them.
                match m.tmpl_of(t).and_then(|tm| lanes.get(&(tm, key))) {
                    Some(&s) => {
                        hit = true;
                        uniform &= slot.is_none_or(|x| x == s);
                        slot = Some(s);
                    }
                    None => uniform = false,
                }
            }
        }
        if !hit {
            continue;
        }
        let verdict = slot.filter(|_| uniform).unwrap_or(u32::MAX);
        let e = found.entry((rs.module, rs.byte_offset)).or_insert(verdict);
        if *e != verdict {
            *e = u32::MAX;
        }
    }
    for ((module, offset), slot) in found {
        if let Some(mb) = bindings.get_mut(module as usize) {
            mb.rest_lane_reads.push((offset, slot));
        }
    }
    for mb in bindings.iter_mut() {
        mb.rest_lane_reads.sort_unstable();
    }
}
