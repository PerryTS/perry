//! Receiver facts per binding (slice 2 consumer input): what a local or a
//! function's `this` can hold, read off the solved graph.
//!
//! The analysis is flow-insensitive per binding, so one answer holds at
//! every use of the binding. Codegen asks it at a property access whose
//! receiver is `LocalGet(id)` or `This`; HIR property sets carry no source
//! offset, so the binding (not the site) is the key.

use crate::gen::Gen;
use crate::model::{Model, NodeId, TmplId, TmplKind, Tok, TokKind, TOP};

/// A compiled body a binding or a field may hold (FVA): what codegen needs
/// to call it without the runtime's dispatch.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BodyRef {
    /// The defining module (program module index) and the body's HIR id.
    pub module: u32,
    pub func_id: u32,
    /// Its declared parameters (a rest parameter not counted).
    pub params: u32,
    /// No rest parameter and no `arguments`: a call passing at least
    /// `params` arguments enters the body directly.
    pub plain: bool,
    /// A closure expression; else a function declaration.
    pub closure: bool,
}

/// The bodies node `n` holds when it holds nothing else: every value is a
/// compiled closure or function declaration (no ⊤, primitive, object,
/// builtin, bound function, method, another unit's function or a function
/// only the runtime has). Empty otherwise.
pub(crate) fn bodies_of(m: &Model, n: NodeId) -> Vec<BodyRef> {
    let node = &m.nodes[n as usize];
    if node.prim != 0 {
        return Vec::new();
    }
    let mut out: Vec<BodyRef> = Vec::new();
    for t in node.pts.to_sorted() {
        // A marker beside its values, or a closed function's parameter
        // marker (the site rule skips it: `derive::callee_set`).
        if t != TOP && (m.beside_values(t) || m.closed_marker(t)) {
            continue;
        }
        let TokKind::Func(f, _) = m.toks[t as usize] else {
            return Vec::new();
        };
        let fi = &m.funcs[f as usize];
        let closure = m.closures.contains(&f);
        if fi.func_id == u32::MAX || !(closure || m.decls.contains(&f)) {
            return Vec::new();
        }
        out.push(BodyRef {
            module: fi.module,
            func_id: fi.func_id,
            params: fi.params.len() as u32,
            plain: fi.rest.is_none() && !fi.has_arguments,
            closure,
        });
    }
    out.sort();
    out.dedup();
    out
}

/// A stand-in describes another declaration unit's code, but still names
/// the original emitted body. Consume that identity only in the defining
/// source module: the native consumer cannot call a foreign module's HIR id.
fn bodies_with_contracts(g: &Gen, n: NodeId) -> Vec<BodyRef> {
    let m = &g.m;
    let node = &m.nodes[n as usize];
    if node.prim != 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for t in node.pts.to_sorted() {
        if t != TOP && (m.beside_values(t) || m.closed_marker(t)) {
            continue;
        }
        let TokKind::Func(f, _) = m.toks[t as usize] else {
            return Vec::new();
        };
        let fi = &m.funcs[f as usize];
        // Contextual values share this stand-in's immutable emitted origin.
        let Some(c) = g.stand_in_fc.get(&fi.token) else {
            let closure = m.closures.contains(&f);
            if fi.func_id == u32::MAX || !(closure || m.decls.contains(&f)) {
                return Vec::new();
            }
            out.push(BodyRef {
                module: fi.module,
                func_id: fi.func_id,
                params: fi.params.len() as u32,
                plain: fi.rest.is_none() && !fi.has_arguments,
                closure,
            });
            continue;
        };
        let Some((path, func_id, closure)) = &c.compiled_body else {
            return Vec::new();
        };
        let Some(module) = g.modules.iter().position(|(p, _)| *p == path) else {
            return Vec::new();
        };
        out.push(BodyRef {
            module: module as u32,
            func_id: *func_id,
            params: c.params.len() as u32,
            plain: !c.rest && !c.variadic,
            closure: *closure,
        });
    }
    out.sort();
    out.dedup();
    out
}

/// The one representation of a value (representation RFC §10.3): what the
/// whole-program facts give the binding, which codegen obeys.
///
/// It has no order: the derived one would not be the lattice (REVIEW-p4l1
/// finding 7). `Rep::join` is the only join, and every encoding decision
/// goes through `Rep::encoding` (an `Arr` value is the NaN-boxed pointer
/// word every Boxed value is).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Rep {
    /// Any value, NaN-boxed (⊤).
    #[default]
    Boxed,
    /// Only Numbers: a canonical double.
    F64,
    /// Only int32 Numbers (never `-0`) with int demand (`demand`).
    I32,
    /// Only plain program Arrays (DESIGN-p4l1 §2.1-2.2, `arr_rep`).
    Arr(ArrRep),
    /// Only instances of one admitted value-record template, or (`opt`)
    /// `undefined` (DESIGN-p4l4 §1.1, `rep_of`). Its fields by value; until
    /// codegen lowers records (VR-3) its slot encoding is the Boxed word.
    Record(RecRep),
}

/// A value record's representation: the template, and whether `undefined`
/// is among the values (`opt`, the "absent" lane).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RecRep {
    pub tmpl: crate::model::TmplId,
    pub opt: bool,
}

/// The representation of an `Arr` value's elements: `rep_of` of the join
/// of every token's element node (an `undefined` stored there makes it
/// Boxed).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ElemRep {
    #[default]
    Boxed,
    F64,
    I32,
}

