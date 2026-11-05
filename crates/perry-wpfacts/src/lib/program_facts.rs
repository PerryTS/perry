use super::*;

impl ProgramFacts {
    /// DECISIONS 225: `s`, a site of another run (`path` its module), is
    /// dead in this whole-program run: no view of it here, and activation
    /// proved that no body holding it can run. Any fact about it is
    /// vacuously true, so the other run proving it is not stronger. `false`
    /// when every body was walked.
    pub fn dead_site(&self, path: &str, s: &SiteFacts) -> bool {
        let Some(on) = &self.active_bodies else {
            return false;
        };
        !self.sites.iter().any(|w| {
            self.module_paths[w.module as usize] == path
                && w.byte_offset == s.byte_offset
                && w.key == s.key
                && w.is_call == s.is_call
        }) && s
            .bodies
            .iter()
            .all(|(id, name)| !on.contains(&(path.to_owned(), *id, name.clone())))
    }

    /// Conservative program-wide metadata effect contract. Handing a shared
    /// object to another analyzed unit is not itself a mutation; every actual
    /// mutation or exposure to unmodeled code withholds this proof.
    pub fn indexed_metadata_pristine(&self) -> bool {
        !self.stats.aborted && self.interp_guard.is_none() && self.indexed_metadata_unwritten
    }

    fn module_index(&self, path: &str) -> Option<usize> {
        self.module_paths
            .binary_search_by(|p| p.as_str().cmp(path))
            .ok()
    }

    /// The digest of the facts module `path` can observe (its own templates
    /// and the templates in its sites' receiver sets). Goes into the
    /// module's object-cache key (design section 7).
    pub fn digest_for(&self, path: &str) -> u64 {
        match self.module_index(path) {
            Some(i) => self.module_digests[i],
            None => 0,
        }
    }

    /// The contract-build claims for the class templates `path` defines:
    /// the `.perry_facts` payload (empty when nothing is claimed).
    pub fn contract_section(&self, path: &str) -> Vec<u8> {
        match self.module_index(path) {
            Some(i) => dump::contract_section(self, i as u32),
            None => Vec::new(),
        }
    }

    /// The classes whose layout facts hold for every instance, by class id.
    /// A class id names one runtime class, and structurally identical
    /// literal classes of several modules share it: its facts hold only when
    /// every template with that id is prefix-stable and they agree.
    pub fn stable_classes(&self) -> std::collections::HashMap<u32, &TemplateFacts> {
        let mut by_class: std::collections::HashMap<u32, Vec<&TemplateFacts>> = Default::default();
        for t in &self.templates {
            if let Some(c) = t.class_id {
                by_class.entry(c).or_default().push(t);
            }
        }
        by_class
            .into_iter()
            .filter(|(_, ts)| {
                ts.iter().all(|t| t.prefix_stable) && ts.iter().all(|t| t.fields == ts[0].fields)
            })
            .map(|(c, ts)| (c, ts[0]))
            .collect()
    }

    /// The class ids of the objects a field's kind names (its `objs`, by
    /// display), sorted. `None` when one of them is no class instance (a
    /// literal or function-instance template) or its display does not name
    /// exactly one class. Says nothing about the kind's other members (⊤,
    /// functions, builtins, primitives): the caller decides on those.
    pub fn classes_of(&self, k: &derive::KindFact) -> Option<Vec<u32>> {
        let mut out = Vec::with_capacity(k.objs.len());
        for d in &k.objs {
            let mut ids = self
                .templates
                .iter()
                .filter(|t| t.display == *d)
                .map(|t| t.class_id);
            let cid = ids.next()??;
            if !ids.all(|c| c == Some(cid)) {
                return None;
            }
            out.push(cid);
        }
        out.sort_unstable();
        out.dedup();
        Some(out)
    }

    /// Receiver facts of module `path`'s bindings (empty when unknown).
    pub fn bindings_for(&self, path: &str) -> Option<&ModuleBindings> {
        self.bindings.get(self.module_index(path)?)
    }

