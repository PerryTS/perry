//! Derived facts F1–F6 read off the solved graph (design section 2), and
//! the per-site verdicts the precision report uses.

use crate::gen::Gen;
use crate::model::*;
use crate::pts::{FxHashMap, FxHashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindFact {
    pub prims: u16,
    /// Object templates the field can hold (display names), sorted.
    pub objs: Vec<String>,
    pub funcs: usize,
    pub builtin_objs: usize,
    pub top: bool,
}

impl KindFact {
    /// A single representation the consumer can rely on: one primitive
    /// kind, or objects (optionally nullable), never ⊤.
    pub fn fixed(&self) -> bool {
        if self.top {
            return false;
        }
        let has_obj = !self.objs.is_empty() || self.funcs > 0 || self.builtin_objs > 0;
        let p = self.prims;
        if !has_obj {
            return matches!(
                p,
                prim::NUM | prim::STR | prim::BOOL | prim::BIGINT | prim::SYMBOL
            );
        }
        p & !(prim::NULL | prim::UNDEF) == 0
    }

    pub fn desc(&self) -> String {
        if self.top {
            return "top".into();
        }
        let mut parts = Vec::new();
        if self.prims != 0 {
            parts.push(prim::name(self.prims));
        }
        if !self.objs.is_empty() {
            parts.push(format!("obj{{{}}}", self.objs.join(",")));
        }
        if self.funcs > 0 {
            parts.push(format!("fn*{}", self.funcs));
        }
        if self.builtin_objs > 0 {
            parts.push(format!("builtin*{}", self.builtin_objs));
        }
        if parts.is_empty() {
            "none".into()
        } else {
            parts.join("|")
        }
    }

    /// Kind bits for the contract section: prims plus bit 7 = object.
    pub fn mask(&self) -> u16 {
        if self.top {
            return 0xffff;
        }
        let mut m = self.prims;
        if !self.objs.is_empty() || self.funcs > 0 || self.builtin_objs > 0 {
            m |= 1 << 7;
        }
        m
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldFacts {
    pub key: String,
    pub slot: Option<u32>,
    pub definite: bool,
    pub data_field: bool,
    pub readonly: bool,
    pub writers_ctor: usize,
    pub writers_post: usize,
    pub kind: KindFact,
    /// F6: readonly, holds only typed arrays nobody can expose, detach,
    /// transfer or resize (B1–B4).
    pub ta_stable: bool,
    /// The one element kind (`perry_hir::TYPED_ARRAY_KIND_*`) of every
    /// typed array the field can hold, when all are `TypedArrayNew` owners
    /// of that kind.
    pub ta_kind: Option<u8>,
    /// A definite data field holding only these compiled bodies
    /// (`bindings::bodies_of` over every token of the layout); empty when
    /// it may hold anything else.
    pub fns: Vec<crate::bindings::BodyRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TemplateFacts {
    pub id: TmplId,
    pub display: String,
    pub kind: &'static str,
    pub module: Option<u32>,
    pub class_id: Option<u32>,
    pub escape: Option<String>,
    pub hard: Option<String>,
    pub chain: Option<String>,
    pub prefix_stable: bool,
    pub fixed_layout: bool,
    pub adds_keys: u32,
    pub birth_keys: Vec<String>,
    pub fields: Vec<FieldFacts>,
    pub sabotaged: bool,
    /// Class ids of the parent chain (leaf's parent first).
    pub ancestors: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    T0Mono,
    T0Set,
    /// Proven in the typed body behind the entry guard on the receiver
    /// binding (design §11.4); the generic body keeps the site.
    E0,
    No(String),
}

#[derive(Clone, Debug)]
pub struct SiteFacts {
    pub module: u32,
    pub byte_offset: u32,
    pub key: String,
    pub is_call: bool,
    pub receivers: Vec<String>,
    pub top: bool,
    pub verdict: Verdict,
    pub fixed_shape: bool,
    /// A call site: the bodies its callee set holds (every body, sorted;
    /// empty when the set holds anything that is not a body).
    pub targets: Vec<String>,
    /// Per-unit run: the receiver is a parameter whose TS object type
    /// names these keys; its verdict is decided after the census
    /// (`units::typed_param_verdicts`).
    pub(crate) guard_keys: Option<Vec<String>>,
}

/// `Some` (a reason a claim fails) absorbs `None`; of two reasons the
/// smaller is kept, so the join is commutative and only descends among
/// the finitely many reasons.
fn join_reason(a: &mut Option<String>, b: &Option<String>) {
    match (a.as_mut(), b) {
        (_, None) => {}
        (None, Some(y)) => *a = Some(y.clone()),
        (Some(x), Some(y)) => {
            if y < x {
                *x = y.clone();
            }
        }
    }
}

/// The common prefix of two lists.
fn common_prefix<T: PartialEq>(a: &mut Vec<T>, b: &[T]) {
    let n = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    a.truncate(n);
}

impl KindFact {
    /// The join: what a field may hold, in either.
    pub(crate) fn join(&mut self, o: &KindFact) {
        self.prims |= o.prims;
        for x in &o.objs {
            if !self.objs.contains(x) {
                self.objs.push(x.clone());
            }
        }
        self.objs.sort();
        self.funcs = self.funcs.max(o.funcs);
        self.builtin_objs = self.builtin_objs.max(o.builtin_objs);
        self.top |= o.top;
    }
}

impl FieldFacts {
    /// The join of two descriptions of one field (by `key`): a claim holds
    /// only if both make it; a slot or element kind they disagree on is
    /// none; the writers and the kind grow.
    pub(crate) fn join(&mut self, o: &FieldFacts) {
        if self.slot != o.slot {
            self.slot = None;
        }
        self.definite &= o.definite;
        self.data_field &= o.data_field;
        self.readonly &= o.readonly;
        self.writers_ctor = self.writers_ctor.max(o.writers_ctor);
        self.writers_post = self.writers_post.max(o.writers_post);
        self.kind.join(&o.kind);
        self.ta_stable &= o.ta_stable;
        if self.ta_kind != o.ta_kind {
            self.ta_kind = None;
        }
        // Each says the field holds only its bodies: both, their union.
        if self.fns.is_empty() || o.fns.is_empty() {
            self.fns.clear();
        } else {
            self.fns.extend(o.fns.iter().cloned());
            self.fns.sort();
            self.fns.dedup();
        }
    }
}

impl TemplateFacts {
    /// The join of two descriptions of one template (by `display`; its id,
    /// kind, module and class id are its identity): a claim holds only if
    /// both make it. Escape, HARD and chain problems absorb; stability and
    /// a fixed layout need both; birth keys and ancestors keep their common
    /// prefix; a field only one has is not definite in the join.
    pub(crate) fn join(&mut self, o: &TemplateFacts) {
        join_reason(&mut self.escape, &o.escape);
        join_reason(&mut self.hard, &o.hard);
        join_reason(&mut self.chain, &o.chain);
        self.prefix_stable &= o.prefix_stable;
        self.fixed_layout &= o.fixed_layout;
        self.adds_keys = self.adds_keys.max(o.adds_keys);
        common_prefix(&mut self.birth_keys, &o.birth_keys);
        common_prefix(&mut self.ancestors, &o.ancestors);
        self.sabotaged |= o.sabotaged;
        for f in self.fields.iter_mut() {
            match o.fields.iter().find(|g| g.key == f.key) {
                Some(g) => f.join(g),
                None => f.definite = false,
            }
        }
        for g in &o.fields {
            if !self.fields.iter().any(|f| f.key == g.key) {
                let mut g = g.clone();
                g.definite = false;
                self.fields.push(g);
            }
        }
    }
}

pub fn derive(g: &mut Gen) -> (Vec<TemplateFacts>, Vec<SiteFacts>) {
    let m = &mut g.m;
    // A program-wide kill only for what the compiled program itself does:
    // an accessor it installs on `Object.prototype` (E23, any key). Dynamic
    // code is not a kill (decision 150): its data flow is ⊤, and what it can
    // do to the intrinsic prototypes is the runtime's guard word
    // (`ProgramFacts::interp_guard`).
    let global = m
        .objproto_accessor
        .then(|| "E23 accessor installed on Object.prototype".to_string());
    let objproto = m.tok(TokKind::ObjectProto);
    let objproto_hard = m
        .hard
        .get(&objproto)
        .map(|(k, r)| format!("{} {}", k.code(), m.reasons[*r as usize]));
    let mut out = Vec::new();
    for tm in 0..m.tmpls.len() as TmplId {
        if !m.tmpls[tm as usize].kind.is_fact_unit() {
            continue;
        }
        // Another unit's class: the facts its defining unit published; what
        // this unit does beyond the contract is its census report (§11.3).
        if let Some(f) = m.foreign.get(&tm) {
            out.push(crate::TemplateFacts {
                id: tm,
                ..f.clone()
            });
            continue;
        }
        let tf = template_facts(m, tm, &global, &objproto_hard);
        out.push(tf);
    }
    let sites = site_facts(g, &out);
    let foreign = &g.m.foreign;
    out.retain(|t| !foreign.contains_key(&t.id));
    (out, sites)
}

/// The static prototype chain tokens of a template (plus every prototype
/// object a `new f()` / literal `__proto__` can name).
fn chain_tokens(m: &Model, tm: TmplId) -> Vec<Tok> {
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    let mut stack = Vec::new();
    stack.extend(m.tmpl_toks(tm));
    let mut out = Vec::new();
    while let Some(t) = stack.pop() {
        let Some(&pn) = m.proto_node.get(&t) else {
            continue;
        };
        for p in m.nodes[pn as usize].pts.to_sorted() {
            if seen.insert(p) {
                out.push(p);
                stack.push(p);
            }
        }
    }
    out
}

fn template_facts(
    m: &mut Model,
    tm: TmplId,
    global: &Option<String>,
    objproto_hard: &Option<String>,
) -> TemplateFacts {
    // The template's own token and every birth-site token: one layout.
    m.tok(TokKind::Obj(tm, NO_CTX));
    let ts = m.tmpl_toks(tm);
    let tpl = m.tmpls[tm as usize].clone();
    let escape = ts
        .iter()
        .find_map(|t| m.escaped.get(t))
        .map(|r| m.reasons[*r as usize].clone());
    let hard = ts
        .iter()
        .find_map(|t| m.hard.get(t))
        .map(|(k, r)| format!("{} {}", k.code(), m.reasons[*r as usize]));
    // The prototype chain must be stable: no HARD op or escape on any
    // prototype object, no accessor named after a birth key, no computed
    // accessor anywhere.
    let chain = chain_tokens(m, tm);
    let mut chain_problem: Option<String> = None;
    let mut accessor_keys: Vec<Atom> = tpl.accessor_keys.clone();
    for t in &ts {
        if let Some(ks) = m.accessor_keys.get(t) {
            accessor_keys.extend(ks.iter().copied());
        }
    }
    for &p in &chain {
        if p == TOP {
            chain_problem.get_or_insert_with(|| "prototype from unmodeled code".into());
            continue;
        }
        if let Some((k, r)) = m.hard.get(&p) {
            chain_problem.get_or_insert_with(|| {
                format!(
                    "prototype {} {} {}",
                    m.tok_name(p),
                    k.code(),
                    m.reasons[*r as usize]
                )
            });
        }
        if p != m.tok_index[&TokKind::ObjectProto] {
            if let Some(r) = m.escaped.get(&p) {
                chain_problem.get_or_insert_with(|| {
                    format!(
                        "prototype {} escapes: {}",
                        m.tok_name(p),
                        m.reasons[*r as usize]
                    )
                });
            }
        }
        if let Some(ks) = m.accessor_keys.get(&p) {
            accessor_keys.extend(ks.iter().copied());
        }
    }
    if let Some(r) = objproto_hard {
        chain_problem.get_or_insert_with(|| format!("Object.prototype {r}"));
    }
    if let Some(r) = global {
        chain_problem.get_or_insert_with(|| r.clone());
    }
    let star = m.atom(crate::STAR_KEY);
    let escape = if accessor_keys.contains(&star) && escape.is_none() {
        // A computed-key accessor on the chain runs as unknown code with
        // the receiver: treat the template as escaped (kills readonly and
        // kinds, not just the layout facts).
        Some("accessor under a computed key on the chain".to_string())
    } else {
        escape
    };
    if tpl.has_computed_members
        && tpl.birth_keys.is_empty()
        && !matches!(tpl.kind, TmplKind::FnInst { .. })
    {
        chain_problem.get_or_insert_with(|| "computed accessor/layout on the chain".into());
    }
    let unsummarized_native = tpl
        .native_parent
        .as_ref()
        .is_some_and(|(_, n)| !crate::builtins::native_parent_summarized(n));
    if unsummarized_native {
        chain_problem.get_or_insert_with(|| "E21 native parent without a summary".into());
    }
    let escape = global.clone().or(escape);
    let prefix_stable = escape.is_none() && hard.is_none() && chain_problem.is_none();
    let adds = m.adds_keys.get(&tm).copied().unwrap_or(0);
    // Slots: birth order, valid up to the first non-definite / accessor key.
    let mut fields = Vec::new();
    let mut slot_ok = true;
    let mut keys: Vec<Atom> = tpl.birth_keys.clone();
    // Fields written later (non-birth) are reported too.
    for t in &ts {
        if let Some(fk) = m.field_keys.get(t) {
            for &a in fk {
                if !keys.contains(&a) {
                    keys.push(a);
                }
            }
        }
    }
    let mut fixed_layout = prefix_stable && adds == 0;
    for (i, &a) in keys.iter().enumerate() {
        let birth = i < tpl.birth_keys.len();
        let is_accessor = accessor_keys.contains(&a);
        let definite = birth && tpl.definite.contains(&a) && !is_accessor;
        if birth && !definite {
            slot_ok = false;
            fixed_layout = false;
        }
        let slot = (birth && definite && slot_ok).then_some(i as u32);
        let w = m.writers.get(&(tm, a)).cloned().unwrap_or_default();
        let any = m.any_writers.get(&tm).cloned().unwrap_or_default();
        let readonly = escape.is_none()
            && hard.is_none()
            && w.post.is_empty()
            && any.post.is_empty()
            && !is_accessor;
        let mut kind = field_kind(m, &ts, a, definite);
        if escape.is_some() {
            kind.top = true;
        }
        let ta_stable = readonly && definite && ta_only_sealed(m, &ts, a);
        let ta_kind = if ta_stable {
            ta_one_kind(m, &ts, a)
        } else {
            None
        };
        let fns = if slot.is_some() && !is_accessor && escape.is_none() {
            field_bodies(m, &ts, a)
        } else {
            Vec::new()
        };
        fields.push(FieldFacts {
            fns,
            ta_stable,
            ta_kind,
            key: m.atoms[a as usize].clone(),
            slot,
            definite,
            data_field: !is_accessor,
            readonly,
            writers_ctor: w.ctor.len(),
            writers_post: w.post.len() + any.post.len(),
            kind,
        });
    }
    let (kind, module, class_id) = match &tpl.kind {
        TmplKind::Class {
            module,
            class_id,
            literal_shape,
            ..
        } => (
            if *literal_shape { "litclass" } else { "class" },
            Some(*module),
            Some(*class_id),
        ),
        TmplKind::Lit { module, .. } => ("lit", Some(*module), None),
        TmplKind::FnInst { func } => ("fninst", Some(m.funcs[*func as usize].module), None),
        _ => ("other", None, None),
    };
    TemplateFacts {
        id: tm,
        display: tpl.display.clone(),
        kind,
        module,
        class_id,
        escape,
        hard,
        chain: chain_problem,
        prefix_stable,
        fixed_layout,
        adds_keys: adds,
        birth_keys: tpl
            .birth_keys
            .iter()
            .map(|a| m.atoms[*a as usize].clone())
            .collect(),
        fields,
        sabotaged: false,
        ancestors: {
            let mut v = Vec::new();
            let mut cur = tpl.parent;
            while let Some(p) = cur {
                if let TmplKind::Class { class_id, .. } = m.tmpls[p as usize].kind {
                    v.push(class_id);
                }
                cur = m.tmpls[p as usize].parent;
            }
            v
        },
    }
}

/// The field holds only typed-array objects, none exposed, transferred,
/// resized or escaped.
/// The compiled bodies field `a` holds over every token of a layout, when
/// it holds nothing else (`bindings::bodies_of`); empty otherwise.
fn field_bodies(m: &Model, ts: &[Tok], a: Atom) -> Vec<crate::bindings::BodyRef> {
    let mut out = Vec::new();
    for &t in ts {
        let Some(&n) = m.fields.get(&(t, a)) else {
            continue;
        };
        let nd = &m.nodes[n as usize];
        if nd.pts.is_empty() && nd.prim == 0 {
            continue;
        }
        let b = crate::bindings::bodies_of(m, n);
        if b.is_empty() {
            return Vec::new();
        }
        out.extend(b);
    }
    out.sort();
    out.dedup();
    out
}

fn ta_only_sealed(m: &Model, ts: &[Tok], a: Atom) -> bool {
    let ns: Vec<NodeId> = ts
        .iter()
        .filter_map(|&t| m.fields.get(&(t, a)).copied())
        .collect();
    !ns.is_empty()
        && ns.iter().all(|&n| {
            let node = &m.nodes[n as usize];
            node.prim & !prim::UNDEF == 0 && {
                let toks = node.pts.to_sorted();
                !toks.is_empty() && toks.iter().all(|&x| ta_sealed_tok(m, x))
            }
        })
}

fn ta_sealed_tok(m: &Model, x: Tok) -> bool {
    matches!(m.toks[x as usize], TokKind::Obj(tm, _)
        if matches!(m.tmpls[tm as usize].kind, TmplKind::BObj { what: "TypedArray", .. }))
        && !m.hard.contains_key(&x)
        && !m.escaped.contains_key(&x)
}

/// The single recorded element kind of the typed arrays in field `a` of `t`.
fn ta_one_kind(m: &Model, ts: &[Tok], a: Atom) -> Option<u8> {
    let mut kind = None;
    for &t in ts {
        let Some(&n) = m.fields.get(&(t, a)) else {
            continue;
        };
        for x in m.nodes[n as usize].pts.to_sorted() {
            let TokKind::Obj(tm, _) = m.toks[x as usize] else {
                return None;
            };
            let k = *m.ta_elem.get(&tm)?;
            if kind.is_some_and(|c| c != k) {
                return None;
            }
            kind = Some(k);
        }
    }
    kind
}

fn field_kind(m: &Model, ts: &[Tok], a: Atom, definite: bool) -> KindFact {
    let mut k = KindFact {
        prims: if definite { 0 } else { prim::UNDEF },
        objs: Vec::new(),
        funcs: 0,
        builtin_objs: 0,
        top: ts.iter().any(|t| m.escaped.contains_key(t)),
    };
    let mut objs = Vec::new();
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    for &t in ts {
        let Some(&n) = m.fields.get(&(t, a)) else {
            continue;
        };
        let node = &m.nodes[n as usize];
        k.prims |= node.prim;
        for x in node.pts.to_sorted() {
            if !seen.insert(x) {
                continue;
            }
            match &m.toks[x as usize] {
                TokKind::Top => k.top = true,
                TokKind::Obj(tm, _) if m.tmpls[*tm as usize].kind.is_fact_unit() => {
                    objs.push(m.tmpls[*tm as usize].display.clone())
                }
                TokKind::Func(_, _) | TokKind::Bound(_) | TokKind::BuiltinFn => k.funcs += 1,
                _ => k.builtin_objs += 1,
            }
        }
    }
    objs.sort();
    objs.dedup();
    k.objs = objs;
    k
}

fn site_facts(g: &mut Gen, tfs: &[TemplateFacts]) -> Vec<SiteFacts> {
    let m = &g.m;
    let by_id: crate::pts::FxHashMap<TmplId, &TemplateFacts> =
        tfs.iter().map(|t| (t.id, t)).collect();
    let mut out = Vec::new();
    // One instance token per class: the receiver an entry guard admits.
    let mut class_tok: FxHashMap<TmplId, Tok> = FxHashMap::default();
    for (i, t) in m.toks.iter().enumerate() {
        if let TokKind::Obj(tm, c) = t {
            if *c == NO_CTX || !class_tok.contains_key(tm) {
                class_tok.insert(*tm, i as Tok);
            }
        }
    }
    // One fact per read site: records of the same site (a body walked
    // twice) unite their receivers.
    let mut first: FxHashMap<(u32, u32, u32, bool, bool), usize> = FxHashMap::default();
    let mut groups: Vec<(usize, Vec<u32>)> = Vec::new();
    // The functions only the runtime has (a promise's resolvers). Every
    // other function token is a body: compiled here, or another unit's,
    // held by its contract (a stand-in, a foreign class's method).
    let runtime: FxHashSet<FnIdx> = m.resolvers.values().map(|&t| m.tok_fn(t)).collect();
    for (i, s) in g.read_sites.iter().enumerate() {
        let k = (s.module, s.byte_offset, s.key, s.is_call, s.value_call);
        match first.get(&k) {
            Some(&gi) => groups[gi].1.push(s.recv),
            None => {
                first.insert(k, groups.len());
                groups.push((i, vec![s.recv]));
            }
        }
    }
    for (i, recvs) in &groups {
        let s = &g.read_sites[*i];
        let mut toks: Vec<Tok> = recvs
            .iter()
            .flat_map(|&r| m.nodes[r as usize].pts.to_sorted())
            .collect();
        toks.sort_unstable();
        toks.dedup();
        let prim = recvs.iter().fold(0, |p, &r| p | m.nodes[r as usize].prim);
        let top_recv = recvs
            .iter()
            .copied()
            .find(|&r| m.nodes[r as usize].pts.to_sorted().contains(&TOP))
            .unwrap_or(s.recv);
        let key = m.atoms[s.key as usize].clone();
        let (mut receivers, mut top, mut verdict, mut fixed_shape, mut targets) =
            judge(m, &by_id, &runtime, s, &key, &toks, prim, top_recv);
        // Design §11.4: a site whose receiver is the `this` of a method of
        // class C runs, in the typed body behind the entry guard
        // `class_id(this) == C`, on C's instances only: E0 when it is T0
        // over C's layout (the generic body keeps the full site). A
        // parameter typed with a class type is guarded the same way; one
        // typed with an object type is decided after the census.
        let mut guard_keys = None;
        if let Verdict::No(_) = verdict {
            let guards: Option<Vec<&crate::gen::ParamGuard>> =
                recvs.iter().map(|r| g.param_guard.get(r)).collect();
            match guards.as_deref() {
                Some([crate::gen::ParamGuard::Keys(k), rest @ ..])
                    if rest
                        .iter()
                        .all(|x| matches!(x, crate::gen::ParamGuard::Keys(k2) if k2 == k)) =>
                {
                    guard_keys = Some(k.clone());
                }
                _ => {}
            }
            let guarded = recvs
                .iter()
                .map(|r| match g.param_guard.get(r) {
                    Some(crate::gen::ParamGuard::Class(tm)) => Some(*tm),
                    _ => m.this_class.get(r).copied(),
                })
                .collect::<Option<Vec<_>>>();
            if let Some(tm) = guarded
                .filter(|g| g.windows(2).all(|w| w[0] == w[1]))
                .and_then(|g| g.first().copied())
            {
                if let Some(&it) = class_tok.get(&tm) {
                    let (r2, _, v2, f2, t2) =
                        judge(m, &by_id, &runtime, s, &key, &[it], 0, top_recv);
                    if matches!(v2, Verdict::T0Mono | Verdict::T0Set) {
                        (receivers, top, verdict, fixed_shape, targets) =
                            (r2, true, Verdict::E0, f2, t2);
                    }
                }
            }
        }
        let mut t: Vec<String> = targets.into_values().collect();
        t.sort();
        t.dedup();
        out.push(SiteFacts {
            module: s.module,
            byte_offset: s.byte_offset,
            key,
            is_call: s.is_call,
            receivers,
            top,
            verdict,
            fixed_shape,
            targets: t,
            guard_keys,
        });
    }
    out
}

/// The verdict of one site over receiver tokens `toks` (and primitive kinds
/// `prim`): receivers, ⊤, verdict, fixed shape, the bodies a call runs.
///
/// A call is proven when its lookup is the read rule's (every receiver
/// names the key by one definite data field of its layout, or resolves it
/// on its stable chain) and every value the lookups give, the callee set,
/// is a compiled body. Several bodies are fine: each runs in its own form.
/// A call through a value has the callee node's values as its set.
#[allow(clippy::too_many_arguments)]
fn judge(
    m: &Model,
    by_id: &FxHashMap<TmplId, &TemplateFacts>,
    runtime: &FxHashSet<FnIdx>,
    s: &crate::gen::ReadSite,
    key: &str,
    toks: &[Tok],
    prim: u16,
    top_recv: NodeId,
) -> (Vec<String>, bool, Verdict, bool, FxHashMap<FnIdx, String>) {
    if s.value_call {
        let top = toks.contains(&TOP);
        return match callee_set(m, runtime, toks, prim, top_recv) {
            Ok(t) if t.len() == 1 => (Vec::new(), top, Verdict::T0Mono, true, t),
            Ok(t) => (Vec::new(), top, Verdict::T0Set, true, t),
            Err(why) => (
                Vec::new(),
                top,
                Verdict::No(why),
                true,
                FxHashMap::default(),
            ),
        };
    }
    let mut receivers = Vec::new();
    let mut top = false;
    let mut verdict: Option<Verdict> = None;
    let mut slots: Vec<Option<u32>> = Vec::new();
    let mut fixed_shape = true;
    // A call: the nodes whose values the lookups give.
    let mut callee: Vec<NodeId> = Vec::new();
    for &t in toks {
        match &m.toks[t as usize] {
            TokKind::Top => {
                top = true;
                verdict.get_or_insert_with(|| {
                    if m.explain {
                        Verdict::No(format!(
                            "receiver from unmodeled code (TOP) <- {}",
                            m.top_origin(top_recv)
                        ))
                    } else {
                        Verdict::No("receiver from unmodeled code (TOP)".into())
                    }
                });
            }
            TokKind::Obj(..) if m.closed_marker(t) => {}
            TokKind::Obj(tm, _) => {
                let Some(tf) = by_id.get(tm) else {
                    receivers.push(m.tok_name(t));
                    verdict.get_or_insert(Verdict::No("receiver is not a fact unit".into()));
                    continue;
                };
                receivers.push(tf.display.clone());
                if !tf.prefix_stable {
                    let why = if let Some(e) = &tf.escape {
                        format!("escape: {e}")
                    } else if let Some(h) = &tf.hard {
                        format!("hard: {h}")
                    } else {
                        format!("chain: {}", tf.chain.clone().unwrap_or_default())
                    };
                    verdict.get_or_insert(Verdict::No(why));
                    continue;
                }
                if !tf.fixed_layout {
                    fixed_shape = false;
                }
                if s.is_call {
                    match lookup(m, tf, t, key, s.key) {
                        Ok(ns) => callee.extend(ns),
                        Err(why) => {
                            verdict.get_or_insert(Verdict::No(why));
                        }
                    }
                    continue;
                }
                match tf.fields.iter().find(|f| f.key == *key) {
                    Some(f) if f.definite && f.data_field && f.slot.is_some() => slots.push(f.slot),
                    Some(f) if !f.data_field => {
                        verdict.get_or_insert(Verdict::No("accessor key (E25)".into()));
                    }
                    Some(_) | None => {
                        // Not a definite birth field: a prototype member
                        // or a key added after birth.
                        verdict.get_or_insert(Verdict::No("key not a definite birth field".into()));
                    }
                }
            }
            other => {
                receivers.push(m.tok_name(t));
                let _ = other;
                verdict.get_or_insert(Verdict::No("receiver is a builtin/function value".into()));
            }
        }
    }
    // Birth-site tokens of one template are one receiver layout.
    let mut seen_r: FxHashSet<String> = FxHashSet::default();
    receivers.retain(|r| seen_r.insert(r.clone()));
    if toks.is_empty() && prim == 0 {
        verdict.get_or_insert(Verdict::No("dead site (no receiver flows)".into()));
    }
    if toks.is_empty() && prim != 0 {
        verdict.get_or_insert(Verdict::No("primitive receiver".into()));
    }
    let mut targets = FxHashMap::default();
    let verdict = match verdict {
        Some(v) => v,
        None if s.is_call => {
            callee.sort_unstable();
            callee.dedup();
            let mut ts: Vec<Tok> = callee
                .iter()
                .flat_map(|&n| m.nodes[n as usize].pts.to_sorted())
                .collect();
            ts.sort_unstable();
            ts.dedup();
            let p = callee.iter().fold(0, |p, &n| p | m.nodes[n as usize].prim);
            let top_node = callee
                .iter()
                .copied()
                .find(|&n| m.nodes[n as usize].pts.contains_tok(TOP))
                .unwrap_or(top_recv);
            match callee_set(m, runtime, &ts, p, top_node) {
                Ok(t) => {
                    let one = t.len() == 1 && receivers.len() == 1;
                    targets = t;
                    if one {
                        Verdict::T0Mono
                    } else {
                        Verdict::T0Set
                    }
                }
                Err(why) => Verdict::No(why),
            }
        }
        None => {
            let first = slots.first().copied().flatten();
            if slots.iter().all(|s| *s == first) {
                if receivers.len() == 1 {
                    Verdict::T0Mono
                } else {
                    Verdict::T0Set
                }
            } else {
                Verdict::No("slots differ across the receiver set".into())
            }
        }
    };
    (receivers, top, verdict, fixed_shape, targets)
}

/// The bodies a callee set holds, or why it is not a set of bodies: it
/// holds ⊤, a builtin function value, a bound function, a class, a function
/// only the runtime has (a resolver), or a value that is no
/// function (calling a primitive throws).
fn callee_set(
    m: &Model,
    runtime: &FxHashSet<FnIdx>,
    toks: &[Tok],
    prim: u16,
    top_node: NodeId,
) -> Result<FxHashMap<FnIdx, String>, String> {
    if toks.contains(&TOP) {
        return Err(if m.explain {
            format!("callee set holds ⊤ <- {}", m.top_origin(top_node))
        } else {
            "callee set holds ⊤".into()
        });
    }
    let mut out = FxHashMap::default();
    for &t in toks {
        let why = match &m.toks[t as usize] {
            TokKind::Func(f, _) if !runtime.contains(f) => {
                out.insert(*f, body_name(m, *f));
                continue;
            }
            TokKind::Obj(..) if m.closed_marker(t) => continue,
            TokKind::BuiltinFn | TokKind::Builtin(_) => {
                "callee set holds a builtin function value".into()
            }
            TokKind::Bound(_) => "callee set holds a bound function".into(),
            TokKind::ClassObj(_) => "callee set holds a class".into(),
            TokKind::Func(..) => "callee set holds a runtime function".into(),
            _ => "callee set holds a non-function value".into(),
        };
        return Err(why);
    }
    if prim != 0 {
        return Err("callee set holds a non-function value".into());
    }
    if out.is_empty() {
        return Err("dead site (no callee flows)".into());
    }
    Ok(out)
}

/// A body's name in a site record: a compiled body by its HIR id too (two
/// methods share a name), another unit's by its contract name.
fn body_name(m: &Model, f: FnIdx) -> String {
    let fi = &m.funcs[f as usize];
    if fi.func_id == u32::MAX {
        fi.name.clone()
    } else {
        format!("{}#{}", fi.name, fi.func_id)
    }
}

/// The read rule's lookup of `key` on receiver token `t` (layout `tf`): one
/// definite data birth field of the layout, or else, per holder, the first
/// prototype on the stable chain that holds the key (F5). The nodes whose
/// values the read gives; a holder that does not have the key and ends its
/// chain makes the read `undefined`, which no call admits.
fn lookup(
    m: &Model,
    tf: &TemplateFacts,
    t: Tok,
    key: &str,
    a: Atom,
) -> Result<Vec<NodeId>, String> {
    let computed = |h: Tok| {
        m.field_any
            .get(&h)
            .is_some_and(|&n| !m.nodes[n as usize].pts.is_empty())
    };
    let holds = |h: Tok| {
        m.fields.get(&(h, a)).copied().filter(|&n| {
            let nd = &m.nodes[n as usize];
            !nd.pts.is_empty() || nd.prim != 0
        })
    };
    if computed(t) {
        return Err("a computed key may hold it".into());
    }
    // A key the layout never writes (a read names it) is the chain's.
    match tf.fields.iter().find(|f| f.key == *key) {
        Some(f) if f.definite && f.data_field && f.slot.is_some() => {
            return Ok(holds(t).into_iter().collect());
        }
        Some(f) if !f.data_field => return Err("accessor key (E25)".into()),
        Some(f) if f.writers_ctor + f.writers_post > 0 || tf.birth_keys.contains(&f.key) => {
            return Err("key not a definite birth field".into());
        }
        _ if holds(t).is_some() => return Err("key not a definite birth field".into()),
        _ => {}
    }
    let mut out = Vec::new();
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    let mut todo = vec![t];
    while let Some(h) = todo.pop() {
        let protos: Vec<Tok> = m
            .proto_node
            .get(&h)
            .map(|&pn| m.nodes[pn as usize].pts.to_sorted())
            .unwrap_or_default();
        if protos.is_empty() {
            return Err("method not found on the chain".into());
        }
        for p in protos {
            if !seen.insert(p) {
                continue;
            }
            if computed(p) {
                return Err("a computed key may hold it".into());
            }
            match holds(p) {
                Some(n) => out.push(n),
                None => todo.push(p),
            }
        }
    }
    Ok(out)
}