/// What every Array an `Arr` value may hold has, for its whole life
/// (DESIGN-p4l1 §2.1, Invariants L, C, D and F of `solve::arr_len`). Each
/// field says less as it moves: `elem` up `I32 ⊑ F64 ⊑ Boxed`, `floor`
/// down, `dense` and `fixed` from true to false.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ArrRep {
    pub elem: ElemRep,
    /// `length >= floor` and `capacity >= floor` (L, C).
    pub floor: u32,
    /// No index below `floor` is ever a hole (D); implies `floor > 0`.
    pub dense: bool,
    /// Every token's length interval is one value `lo == hi`: its storage
    /// never grows, so it is never reallocated or forwarded (F;
    /// REVIEW-p4l1 R3). Carried here so a join (a field over its class set,
    /// a signature slot over its copies) keeps or drops it with the rest,
    /// and no consumer asks a second fact (`const_length`) for it.
    pub fixed: bool,
    /// Every token is immutable (`const_arrays`): nothing writes it after
    /// its birth, so each index holds its birth value for the whole
    /// program. Implies `fixed`. It is about the Arrays, not about a
    /// binding that may be read before it holds one: consumers ask
    /// `BindingFacts::immutable_array`.
    pub im: bool,
    /// Every token's template is at rest (I32 at rest, PR B;
    /// `rest_elems`): each element slot holds an I32 word
    /// `INT32_TAG | (u32)v` for the Array's whole life.
    pub at_rest: bool,
}

/// The encoding of a value's slot: how its word is stored and passed.
/// `Rep::Arr` is knowledge about a Boxed word, never another encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Enc {
    Boxed,
    F64,
    I32,
    /// A value record: its fields by value (DESIGN-p4l4 §1.3), one lane
    /// per layout word (and a presence lane when it may be `undefined`).
    Record,
}

impl ElemRep {
    fn join(a: ElemRep, b: ElemRep) -> ElemRep {
        match (a, b) {
            (ElemRep::Boxed, _) | (_, ElemRep::Boxed) => ElemRep::Boxed,
            (ElemRep::F64, _) | (_, ElemRep::F64) => ElemRep::F64,
            (ElemRep::I32, ElemRep::I32) => ElemRep::I32,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ElemRep::Boxed => "boxed",
            ElemRep::F64 => "f64",
            ElemRep::I32 => "i32",
        }
    }
}

impl ArrRep {
    /// The join (says less) of two array reps.
    pub fn join(a: ArrRep, b: ArrRep) -> ArrRep {
        ArrRep {
            elem: ElemRep::join(a.elem, b.elem),
            floor: a.floor.min(b.floor),
            dense: a.dense && b.dense,
            fixed: a.fixed && b.fixed,
            im: a.im && b.im,
            at_rest: a.at_rest && b.at_rest,
        }
    }
}

impl Rep {
    /// The join of two representations (DESIGN-p4l1 §2.4): a full table,
    /// with no `_` arm, so a new variant is a compile error here. A Number
    /// and an Array have no common representation but Boxed.
    pub fn join(a: Rep, b: Rep) -> Rep {
        match (a, b) {
            (Rep::Boxed, Rep::Boxed | Rep::F64 | Rep::I32 | Rep::Arr(_)) => Rep::Boxed,
            (Rep::F64 | Rep::I32 | Rep::Arr(_), Rep::Boxed) => Rep::Boxed,
            (Rep::F64, Rep::F64 | Rep::I32) | (Rep::I32, Rep::F64) => Rep::F64,
            (Rep::I32, Rep::I32) => Rep::I32,
            (Rep::F64 | Rep::I32, Rep::Arr(_)) | (Rep::Arr(_), Rep::F64 | Rep::I32) => Rep::Boxed,
            (Rep::Arr(x), Rep::Arr(y)) => Rep::Arr(ArrRep::join(x, y)),
            // DESIGN-p4l4 §1.1: one template joins with itself (`opt` OR'd).
            (Rep::Record(x), Rep::Record(y)) if x.tmpl == y.tmpl => Rep::Record(RecRep {
                tmpl: x.tmpl,
                opt: x.opt || y.opt,
            }),
            // Any other join is the record derive's reject (`R5Join`), so
            // after its fixpoint no node holds a record beside anything
            // else; over several nodes (a signature slot's copies) the
            // join is Boxed, a slot the typed signature does not carry.
            (Rep::Record(_), Rep::Record(_))
            | (Rep::Record(_), Rep::Boxed | Rep::F64 | Rep::I32 | Rep::Arr(_))
            | (Rep::Boxed | Rep::F64 | Rep::I32 | Rep::Arr(_), Rep::Record(_)) => Rep::Boxed,
        }
    }

    /// The slot encoding (`Enc`): an `Arr` slot holds the Boxed word.
    pub fn encoding(self) -> Enc {
        match self {
            Rep::Boxed | Rep::Arr(_) => Enc::Boxed,
            Rep::Record(_) => Enc::Record,
            Rep::F64 => Enc::F64,
            Rep::I32 => Enc::I32,
        }
    }

    /// `boxed`, `f64`, `i32`, or `arr<elem,floor[,dense][,fixed]>`.
    pub fn name(self) -> String {
        match self {
            Rep::Boxed => "boxed".into(),
            Rep::F64 => "f64".into(),
            Rep::I32 => "i32".into(),
            Rep::Arr(r) => format!(
                "arr<{},{}{}{}{}{}>",
                r.elem.name(),
                r.floor,
                if r.dense { ",dense" } else { "" },
                if r.fixed { ",fixed" } else { "" },
                if r.im { ",im" } else { "" },
                if r.at_rest { ",rest" } else { "" }
            ),
            Rep::Record(r) => format!("rec<{}{}>", r.tmpl, if r.opt { ",opt" } else { "" }),
        }
    }
}