    /// The human/oracle-readable facts dump.
    pub fn dump_text(&self) -> String {
        dump::dump_text(self)
    }

    /// Dump only: writes the diagnostic records streamed to the sidecar,
    /// after `dump_text`.
    pub fn append_streamed(&self, out: &mut impl std::io::Write) -> std::io::Result<()> {
        dump::append_streamed(self, out)
    }

    /// The constraint relations for `scripts/wpfacts_recompute.py`.
    pub fn relations_text(&self) -> Option<String> {
        dump::relations_text(self)
    }
}

pub(super) fn big_nodes(m: &model::Model) -> Vec<(String, usize, String)> {
    let mut sizes: Vec<(usize, usize)> = m
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.pts.len(), i))
        .filter(|(l, _)| *l > 64)
        .collect();
    sizes.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    let count_big = sizes.len();
    let mut out: Vec<(String, usize, String)> = sizes
        .into_iter()
        .take(20)
        .map(|(len, i)| {
            let mut mix: std::collections::BTreeMap<&'static str, usize> = Default::default();
            for t in m.nodes[i].pts.to_sorted() {
                let k = match &m.toks[t as usize] {
                    model::TokKind::Top => "top",
                    model::TokKind::Obj(tm, _) => match m.tmpls[*tm as usize].kind {
                        model::TmplKind::Class { .. } => "class",
                        model::TmplKind::Lit { .. } => "lit",
                        model::TmplKind::FnInst { .. } => "fninst",
                        model::TmplKind::Arr { .. } => "arr",
                        _ => "bobj",
                    },
                    model::TokKind::Func(_, _) => "func",
                    model::TokKind::Bound(_) => "bound",
                    model::TokKind::ClassObj(_) => "classobj",
                    model::TokKind::Proto(_) => "proto",
                    _ => "builtin",
                };
                *mix.entry(k).or_default() += 1;
            }
            let label = m.labels.get(i).copied().unwrap_or("?");
            // A field node names its holder and key; any node its function.
            let mut owner = m
                .fields
                .iter()
                .find(|(_, &n)| n as usize == i)
                .map(|(&(t, a), _)| format!("({}.{})", m.tok_name(t), m.atoms[a as usize]))
                .unwrap_or_default();
            if let Some(&f) = m.node_fn.get(i).filter(|&&f| f != model::NO_FN) {
                owner.push_str(&format!("@{}", m.funcs[f as usize].name));
            }
            (
                format!("{label}#{i}{owner}"),
                len,
                mix.iter()
                    .map(|(k, v)| format!("{k}:{v}"))
                    .collect::<Vec<_>>()
                    .join(","),
            )
        })
        .collect();
    out.push((format!("nodes>64:{count_big}"), 0, String::new()));
    out
}

/// Rule R1's fields (`Model::builtin_field`): every field of a holder that
/// took one under a key it does not declare (the native function values'
/// shared holder: all of its fields), the ten largest by set size.
pub(super) fn builtin_fields(m: &model::Model) -> (usize, Vec<(String, usize)>, [u64; 4]) {
    let any = m
        .tok_index
        .get(&model::TokKind::BuiltinFn(model::Native::Any))
        .copied();
    let mut v: Vec<(usize, String)> = Vec::new();
    let mut hs: Vec<model::Tok> = m.r1_holders.iter().copied().collect();
    hs.sort_unstable();
    for h in hs {
        for &a in m.field_keys.get(&h).map(|v| v.as_slice()).unwrap_or(&[]) {
            if Some(h) != any && crate::builtins::undeclared(m, h, a).is_none() {
                continue;
            }
            if let Some(&n) = m.fields.get(&(h, a)) {
                let name = format!("{}.{}", m.tok_name(h), m.atoms[a as usize]);
                v.push((m.nodes[n as usize].pts.len(), name));
            }
        }
    }
    let count = v.len();
    v.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let top = v.into_iter().take(10).map(|(n, l)| (l, n)).collect();
    (count, top, m.r1_sinks)
}
