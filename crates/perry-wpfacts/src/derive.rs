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

#[derive(Clone, Debug)]
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
}

#[derive(Clone, Debug)]
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
    pub call_target: Option<String>,
}

pub fn derive(g: &mut Gen) -> (Vec<TemplateFacts>, Vec<SiteFacts>) {
    let m = &mut g.m;
    let global = g.global_hard.clone().or_else(|| {
        m.objproto_accessor
            .then(|| "E23 accessor installed on Object.prototype".to_string())
    });
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
        let tf = template_facts(m, tm, &global, &objproto_hard);
        out.push(tf);
    }
    let sites = site_facts(g, &out);
    (out, sites)
}

/// The static prototype chain tokens of a template (plus every prototype
/// object a `new f()` / literal `__proto__` can name).
fn chain_tokens(m: &Model, tm: TmplId) -> Vec<Tok> {
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    let mut stack = Vec::new();
    if let Some(&t) = m.tok_index.get(&TokKind::Obj(tm)) {
        stack.push(t);
    }
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
    let t = m.tok(TokKind::Obj(tm));
    let tpl = m.tmpls[tm as usize].clone();
    let escape = m.escaped.get(&t).map(|r| m.reasons[*r as usize].clone());
    let hard = m
        .hard
        .get(&t)
        .map(|(k, r)| format!("{} {}", k.code(), m.reasons[*r as usize]));
    // The prototype chain must be stable: no HARD op or escape on any
    // prototype object, no accessor named after a birth key, no computed
    // accessor anywhere.
    let chain = chain_tokens(m, tm);
    let mut chain_problem: Option<String> = None;
    let mut accessor_keys: Vec<Atom> = tpl.accessor_keys.clone();
    if let Some(ks) = m.accessor_keys.get(&t) {
        accessor_keys.extend(ks.iter().copied());
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
    let escape = if let Some(reason) = global {
        Some(reason.clone())
    } else if m.objproto_accessor && escape.is_none() {
        Some("E23 accessor installed on Object.prototype".to_string())
    } else {
        escape
    };
    let prefix_stable = escape.is_none() && hard.is_none() && chain_problem.is_none();
    let adds = m.adds_keys.get(&tm).copied().unwrap_or(0);
    // Slots: birth order, valid up to the first non-definite / accessor key.
    let mut fields = Vec::new();
    let mut slot_ok = true;
    let mut keys: Vec<Atom> = tpl.birth_keys.clone();
    // Fields written later (non-birth) are reported too.
    if let Some(fk) = m.field_keys.get(&t) {
        for &a in fk {
            if !keys.contains(&a) {
                keys.push(a);
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
        let mut kind = field_kind(m, t, a, definite);
        if escape.is_some() {
            kind.top = true;
        }
        let ta_stable = readonly && definite && ta_only_sealed(m, t, a);
        let ta_kind = if ta_stable {
            ta_one_kind(m, t, a)
        } else {
            None
        };
        fields.push(FieldFacts {
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
fn ta_only_sealed(m: &Model, t: Tok, a: Atom) -> bool {
    let Some(&n) = m.fields.get(&(t, a)) else {
        return false;
    };
    let node = &m.nodes[n as usize];
    if node.prim & !prim::UNDEF != 0 {
        return false;
    }
    let toks = node.pts.to_sorted();
    !toks.is_empty()
        && toks.iter().all(|&x| {
            matches!(m.toks[x as usize], TokKind::Obj(tm)
                if matches!(m.tmpls[tm as usize].kind, TmplKind::BObj { what: "TypedArray", .. }))
                && !m.hard.contains_key(&x)
                && !m.escaped.contains_key(&x)
        })
}

/// The single recorded element kind of the typed arrays in field `a` of `t`.
fn ta_one_kind(m: &Model, t: Tok, a: Atom) -> Option<u8> {
    let &n = m.fields.get(&(t, a))?;
    let mut kind = None;
    for x in m.nodes[n as usize].pts.to_sorted() {
        let TokKind::Obj(tm) = m.toks[x as usize] else {
            return None;
        };
        let k = *m.ta_elem.get(&tm)?;
        if kind.is_some_and(|c| c != k) {
            return None;
        }
        kind = Some(k);
    }
    kind
}

fn field_kind(m: &Model, t: Tok, a: Atom, definite: bool) -> KindFact {
    let mut k = KindFact {
        prims: if definite { 0 } else { prim::UNDEF },
        objs: Vec::new(),
        funcs: 0,
        builtin_objs: 0,
        top: m.escaped.contains_key(&t),
    };
    let Some(&n) = m.fields.get(&(t, a)) else {
        return k;
    };
    let node = &m.nodes[n as usize];
    k.prims |= node.prim;
    let mut objs = Vec::new();
    for x in node.pts.to_sorted() {
        match &m.toks[x as usize] {
            TokKind::Top => k.top = true,
            TokKind::Obj(tm) if m.tmpls[*tm as usize].kind.is_fact_unit() => {
                objs.push(m.tmpls[*tm as usize].display.clone())
            }
            TokKind::Func(_) | TokKind::Bound(_) | TokKind::BuiltinFn => k.funcs += 1,
            _ => k.builtin_objs += 1,
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
    for s in &g.read_sites {
        let node = &m.nodes[s.recv as usize];
        let toks = node.pts.to_sorted();
        let key = m.atoms[s.key as usize].clone();
        let mut receivers = Vec::new();
        let mut top = false;
        let mut verdict: Option<Verdict> = None;
        let mut slots: Vec<Option<u32>> = Vec::new();
        let mut fixed_shape = true;
        // Keyed by the function itself: two methods may share a name.
        let mut targets: FxHashMap<FnIdx, String> = FxHashMap::default();
        for &t in &toks {
            match &m.toks[t as usize] {
                TokKind::Top => {
                    top = true;
                    verdict.get_or_insert_with(|| {
                        if m.explain {
                            Verdict::No(format!(
                                "receiver from unmodeled code (TOP) <- {}",
                                m.top_origin(s.recv)
                            ))
                        } else {
                            Verdict::No("receiver from unmodeled code (TOP)".into())
                        }
                    });
                }
                TokKind::Obj(tm) => {
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
                        // F5: the method resolves through the (stable) chain.
                        let tgt = resolve_method(m, t, s.key);
                        match tgt {
                            Some((f, name)) => {
                                targets.insert(f, name);
                            }
                            None => {
                                verdict.get_or_insert(Verdict::No(
                                    "method not resolved to one body".into(),
                                ));
                            }
                        }
                        continue;
                    }
                    match tf.fields.iter().find(|f| f.key == key) {
                        Some(f) if f.definite && f.data_field && f.slot.is_some() => {
                            slots.push(f.slot)
                        }
                        Some(f) if !f.data_field => {
                            verdict.get_or_insert(Verdict::No("accessor key (E25)".into()));
                        }
                        Some(_) | None => {
                            // Not a definite birth field: a prototype member
                            // or a key added after birth.
                            verdict.get_or_insert(Verdict::No(
                                "key not a definite birth field".into(),
                            ));
                        }
                    }
                }
                other => {
                    receivers.push(m.tok_name(t));
                    let _ = other;
                    verdict
                        .get_or_insert(Verdict::No("receiver is a builtin/function value".into()));
                }
            }
        }
        if toks.is_empty() && node.prim == 0 {
            verdict.get_or_insert(Verdict::No("dead site (no receiver flows)".into()));
        }
        if toks.is_empty() && node.prim != 0 {
            verdict.get_or_insert(Verdict::No("primitive receiver".into()));
        }
        let verdict = match verdict {
            Some(v) => v,
            None if s.is_call => {
                if targets.len() == 1 {
                    if receivers.len() == 1 {
                        Verdict::T0Mono
                    } else {
                        Verdict::T0Set
                    }
                } else {
                    Verdict::No(format!("{} method targets", targets.len()))
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
        let mut t: Vec<String> = targets.into_values().collect();
        t.sort();
        out.push(SiteFacts {
            module: s.module,
            byte_offset: s.byte_offset,
            key,
            is_call: s.is_call,
            receivers,
            top,
            verdict,
            fixed_shape,
            call_target: t.first().cloned(),
        });
    }
    out
}

/// F5: the single function `key` resolves to on token `t` (own field or
/// the first prototype that holds it), if exactly one.
fn resolve_method(m: &Model, t: Tok, key: Atom) -> Option<(FnIdx, String)> {
    let mut cur = vec![t];
    let mut seen: FxHashSet<Tok> = FxHashSet::default();
    while !cur.is_empty() {
        let mut found: Vec<Tok> = Vec::new();
        for &h in &cur {
            if let Some(&n) = m.fields.get(&(h, key)) {
                for x in m.nodes[n as usize].pts.to_sorted() {
                    if !found.contains(&x) {
                        found.push(x);
                    }
                }
            }
            if let Some(&n) = m.field_any.get(&h) {
                if !m.nodes[n as usize].pts.is_empty() {
                    return None;
                }
            }
        }
        if !found.is_empty() {
            if found.len() == 1 {
                if let TokKind::Func(f) = m.toks[found[0] as usize] {
                    return Some((f, m.funcs[f as usize].name.clone()));
                }
            }
            return None;
        }
        let mut next = Vec::new();
        for &h in &cur {
            if let Some(&pn) = m.proto_node.get(&h) {
                for p in m.nodes[pn as usize].pts.to_sorted() {
                    if seen.insert(p) {
                        next.push(p);
                    }
                }
            }
        }
        cur = next;
    }
    None
}