/// What one binding can hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BindingFacts {
    /// Its representation (`Rep`).
    pub rep: Rep,
    /// −0 tolerance (PR D): its interval holds a `-0` no use observes
    /// (`zero_sign`): an I32 slot may then receive `-0` as `0`.
    pub zero_tol: bool,
    /// One of its values can reach an int consumer (`demand`).
    pub int_demand: bool,
    /// Diagnostic-only TOP provenance; never consumed by emitted-code decisions.
    pub top: bool,
    pub top_cause: Option<String>,
    /// Class ids of the class templates (fact units) it can hold, sorted.
    pub class_ids: Vec<u32>,
    /// Primitive kind bits (`model::prim`).
    pub prims: u16,
    /// It can hold ⊤ or any non-class object (literal, builtin, function).
    pub other: bool,
    /// It holds only these compiled bodies (`bodies_of`); empty otherwise.
    pub bodies: Vec<BodyRef>,
    /// It holds exactly one class object and nothing else: that class's id
    /// (a call of one of its constant statics is a static call, FVA 3.4).
    pub class_object: Option<u32>,
    /// Allocation-derived owner kind and minimum extent; no escaped/hard token.
    pub indexed_owner: Option<IndexedOwner>,
    /// The integer interval of the Numbers it holds (`model::range`): every
    /// Number it holds is an integer, never `-0`, in `[lo, hi]`. `None`: no
    /// such bound. Read beside `prims` and `other`.
    pub int_range: Option<(i64, i64)>,
    /// It holds only strings and ordinary Array births: its `length` is a
    /// Number (a String's code-unit count, an Array's non-configurable
    /// `length`), whatever its declared type says.
    pub length_number: bool,
    /// It holds one program Array literal's tokens and nothing else, each
    /// with the same unmoved birth length (`solve::arr_len`): `x.length` is
    /// that constant.
    pub const_length: Option<i64>,
    /// It is a rooted `const` (`const_arrays`) that holds only immutable
    /// Arrays (`ArrRep::im`): every read of it that yields a value yields
    /// an Array nothing writes after its birth.
    pub immutable_array: bool,
    /// Such a binding whose Arrays are all born from literals of constant
    /// Numbers with the same elements (`const_arrays::collect`): index `i`
    /// reads the Number with bits `const_elems[i]`, for the whole program.
    pub const_elems: Option<Vec<u64>>,
}

/// Storage proof over all values a binding can hold. Allocation identities
/// are analysis tokens, never runtime validity state.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IndexedOwner {
    pub kind: u8,
    pub min_len: u32,
    pub exact_len: Option<u32>,
    pub allocations: Vec<String>,
}

/// Only real stores revoke the metadata contract. Native reads may seed
/// primitive values on field observation nodes; that is not a program write.
pub(crate) fn owner_metadata_written(m: &Model, tm: u32) -> bool {
    let numeric_key = |site: &u32| {
        m.stores[*site as usize].key_value.is_some_and(|n| {
            let n = &m.nodes[n as usize];
            n.prim == crate::model::prim::NUM
                && n.pts
                    .to_sorted()
                    .into_iter()
                    .all(|v| v != TOP && m.beside_values(v))
        })
    };
    if m.any_writers
        .get(&tm)
        .is_some_and(|w| w.ctor.iter().chain(&w.post).any(|s| !numeric_key(s)))
    {
        return true;
    }
    m.writers.keys().any(|(owner, key)| {
        let key = &m.atoms[*key as usize];
        *owner == tm
            && key != crate::model::NUM_KEY
            && key != crate::model::ELEM
            && !crate::solve::numeric_key(key)
    })
}

pub(crate) fn sealed_owner(m: &Model, t: u32) -> Option<(u32, u8)> {
    let TokKind::Obj(tm, _) = m.toks[t as usize] else {
        return None;
    };
    if m.hard.contains_key(&t) || m.escaped.contains_key(&t) {
        return None;
    }
    if let Some(n) = m.ta_birth_input.get(&tm) {
        let src = &m.nodes[*n as usize];
        if src.prim != crate::model::prim::NUM
            || src
                .pts
                .to_sorted()
                .into_iter()
                .any(|v| v == TOP || !m.beside_values(v))
        {
            return None;
        }
    } else if !m.ta_min_len.contains_key(&tm) {
        return None;
    }
    if owner_metadata_written(m, tm) {
        return None;
    }
    Some((tm, *m.ta_elem.get(&tm)?))
}

/// The length every value of node `n` has: it holds only program Array
/// tokens whose birth length never moved (`solve::arr_len`), all the same.
fn const_length(m: &Model, n: u32) -> Option<i64> {
    let node = &m.nodes[n as usize];
    if node.prim != 0 || node.pts.contains_tok(TOP) {
        return None;
    }
    let mut len = None;
    for t in node.pts.to_sorted() {
        let TokKind::Obj(tm, _) = m.toks[t as usize] else {
            return None;
        };
        if !m.is_arr(t) || m.foreign.contains_key(&tm) || m.is_escaped(t) {
            return None;
        }
        let birth = *m.arr_birth_len.get(&tm)?;
        if let Some(&ln) = m.arr_len.get(&t) {
            let r = m.nodes[ln as usize].range;
            if r.is_top() || r.lo != birth || r.hi != birth {
                return None;
            }
        }
        match len {
            None => len = Some(birth),
            Some(l) if l == birth => {}
            Some(_) => return None,
        }
    }
    len
}

fn indexed_owner(g: &Gen, n: u32) -> Option<IndexedOwner> {
    let m = &g.m;
    let node = &m.nodes[n as usize];
    if node.prim != 0 {
        return None;
    }
    let mut out: Option<IndexedOwner> = None;
    for t in node.pts.to_sorted() {
        if t != TOP && m.beside_values(t) {
            continue;
        }
        let (tm, kind) = sealed_owner(m, t)?;
        let len = *m.ta_min_len.get(&tm).unwrap_or(&0);
        let exact_len = m.ta_exact_len.get(&tm).copied();
        let d = m.tmpls[tm as usize].display.clone();
        match &mut out {
            Some(o) if o.kind == kind => {
                o.min_len = o.min_len.min(len);
                if o.exact_len != exact_len {
                    o.exact_len = None;
                }
                o.allocations.push(d);
            }
            Some(_) => return None,
            None => {
                out = Some(IndexedOwner {
                    kind,
                    min_len: len,
                    exact_len,
                    allocations: vec![d],
                })
            }
        }
    }
    if let Some(o) = &mut out {
        o.allocations.sort();
        o.allocations.dedup();
    }
    out
}

/// Per module: `(local id, facts)` and `(HIR function id, facts of this)`,
/// both sorted by id.
#[derive(Clone, Debug, Default)]
pub struct ModuleBindings {
    pub locals: Vec<(u32, BindingFacts)>,
    pub this_of: Vec<(u32, BindingFacts)>,
    /// Numeric leaf functions: evaluating the expression on numeric arguments
    /// has no calls, allocation, mutation or user coercion.
    pub numeric_functions: Vec<(u32, Vec<u32>, perry_hir::Expr)>,
    /// Method call sites by byte offset: each argument's integer interval
    /// at the call (after narrowing: `gen::narrow`), joined over every copy
    /// of the call the program holds (inlining duplicates a site); `None`
    /// for an argument that may be anything but such a Number.
    pub call_arg_ranges: Vec<(u32, Vec<Option<(i64, i64)>>)>,
    /// Element reads by `IndexGet::element_offset` whose receiver is proven
    /// (`ElementRead`), joined over every copy of the read the program holds.
    pub element_reads: Vec<(u32, ElementRead)>,
    /// Method call byte offsets whose result is a Number at every copy of
    /// the call the program holds: every callee the site reaches returns
    /// only Numbers (representation RFC §10.4, the call's return
    /// representation). Sorted.
    pub number_calls: Vec<u32>,
    /// Method call byte offsets proven field element reads
    /// (`field_read_calls`, b1), sorted.
    pub field_read_calls: Vec<(u32, crate::field_read_calls::FieldReadCall)>,
    /// P2: typed signatures by HIR function id, sorted (`signatures`).
    pub signatures: Vec<(u32, crate::signatures::Signature)>,
    /// P2: call byte offset -> the one body `(module, func_id)` a typed
    /// entry serves, sorted (`signatures`, DESIGN-p2 §2).
    pub typed_calls: Vec<(u32, (u32, u32))>,
    /// DESIGN-p4l4 VR-3b: the functions whose Record signature slots stay
    /// Boxed because a source call of the body is not one of its typed
    /// calls in its module (`signatures`), sorted: their callers pass a
    /// materialized copy.
    pub record_demoted: Vec<u32>,
    /// Element reads whose value is a plain Number at every copy, by
    /// `element_offset`, sorted; `true` when the value may also be
    /// `undefined` (`canonical_numbers`, P3 B1).
    pub number_element_reads: Vec<(u32, bool)>,
    /// Check mode: element reads by `element_offset` whose past-the-end
    /// `undefined` the solve withheld at every copy (`Cons::ElemUndef`,
    /// PR-C1), with the least length lower bound of their receivers; sorted
    /// (`element_floors`).
    pub element_floors: Vec<(u32, u32)>,
    /// Module-scope bindings (by local id) whose every value is this one
    /// integer Number, sorted (`const_numbers`, N6).
    pub const_numbers: Vec<(u32, i64)>,
    /// PR-C2: element reads by `element_offset` whose key is an integer
    /// Number at every copy, with the hull of its interval; sorted
    /// (`element_keys`).
    pub element_keys: Vec<(u32, (i64, i64))>,
    /// I32 at rest, PR B (`rest_elems`): element reads and element stores
    /// by `element_offset` whose every receiver holds only Arrays of at-rest
    /// templates, with the least floor of those Arrays; the `fill` call
    /// offsets of the at-rest births, and of those that reserve a capacity
    /// with it (`rest_elems::reserve`). Sorted.
    pub rest_reads: Vec<(u32, u32)>,
    pub rest_writes: Vec<(u32, u32)>,
    pub rest_births: Vec<u32>,
    pub rest_reserves: Vec<(u32, u32)>,
    /// I32 at rest, PR C (`rest_fields::export_reads`): the named reads (by
    /// byte offset) whose receiver may hold an instance with an I32 lane at
    /// the read's key, with the lane's slot when every receiver value is such
    /// an instance at that slot or nullish (`u32::MAX` otherwise). Sorted.
    pub rest_lane_reads: Vec<(u32, u32)>,
}

/// An element read `a[i]` whose every receiver is a program Array that
/// keeps its birth length for the whole program (`const_length`): no store
/// moves it, so its storage never grows, is never forwarded and holds `len`
/// slots (a birth length is recorded only for a densely backed birth:
/// `solve::arr_len`), and no `defineProperty` ever gave it an element descriptor
/// (`solve::arr_len`). A slot holds its own data or a hole; a hole reads
/// through the prototype chain, which the read leaves to the generic path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElementRead {
    /// The length every receiver has, at every copy.
    pub len: u32,
    /// The key is an integer Number in `[0, len)` at every copy.
    pub key_in_bounds: bool,
}

/// The `ElementRead` of one copy of an element read.
fn element_read(m: &Model, site: &crate::gen::ElementSite) -> Option<ElementRead> {
    // A computed key, or a literal one that is a canonical array index (a
    // loop the HIR unrolled reads `a[0]`, `a[1]`, ... at the loop's site).
    let index_key = match site.key {
        crate::model::Key::Any(_) => true,
        crate::model::Key::Named(a) => {
            let k = &m.atoms[a as usize];
            k.parse::<u32>()
                .is_ok_and(|v| v != u32::MAX && v.to_string() == *k)
        }
    };
    if !index_key {
        return None;
    }
    let len = const_length(m, site.recv)?;
    // A named property moves an Array to the named layout.
    for t in m.nodes[site.recv as usize].pts.to_sorted() {
        let TokKind::Obj(tm, _) = m.toks[t as usize] else {
            return None;
        };
        if owner_metadata_written(m, tm) {
            return None;
        }
    }
    // The read addresses slot `i` of the store, which holds `len` slots: a
    // birth length is recorded only for a birth whose capacity is at least
    // its length, and capacity never falls below it while the length is
    // defined (`solve::arr_len`).
    let len = u32::try_from(len).ok().filter(|&l| l >= 1)?;
    let key = number_range(m, site.idx);
    let key_in_bounds = key.is_some_and(|(lo, hi)| lo >= 0 && hi < i64::from(len));
    Some(ElementRead { len, key_in_bounds })
}

/// Node `n` holds only Numbers, each an integer in this interval. An empty
/// node proves nothing (it may be one the solve left unpopulated).
pub(crate) fn number_range(m: &Model, n: NodeId) -> Option<(i64, i64)> {
    let node = &m.nodes[n as usize];
    if node.prim != crate::model::prim::NUM || !node.pts.is_empty() {
        return None;
    }
    let r = node.range;
    (!r.is_top() && !r.is_empty()).then_some((r.lo, r.hi))
}

/// `facts_of` for P2's signatures (`signatures`).
pub(crate) fn facts_of_node(g: &Gen, n: u32) -> BindingFacts {
    facts_of(g, n)
}

/// The one representation of node `n` (`rep_of`), for P2's signatures.
pub(crate) fn rep_of_node(g: &Gen, n: u32) -> Rep {
    facts_of(g, n).rep
}

fn facts_of(g: &Gen, n: u32) -> BindingFacts {
    let m = &g.m;
    let node = &m.nodes[n as usize];
    let mut arrays_only = true;
    let mut values = node.prim != 0;
    let mut out = BindingFacts {
        prims: node.prim,
        top: m.explain && node.pts.contains_tok(TOP),
        top_cause: (m.explain && node.pts.contains_tok(TOP)).then(|| m.top_origin(n)),
        ..Default::default()
    };
    for t in node.pts.to_sorted() {
        if t == TOP {
            out.other = true;
            continue;
        }
        // A marker beside its values (a closed function's described
        // parameter, what it reaches, an argument marker of only such
        // markers; DECISIONS 74, 75) stands for no value of its own: the
        // callers' values are the contract's, beside it in this node. It
        // adds no class and is not `other` (DECISIONS 76).
        if m.beside_values(t) {
            continue;
        }
        // Any other marker is another unit's argument whole (DECISIONS
        // 119): any value, any primitive.
        if m.bare_marker(t) {
            out.other = true;
            out.prims |= crate::model::prim::ALL;
            continue;
        }
        values = true;
        match &m.toks[t as usize] {
            TokKind::Obj(tm, _) => match m.tmpls[*tm as usize].kind {
                TmplKind::Class { class_id, .. } => {
                    arrays_only = false;
                    out.class_ids.push(class_id)
                }
                TmplKind::Arr { .. } => out.other = true,
                _ => {
                    arrays_only = false;
                    out.other = true
                }
            },
            _ => {
                arrays_only = false;
                out.other = true
            }
        }
    }
    out.length_number = values
        && arrays_only
        && !node.pts.contains_tok(TOP)
        && node.prim & !crate::model::prim::STR == 0;
    out.class_ids.sort_unstable();
    out.class_ids.dedup();
    out.indexed_owner = indexed_owner(g, n);
    if !node.range.is_top() && !node.range.is_empty() {
        out.int_range = Some((node.range.lo, node.range.hi));
    }
    out.const_length = const_length(m, n);
    out.bodies = bodies_with_contracts(g, n);
    out.class_object = class_object_of(m, n);
    out.int_demand = node.int_demand;
    out.zero_tol = node.zs_tol;
    out.rep = match arr_rep(m, &[n]) {
        Some(r) => Rep::Arr(r),
        None => match record_rep(m, n) {
            Some(r) => Rep::Record(r),
            None => rep_of(&out),
        },
    };
    out
}

/// DESIGN-p4l4 §1.1: `Record` when every token of the node is an instance
/// of one admitted value-record template (`Model::records`), its primitives
/// are within `undefined` (which sets `opt`), and it holds no ⊤ or marker.
fn record_rep(m: &Model, n: NodeId) -> Option<RecRep> {
    if m.records.is_empty() {
        return None;
    }
    let node = &m.nodes[n as usize];
    if node.pts.is_empty() || node.prim & !crate::model::prim::UNDEF != 0 {
        return None;
    }
    let mut one: Option<TmplId> = None;
    for t in node.pts.to_sorted() {
        let TokKind::Obj(tm, _) = m.toks[t as usize] else {
            return None;
        };
        if t == TOP || m.is_marker(t) || !m.records.contains(&tm) || one.is_some_and(|o| o != tm)
        {
            return None;
        }
        one = Some(tm);
    }
    one.map(|tmpl| RecRep {
        tmpl,
        opt: node.prim & crate::model::prim::UNDEF != 0,
    })
}

/// `Rep::Arr` of what `nodes` hold together (DESIGN-p4l1 §2.2): every value
/// is a token of a plain program Array template (`TmplKind::Arr::plain`)
/// that is not foreign, escaped or hard (each tested here: `const_length`
/// leans on `mark_hard` making the length ⊤), and whose template no named
/// or metadata write touched (`owner_metadata_written`, as `ElementRead`
/// requires). No primitive, no ⊤, no marker of any kind; a node with no
/// value proves nothing. The rep is the join over the tokens of each one's
/// own (`tok_arr_rep`). `None` with `Model::array_reps` off.
pub(crate) fn arr_rep(m: &Model, nodes: &[NodeId]) -> Option<ArrRep> {
    arr_rep_beside(m, nodes, 0)
}

/// `arr_rep` of `nodes` with the primitives `ignored` left out of account:
/// 0 everywhere but under the negative control `arrfield`
/// (`Model::arr_field_sabotage`).
pub(crate) fn arr_rep_beside(m: &Model, nodes: &[NodeId], ignored: u16) -> Option<ArrRep> {
    if !m.array_reps {
        return None;
    }
    let mut out: Option<ArrRep> = None;
    for &n in nodes {
        let node = &m.nodes[n as usize];
        if node.prim & !ignored != 0 {
            return None;
        }
        for t in node.pts.to_sorted() {
            if t == TOP || m.is_marker(t) || m.bare_marker(t) || m.beside_values(t) {
                return None;
            }
            let TokKind::Obj(tm, _) = m.toks[t as usize] else {
                return None;
            };
            let TmplKind::Arr {
                hole_free,
                plain: true,
                ..
            } = m.tmpls[tm as usize].kind
            else {
                return None;
            };
            if m.foreign.contains_key(&tm)
                || m.is_escaped(t)
                || m.hard.contains_key(&t)
                || owner_metadata_written(m, tm)
            {
                return None;
            }
            let r = tok_arr_rep(m, t, tm, hole_free);
            out = Some(out.map_or(r, |o| ArrRep::join(o, r)));
        }
    }
    out
}

/// One plain Array token's `ArrRep`:
/// - `floor`: the low end of its length interval (`solve::arr_len`), 0 when
///   the length is ⊤. A token no length rule touched has its birth length
///   (`Model::arr_len` is born from it), or none;
/// - `fixed`: that interval is one value;
/// - `dense`: born hole-free with `floor > 0` (Invariant D);
/// - `elem`: `rep_of` of its element node (`all_fields`), I32 for int32
///   Numbers with int demand, F64 for Numbers, else Boxed.
pub(crate) fn tok_arr_rep(m: &Model, t: Tok, tm: u32, hole_free: bool) -> ArrRep {
    let (lo, fixed) = match m.arr_len.get(&t) {
        Some(&ln) => {
            let r = m.nodes[ln as usize].range;
            if r.is_top() || r.is_empty() {
                (0, false)
            } else {
                (r.lo, r.lo == r.hi)
            }
        }
        None => match m.arr_birth_len.get(&tm) {
            Some(&k) => (k, true),
            None => (0, false),
        },
    };
    let floor = u32::try_from(lo.max(0)).unwrap_or(u32::MAX);
    let elem = match m.all_fields.get(&t) {
        Some(&e) => {
            let node = &m.nodes[e as usize];
            let int32 = !node.range.is_top()
                && !node.range.is_empty()
                && node.range.lo >= i64::from(i32::MIN)
                && node.range.hi <= i64::from(i32::MAX);
            let numbers = node.prim == crate::model::prim::NUM && node.pts.is_empty();
            if m.arr_elem_sabotage && node.prim & crate::model::prim::NUM != 0 {
                ElemRep::F64
            } else if !numbers {
                ElemRep::Boxed
            } else if int32 && elem_demand(m, t, node) {
                ElemRep::I32
            } else {
                ElemRep::F64
            }
        }
        None => ElemRep::Boxed,
    };
    ArrRep {
        elem,
        floor,
        dense: hole_free && floor > 0,
        fixed,
        im: fixed && m.arr_immutable(t),
        at_rest: m.rest_tmpls.contains(&tm),
    }
}

/// Int demand on token `t`'s elements (`node`, its element node). A read
/// `a[k]` takes its value from the fields a number key names
/// (`Model::num_fields`), which feed the element node, so its demand lands
/// there; with I32 at rest on (PR B, `rest_elems`) that demand counts too.
pub(crate) fn elem_demand(m: &Model, t: Tok, node: &crate::model::Node) -> bool {
    node.int_demand
        || m.rest_elems
            && m
                .num_fields
                .get(&t)
                .is_some_and(|&n| m.nodes[n as usize].int_demand)
}

/// The Number representation of what `nodes` hold together (a field over
/// its template's tokens; I32 at rest, RFC §10.7): F64 when every value is
/// a Number, I32 when also every one is an int32 (the joined interval) and
/// one reaches an int consumer (demand); Boxed for anything else, and for
/// no node or no value. The negative control `restfield` claims I32 for
/// any Number field.
pub(crate) fn number_rep_of_nodes(m: &Model, nodes: &[NodeId]) -> Rep {
    let mut range = crate::model::Iv::EMPTY;
    let mut demand = false;
    let mut values = false;
    for &n in nodes {
        let node = &m.nodes[n as usize];
        if !node.pts.is_empty() || node.prim & !crate::model::prim::NUM != 0 {
            return Rep::Boxed;
        }
        values |= node.prim != 0;
        range = range.join(node.range);
        demand |= node.int_demand;
    }
    if !values {
        return Rep::Boxed;
    }
    if m.rest_field_sabotage {
        return Rep::I32;
    }
    let int32 = !range.is_top()
        && !range.is_empty()
        && range.lo >= i64::from(i32::MIN)
        && range.hi <= i64::from(i32::MAX);
    if int32 && demand {
        Rep::I32
    } else {
        Rep::F64
    }
}

/// The representation the facts give: F64 for a binding of only Numbers,
/// I32 for one of only int32 Numbers that an int consumer reads.
fn rep_of(b: &BindingFacts) -> Rep {
    if b.other || !b.class_ids.is_empty() || b.prims != crate::model::prim::NUM {
        return Rep::Boxed;
    }
    let int32 = b
        .int_range
        .is_some_and(|(lo, hi)| lo >= i64::from(i32::MIN) && hi <= i64::from(i32::MAX));
    if int32 && b.int_demand {
        Rep::I32
    } else {
        Rep::F64
    }
}

/// The dump's representation probe (N1, `PERRY_FACTS_DUMP` only): one
/// `# rep` line per Number local (F64, I32, an interval or an int use)
/// with what decides its representation (RFC §10.3): the prims, the
/// interval, int demand and whether the binding is itself an int
/// consumer's operand. A local is named by its node, readable with
/// `PERRY_FACTS_RELATIONS=1`; otherwise by its id (`local=`), the HIR
/// local id codegen reports.
pub(crate) fn rep_probe(g: &Gen) -> Vec<String> {
    let m = &g.m;
    let mut out = Vec::new();
    for (mi, info) in g.mods.iter().enumerate() {
        let mut ls: Vec<(u32, NodeId)> = info.locals.iter().map(|(&i, &n)| (i, n)).collect();
        ls.sort_unstable();
        for (id, n) in ls {
            let node = &m.nodes[n as usize];
            let b = facts_of(g, n);
            if !(matches!(b.rep, Rep::F64 | Rep::I32) || b.int_range.is_some() || node.int_use) {
                continue;
            }
            let range = b
                .int_range
                .map_or("top".to_string(), |(lo, hi)| format!("[{lo},{hi}]"));
            let name = m.node_names.get(n as usize).map_or("?", |s| s.as_str());
            out.push(format!(
                "# rep module={mi} local={id} name={} rep={} prims={:#x} other={} range={range} demand={} use={}",
                name.replace(' ', "_"),
                b.rep.name(),
                b.prims,
                (b.other || !b.class_ids.is_empty()) as u8,
                b.int_demand as u8,
                node.int_use as u8,
            ));
        }
    }
    out.extend(rest_probe(m));
    out.extend(crate::rest_elems::probe(m));
    out
}

/// The at-rest probe (I32 at rest, RFC §10.7; `PERRY_FACTS_DUMP` only): one
/// `# rest field` line per class or literal field whose nodes hold Numbers
/// and one `# rest elem` line per plain Array template whose element nodes
/// do, each with the join over the template's tokens of what decides the
/// slot's representation (RFC §10.3): the prims, the tokens, the interval
/// and int demand.
fn rest_probe(m: &Model) -> Vec<String> {
    let mut out = Vec::new();
    for (tm, tpl) in m.tmpls.iter().enumerate() {
        let tm = tm as u32;
        let toks = m.tmpl_toks(tm);
        if toks.is_empty() {
            continue;
        }
        let line = |what: &str, nodes: &[NodeId]| -> Option<String> {
            let mut prims = 0u16;
            let mut other = false;
            let mut range = crate::model::Iv::EMPTY;
            let mut demand = false;
            let mut zsign = false;
            for &n in nodes {
                let node = &m.nodes[n as usize];
                prims |= node.prim;
                other |= !node.pts.is_empty();
                range = range.join(node.range);
                demand |= node.int_demand;
                zsign |= node.zsign;
            }
            if prims & crate::model::prim::NUM == 0 {
                return None;
            }
            let range = if range.is_top() || range.is_empty() {
                "top".to_string()
            } else {
                format!("[{},{}]", range.lo, range.hi)
            };
            Some(format!(
                "# rest {what} prims={prims:#x} other={} range={range} demand={} zsign={}",
                other as u8, demand as u8, zsign as u8
            ))
        };
        if let TmplKind::Arr { plain: true, .. } = tpl.kind {
            let nodes: Vec<NodeId> = toks
                .iter()
                .filter_map(|t| m.all_fields.get(t).copied())
                .collect();
            if let Some(l) = line(&format!("elem {}", tpl.display.replace(' ', "_")), &nodes) {
                out.push(l);
            }
            continue;
        }
        if !matches!(tpl.kind, TmplKind::Class { .. } | TmplKind::Lit { .. }) {
            continue;
        }
        for &a in &tpl.birth_keys {
            let nodes: Vec<NodeId> = toks
                .iter()
                .filter_map(|t| m.fields.get(&(*t, a)).copied())
                .collect();
            let what = format!(
                "field {}.{}",
                tpl.display.replace(' ', "_"),
                crate::model::esc_key(&m.atoms[a as usize])
            );
            if let Some(l) = line(&what, &nodes) {
                out.push(l);
            }
        }
    }
    out
}

/// The class id of the one class object node `n` holds when it holds
/// nothing else (markers beside their values aside).
fn class_object_of(m: &Model, n: NodeId) -> Option<u32> {
    let node = &m.nodes[n as usize];
    if node.prim != 0 {
        return None;
    }
    let mut one = None;
    for t in node.pts.to_sorted() {
        if t != TOP && (m.beside_values(t) || m.closed_marker(t)) {
            continue;
        }
        let TokKind::ClassObj(tm) = m.toks[t as usize] else {
            return None;
        };
        // Rule C (DECISIONS 251): a class expression evaluated more than
        // once is one class object per evaluation, never one class's.
        if m.tmpls[tm as usize].per_evaluation {
            return None;
        }
        let TmplKind::Class { class_id, .. } = m.tmpls[tm as usize].kind else {
            return None;
        };
        if one.replace(class_id).is_some_and(|c| c != class_id) {
            return None;
        }
    }
    one
}

pub(crate) fn collect(g: &Gen) -> Vec<ModuleBindings> {
    let mut out: Vec<ModuleBindings> = (0..g.mods.len()).map(|_| Default::default()).collect();
    for (mi, info) in g.mods.iter().enumerate() {
        let mut v: Vec<(u32, BindingFacts)> = info
            .locals
            .iter()
            .map(|(&id, &n)| (id, facts_of(g, n)))
            .collect();
        v.sort_by_key(|(id, _)| *id);
        out[mi].locals = v;
    }
    for f in &g.m.funcs {
        if f.func_id == u32::MAX {
            continue;
        }
        let b = facts_of(g, f.this);
        out[f.module as usize].this_of.push((f.func_id, b));
    }
    for &(mi, offset, ref args) in &g.call_args_at {
        let iv: Vec<Option<(i64, i64)>> = args.iter().map(|&n| number_range(&g.m, n)).collect();
        out[mi as usize].call_arg_ranges.push((offset, iv));
    }
    let mut reads: Vec<Vec<(u32, Option<ElementRead>)>> = vec![Vec::new(); out.len()];
    for site in &g.element_sites {
        if site.op == crate::derive::ElementOp::Read && site.byte_offset != 0 {
            reads[site.module as usize].push((site.byte_offset, element_read(&g.m, site)));
        }
    }
    for (mb, mut rs) in out.iter_mut().zip(reads) {
        rs.sort_by_key(|(o, _)| *o);
        let mut joined: Vec<(u32, Option<ElementRead>)> = Vec::new();
        for (o, r) in rs {
            match joined.last_mut() {
                Some((lo, prev)) if *lo == o => {
                    *prev = match (*prev, r) {
                        (Some(a), Some(b)) if a.len == b.len => Some(ElementRead {
                            len: a.len,
                            key_in_bounds: a.key_in_bounds && b.key_in_bounds,
                        }),
                        _ => None,
                    }
                }
                _ => joined.push((o, r)),
            }
        }
        mb.element_reads = joined
            .into_iter()
            .filter_map(|(o, r)| Some((o, r?)))
            .collect();
    }
    for mb in &mut out {
        mb.call_arg_ranges.sort_by_key(|(o, _)| *o);
        let mut joined: Vec<(u32, Vec<Option<(i64, i64)>>)> = Vec::new();
        for (o, iv) in std::mem::take(&mut mb.call_arg_ranges) {
            match joined.last_mut() {
                Some((lo, prev)) if *lo == o => {
                    if prev.len() != iv.len() {
                        prev.iter_mut().for_each(|p| *p = None);
                        continue;
                    }
                    for (p, q) in prev.iter_mut().zip(iv) {
                        *p = match (*p, q) {
                            (Some((a, b)), Some((c, d))) => Some((a.min(c), b.max(d))),
                            _ => None,
                        };
                    }
                }
                _ => joined.push((o, iv)),
            }
        }
        mb.call_arg_ranges = joined;
    }
    let mut results: Vec<Vec<(u32, bool)>> = vec![Vec::new(); out.len()];
    for &(mi, offset, n) in &g.call_results_at {
        let node = &g.m.nodes[n as usize];
        let number = g.m.number_call_sabotage
            || (node.prim == crate::model::prim::NUM && node.pts.is_empty());
        results[mi as usize].push((offset, number));
    }
    for (mb, mut rs) in out.iter_mut().zip(results) {
        rs.sort_unstable();
        // A site is a Number call when every copy's result is (`false`
        // sorts first, so the first entry of an offset decides).
        rs.dedup_by_key(|(o, _)| *o);
        mb.number_calls = rs.into_iter().filter(|(_, n)| *n).map(|(o, _)| o).collect();
    }
    for (mi, (_, m)) in g.modules.iter().enumerate() {
        fn pure(e: &perry_hir::Expr, params: &[u32]) -> bool {
            use perry_hir::{BinaryOp, Expr, UnaryOp};
            match e {
                Expr::Integer(_) => true,
                Expr::Number(n) => n.is_finite(),
                Expr::LocalGet(id) => params.contains(id),
                Expr::Binary { op, left, right } if !matches!(op, BinaryOp::Pow) => {
                    pure(left, params) && pure(right, params)
                }
                Expr::Unary {
                    op: UnaryOp::Pos | UnaryOp::Neg | UnaryOp::BitNot,
                    operand,
                }
                | Expr::MathSqrt(operand) => pure(operand, params),
                _ => false,
            }
        }
        for f in &m.functions {
            if f.is_async
                || f.is_generator
                || f.params.iter().any(|p| p.is_rest || p.default.is_some())
            {
                continue;
            }
            let params: Vec<_> = f.params.iter().map(|p| p.id).collect();
            if let [perry_hir::Stmt::Return(Some(e))] = f.body.as_slice() {
                if pure(e, &params) {
                    out[mi].numeric_functions.push((f.id, params, e.clone()));
                }
            }
        }
    }
    for mb in &mut out {
        mb.this_of.sort_by_key(|(id, _)| *id);
        // A function id registered twice (should not happen) gets no answer.
        let mut dup = Vec::new();
        for w in mb.this_of.windows(2) {
            if w[0].0 == w[1].0 {
                dup.push(w[0].0);
            }
        }
        mb.this_of.retain(|(id, _)| !dup.contains(id));
    }
    out
}
