//! Constraint generation, part 1c: template birth layouts and the class chain.

use super::*;

impl<'a> Gen<'a> {
    pub(super) fn compute_birth_keys(&mut self, tm: TmplId, module: &Module) {
        // Another unit's class: its keys are its contract's.
        if !self.m.tmpls[tm as usize].birth_keys.is_empty() || self.stand_in_cc.contains_key(&tm) {
            return;
        }
        let mut keys = Vec::new();
        let mut definite = Vec::new();
        let mut reserved = Vec::new();
        let mut inert_definite = Vec::new();
        let mut inert_reads = Vec::new();
        if let Some(p) = self.m.tmpls[tm as usize].parent {
            if let TmplKind::Class { module: pm, .. } = self.m.tmpls[p as usize].kind {
                let pmod = self.modules[pm as usize].1;
                self.compute_birth_keys(p, pmod);
            }
            keys = self.m.tmpls[p as usize].birth_keys.clone();
            definite = self.m.tmpls[p as usize].definite.clone();
            reserved = self.m.tmpls[p as usize].reserved.clone();
            inert_definite = self.m.tmpls[p as usize].inert_definite.clone();
            inert_reads = self.m.tmpls[p as usize].inert_reads.clone();
        }
        if let Some((_, np)) = self.m.tmpls[tm as usize].native_parent.clone() {
            // Keys the native parent constructor defines (summary, E21).
            for k in crate::builtins::native_parent_keys(&np) {
                let a = self.m.atom(k);
                push_unique(&mut keys, a);
            }
        }
        if let Some(layout) = self
            .m
            .realm_parent
            .get(&tm)
            .and_then(|id| crate::builtins::construct_layout(id))
        {
            // `super(...)` runs the realm parent's declared construct on the
            // instance (`run_super_construct`): the fields its `fresh` result
            // defines are the instance's first own keys, present from then on
            // when every result defines them.
            for k in layout.keys {
                let a = self.m.atom(k);
                push_unique(&mut keys, a);
                if layout.definite.contains(&k) {
                    push_unique(&mut definite, a);
                }
            }
        }
        let TmplKind::Class {
            module: mi, name, ..
        } = self.m.tmpls[tm as usize].kind.clone()
        else {
            return;
        };
        let Some(c) = module.classes.iter().find(|c| c.name == name) else {
            return;
        };
        self.proto_first_class(tm, c);
        let mut declared = Vec::new();
        let phase = self.ctor_phase.get(&tm).cloned();
        // A parent constructor that leaks `this` lets an observer see the
        // instance before any key of this class is stored or defined. Asked
        // of the chain as it stands: `CtorPhase::parent_leaks` was taken
        // before the solve, when a parent value not yet settled counts as a
        // leak; `settle_parent_layouts` recomputes these keys once it is.
        let parent_leaks = !self.parent_pending(tm) && self.parent_chain_leaks(tm);
        // A parent constructor that may add a key out of the layout's order
        // leaves the instance ordered by an overflow: every key this class
        // adds then gets an entry there, so none is created by a bare store
        // (`CtorPhase::disordered`).
        let parent_disordered = self.parent_chain_disordered(tm);
        // The keys in the order construction defines them
        // (`perry_hir::private_names::construction_entries`): a private
        // field's entry is claimed at its declaration, so it holds the next
        // slot like a public field's (#8969). A key the constructor stores
        // that is no field of the class (a method or accessor name, an
        // inherited field, a computed string key) goes where the constructor
        // first stores it: before the first reservation stored after it, the
        // order the class's layout lists it in (codegen `own_keys`).
        let field_names: std::collections::BTreeSet<&str> = c
            .fields
            .iter()
            .filter(|f| !f.is_private)
            .map(|f| f.name.as_str())
            .collect();
        for f in perry_hir::private_names::construction_entries([c.fields.as_slice()]) {
            if let (Some(ph), perry_hir::ClassFieldOrigin::ConstructorStore, false) =
                (phase.as_ref(), f.origin, f.is_private)
            {
                if let Some(at) = ph.assigned_order.iter().position(|k| *k == f.name) {
                    for k in &ph.assigned_order[..at] {
                        if !field_names.contains(k.as_str()) {
                            let a = self.m.atom(k);
                            push_unique(&mut keys, a);
                        }
                    }
                }
            }
            let a = if f.is_private {
                let key = self.private_field_key(mi, c, &f.name);
                self.m.private_atom(&key)
            } else {
                self.m.atom(&f.name)
            };
            push_unique(&mut keys, a);
            match f.origin {
                perry_hir::ClassFieldOrigin::Declared => push_unique(&mut declared, a),
                // A reservation holds the hole until the constructor stores
                // it: present from birth on only when the construction phase
                // stores it before anyone can observe the instance
                // (`CtorPhase::definite`, below).
                perry_hir::ClassFieldOrigin::ConstructorStore => push_unique(&mut reserved, a),
                // A field is defined at the birth, in order: present from
                // birth on unless the instance could be observed before its
                // definition (`CtorPhase::late_fields`, a leaking parent);
                // then it is a reservation like a constructor store.
                perry_hir::ClassFieldOrigin::Definition | perry_hir::ClassFieldOrigin::Capture
                    if parent_leaks
                        || parent_disordered
                        || phase
                            .as_ref()
                            .is_some_and(|ph| ph.late_fields.contains(&f.name)) =>
                {
                    push_unique(&mut reserved, a)
                }
                perry_hir::ClassFieldOrigin::Definition | perry_hir::ClassFieldOrigin::Capture => {
                    push_unique(&mut definite, a)
                }
            }
        }
        self.m.tmpls[tm as usize].declared = declared;
        if let Some(ph) = phase {
            if !ph.m_calls.is_empty() {
                let names: Vec<Atom> = ph.m_calls.iter().map(|k| self.m.atom(k)).collect();
                self.m.rule_m.insert(tm, names);
            }
            if !ph.s_calls.is_empty() {
                let v: Vec<(Atom, Option<TmplId>)> = ph
                    .s_calls
                    .iter()
                    .map(|k| (self.m.atom(k), self.setter_owner(tm, k)))
                    .collect();
                self.m.rule_s.insert(tm, v);
            }
            for k in &ph.assigned_order {
                let a = self.m.atom(k);
                push_unique(&mut keys, a);
            }
            if !parent_leaks && !parent_disordered {
                for k in &ph.definite {
                    let a = self.m.atom(k);
                    push_unique(&mut definite, a);
                }
                // Rule I: a native parent's prototypes hold builtin
                // accessors no program definition names: no inert read.
                if !self.native_on_chain(tm) {
                    for k in &ph.inert_definite {
                        let a = self.m.atom(k);
                        push_unique(&mut inert_definite, a);
                    }
                    for k in &ph.inert_reads {
                        let a = self.m.atom(k);
                        push_unique(&mut inert_reads, a);
                    }
                }
            }
        }
        // A parent that is a value (§17.2) is not known before the solve
        // settles it: the keys its constructor adds first are unknown, so
        // no key of this class has a known slot or is known present (a read
        // may see `undefined` while the parent runs). Settled, the parent's
        // keys are the prefix (`settle_parent_layouts`).
        if self.parent_pending(tm) {
            definite.clear();
            reserved.clear();
            inert_definite.clear();
        } else if self.value_parent_on_chain(tm) && self.parent_chain_leaks(tm) {
            inert_definite.clear();
            let n = self.m.tmpls[tm as usize]
                .parent
                .map_or(0, |p| self.m.tmpls[p as usize].definite.len());
            definite.truncate(n);
        }
        reserved.retain(|a| !definite.contains(a));
        inert_definite.retain(|a| !definite.contains(a));
        if inert_definite.is_empty() {
            inert_reads.clear();
        }
        self.m.tmpls[tm as usize].inert_definite = inert_definite;
        self.m.tmpls[tm as usize].inert_reads = inert_reads;
        self.m.tmpls[tm as usize].birth_keys = keys;
        self.m.tmpls[tm as usize].definite = definite;
        self.m.tmpls[tm as usize].reserved = reserved;
    }

    /// This program's class of template `tm` (`None`: not a class template
    /// or not found).
    fn class_of(&self, tm: TmplId) -> Option<&'a Class> {
        let TmplKind::Class { module, name, .. } = &self.m.tmpls[tm as usize].kind else {
            return None;
        };
        self.modules[*module as usize]
            .1
            .classes
            .iter()
            .find(|c| &c.name == name)
    }

    /// R9STORE3 rule S: the key -> setter map of `tm`'s chain: for each key
    /// the nearest class (`tm` first, then its parents: 10.1.8 / 10.1.9 walk
    /// the prototypes in that order) that names it as a method, accessor or
    /// field decides; it counts when that class declares exactly one setter
    /// under it, no method or field (a field is an own key at construction)
    /// and the setter body has no `return` (it would end the walk's
    /// constructor). The walk stops at a class this program does not show.
    pub(crate) fn chain_setters(
        &self,
        tm: TmplId,
    ) -> std::collections::BTreeMap<String, &'a perry_hir::Function> {
        let mut out = std::collections::BTreeMap::new();
        let mut named = std::collections::BTreeSet::new();
        let mut cur = Some(tm);
        while let Some(t) = cur {
            let Some(c) = self.class_of(t) else {
                break;
            };
            let names = c
                .methods
                .iter()
                .map(|m| m.name.as_str())
                .chain(c.getters.iter().map(|(k, _)| k.as_str()))
                .chain(c.setters.iter().map(|(k, _)| k.as_str()))
                .chain(c.fields.iter().map(|f| f.name.as_str()));
            for k in names.collect::<Vec<_>>() {
                if !named.insert(k.to_string()) {
                    continue;
                }
                let mut ss = c.setters.iter().filter(|(n, _)| n == k);
                if let (Some((_, f)), None) = (ss.next(), ss.next()) {
                    if !c.methods.iter().any(|m| m.name == k)
                        && !c.fields.iter().any(|x| x.name == k)
                        && !crate::ctor_phase::has_return(&f.body)
                    {
                        out.insert(k.to_string(), f);
                    }
                }
            }
            if self.m.tmpls[t as usize].native_parent.is_some() {
                break;
            }
            cur = self.m.tmpls[t as usize].parent;
        }
        out
    }

    /// Rule S: the class on `tm`'s chain whose setter `k` resolves to
    /// (`chain_setters`).
    fn setter_owner(&self, tm: TmplId, k: &str) -> Option<TmplId> {
        let mut cur = Some(tm);
        while let Some(t) = cur {
            let c = self.class_of(t)?;
            if c.methods.iter().any(|m| m.name == k)
                || c.getters.iter().any(|(n, _)| n == k)
                || c.setters.iter().any(|(n, _)| n == k)
                || c.fields.iter().any(|f| f.name == k)
            {
                return c.setters.iter().any(|(n, _)| n == k).then_some(t);
            }
            cur = self.m.tmpls[t as usize].parent;
        }
        None
    }

    /// Some class on `tm`'s chain (itself too) extends a native constructor.
    fn native_on_chain(&self, tm: TmplId) -> bool {
        let mut cur = Some(tm);
        while let Some(c) = cur {
            if self.m.tmpls[c as usize].native_parent.is_some() {
                return true;
            }
            cur = self.m.tmpls[c as usize].parent;
        }
        false
    }

    /// Some class on `tm`'s chain has a parent value not settled to one
    /// class (§17.2).
    fn parent_pending(&self, tm: TmplId) -> bool {
        let mut cur = Some(tm);
        while let Some(c) = cur {
            let t = &self.m.tmpls[c as usize];
            if t.parent_value.is_some()
                && t.parent.is_none()
                && !self.m.realm_parent.contains_key(&c)
            {
                return true;
            }
            cur = t.parent;
        }
        false
    }

    /// `tm` is linked to a realm parent (`Model::realm_parent`) whose declared
    /// construct hands the instance to other code before `tm`'s own keys
    /// are defined.
    fn realm_parent_leaks(&self, tm: TmplId) -> bool {
        self.m
            .realm_parent
            .get(&tm)
            .is_some_and(|id| crate::builtins::construct_layout(id).is_none_or(|l| l.leaks))
    }

    /// Some class on `tm`'s chain takes its parent from a value (§17.2).
    fn value_parent_on_chain(&self, tm: TmplId) -> bool {
        let mut cur = Some(tm);
        while let Some(c) = cur {
            if self.m.tmpls[c as usize].parent_value.is_some() {
                return true;
            }
            cur = self.m.tmpls[c as usize].parent;
        }
        false
    }

    /// A constructor up `tm`'s chain may leak `this` before `tm`'s own
    /// stores run: one that leaks, a native parent, a parent value not
    /// settled, or another unit's class (its constructor is not seen).
    /// Whether `tm`'s constructor or one up its chain leaks `this`
    /// (published as `ClassContract::ctor_leaks`).
    pub(crate) fn chain_leaks(&self, tm: TmplId) -> bool {
        self.ctor_phase.get(&tm).map_or(true, |ph| ph.leaks) || self.parent_chain_leaks(tm)
    }

    /// Whether `tm`'s constructor, or one up its chain, may return an object
    /// other than its `this` (published as `ClassContract::ctor_returns`).
    /// `new` then yields the returned value, so the class's instance is not
    /// the whole result. Unsettled parents and another unit's constructors
    /// that return answer yes.
    pub(crate) fn chain_returns(&self, tm: TmplId) -> bool {
        if self.parent_pending(tm) {
            return true;
        }
        let mut cur = Some(tm);
        while let Some(c) = cur {
            if self.stand_in_cc.get(&c).is_some_and(|cc| cc.ctor_returns) {
                return true;
            }
            if let Some(f) = self.m.tmpls[c as usize].ctor {
                if self.m.ctor_returns_object(f) {
                    return true;
                }
            }
            cur = self.m.tmpls[c as usize].parent;
        }
        false
    }

    /// Some constructor up the chain may leave the instance with an
    /// out-of-order add behind it (`CtorPhase::disordered`).
    fn parent_chain_disordered(&self, tm: TmplId) -> bool {
        let mut cur = self.m.tmpls[tm as usize].parent;
        while let Some(p) = cur {
            if self.ctor_phase.get(&p).is_some_and(|ph| ph.disordered) {
                return true;
            }
            cur = self.m.tmpls[p as usize].parent;
        }
        false
    }

    /// R9BIRTH1 rule C: a derived class's construction phase again, with
    /// the keys its parent chain's constructors definitely leave as own data
    /// properties (`CtorPhase::definite`) present after `super()`: storing
    /// one is no add (`ctor_phase::analyze_in_present_m`). Parents first. Only
    /// for a chain of this program's classes that never leaks `this` before
    /// its keys are stored (no observer could delete one) and whose
    /// constructors return no value (`super()` binds `this` to the object
    /// the parent constructed).
    pub(super) fn ctor_phase_present(&mut self, modules: &[(&str, &'a Module)]) {
        let mut by_tm: FxHashMap<TmplId, (usize, &'a Class)> = FxHashMap::default();
        for mi in 0..modules.len() {
            for c in self.classes_of(mi) {
                by_tm.insert(self.mods[mi].classes[&c.name], (mi, c));
            }
        }
        let depth = |m: &Model, tm: TmplId| {
            let (mut d, mut cur) = (0usize, m.tmpls[tm as usize].parent);
            while let Some(p) = cur {
                d += 1;
                cur = m.tmpls[p as usize].parent;
            }
            d
        };
        let mut todo: Vec<TmplId> = by_tm
            .keys()
            .copied()
            .filter(|&tm| self.m.tmpls[tm as usize].parent.is_some())
            .collect();
        todo.sort_by_key(|&tm| (depth(&self.m, tm), tm));
        for tm in todo {
            if self.parent_chain_leaks(tm) {
                continue;
            }
            let mut present = std::collections::BTreeSet::new();
            let mut cur = self.m.tmpls[tm as usize].parent;
            let mut ok = true;
            while let Some(p) = cur {
                match (self.ctor_phase.get(&p), by_tm.get(&p)) {
                    (Some(ph), Some((_, pc)))
                        if !pc
                            .constructor
                            .as_ref()
                            .is_some_and(|f| crate::ctor_phase::returns_value(&f.body)) =>
                    {
                        present.extend(ph.definite.iter().cloned());
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
                cur = self.m.tmpls[p as usize].parent;
            }
            if !ok || present.is_empty() {
                continue;
            }
            let (mi, c) = by_tm[&tm];
            let shapes = crate::ctor_phase::plain_shapes(&modules[mi].1.classes);
            let setters = self.chain_setters(tm);
            let ph = crate::ctor_phase::analyze_in_present_ms(
                c,
                &shapes,
                &present,
                true,
                &setters,
                self.m.ns_pairs,
            );
            let followed = !ph.m_calls.is_empty() || !ph.s_calls.is_empty();
            self.ctor_phase.insert(tm, ph);
            if followed {
                self.settle_rule_m(modules, Some((tm, &present)));
            }
        }
    }

    /// R9BIRTH3 rule M. A class `P` whose constructor's `this.m()` call the
    /// phase followed (`CtorPhase::m_calls`) keeps it when no class below
    /// `P` defines `m` (every instance that runs the call finds P's method:
    /// 10.1.8 from its own prototype, a subclass's prototype first), or when
    /// the birth is the same with the call taken as a leak
    /// (`CtorPhase::same_birth`): then a subclass whose prototype holds
    /// another `m` only leaks at the call (`m_override_leaks`). Otherwise
    /// the phase takes the call as a leak, as before. `only`: settle this
    /// class (with its parent chain's present keys, rule C) instead of all.
    /// CLOSED-WORLD PREMISE (owner 2026-11-14): every class is in the program.
    pub(super) fn settle_rule_m(
        &mut self,
        modules: &[(&str, &'a Module)],
        only: Option<(TmplId, &std::collections::BTreeSet<String>)>,
    ) {
        let mut by_tm: FxHashMap<TmplId, (usize, &'a Class)> = FxHashMap::default();
        for mi in 0..modules.len() {
            for c in self.classes_of(mi) {
                by_tm.insert(self.mods[mi].classes[&c.name], (mi, c));
            }
        }
        let defines = |c: &Class, k: &str| {
            c.methods.iter().any(|m| m.name == k)
                || c.getters.iter().any(|(n, _)| n == k)
                || c.setters.iter().any(|(n, _)| n == k)
        };
        let mut tms: Vec<TmplId> = by_tm.keys().copied().collect();
        tms.sort_unstable();
        let empty = std::collections::BTreeSet::new();
        let todo: Vec<(TmplId, &std::collections::BTreeSet<String>)> = match only {
            Some((tm, present)) => vec![(tm, present)],
            None => tms.iter().map(|&tm| (tm, &empty)).collect(),
        };
        for (tm, present) in todo {
            let calls = match self.ctor_phase.get(&tm) {
                Some(ph) if !ph.m_calls.is_empty() || !ph.s_calls.is_empty() => ph
                    .m_calls
                    .union(&ph.s_calls)
                    .cloned()
                    .collect::<std::collections::BTreeSet<String>>(),
                _ => continue,
            };
            // Some class below `tm` defines a followed method.
            let overridden = tms.iter().any(|&d| {
                let mut cur = Some(d);
                let mut between = Vec::new();
                while let Some(x) = cur {
                    if x == tm {
                        return between.iter().any(|b: &TmplId| {
                            by_tm
                                .get(b)
                                .is_some_and(|(_, c)| calls.iter().any(|k| defines(c, k)))
                        });
                    }
                    between.push(x);
                    cur = self.m.tmpls[x as usize].parent;
                }
                false
            });
            if !overridden {
                continue;
            }
            let (mi, c) = by_tm[&tm];
            let shapes = crate::ctor_phase::plain_shapes(&modules[mi].1.classes);
            let off = crate::ctor_phase::analyze_in_present_m(
                c,
                &shapes,
                present,
                false,
                self.m.ns_pairs,
            );
            if !self.ctor_phase[&tm].same_birth(&off) {
                self.ctor_phase.insert(tm, off);
            }
        }
        // The classes that reach a followed call with another method there.
        let mut leaks = crate::pts::FxHashSet::default();
        for &c in &tms {
            let mut seen: Vec<TmplId> = vec![c];
            let mut cur = self.m.tmpls[c as usize].parent;
            while let Some(p) = cur {
                if let Some(ph) = self.ctor_phase.get(&p) {
                    if ph.m_calls.iter().chain(&ph.s_calls).any(|k| {
                        seen.iter()
                            .any(|s| by_tm.get(s).is_some_and(|(_, sc)| defines(sc, k)))
                    }) {
                        leaks.insert(c);
                        break;
                    }
                }
                seen.push(p);
                cur = self.m.tmpls[p as usize].parent;
            }
        }
        self.m_override_leaks = leaks;
    }

    pub(super) fn parent_chain_leaks(&self, tm: TmplId) -> bool {
        if self.m_override_leaks.contains(&tm) {
            return true;
        }
        // Rule K: a parent's closure key is a present key in this class's
        // constructor, whose read hands out a closure that holds `this`.
        let mut up = self.m.tmpls[tm as usize].parent;
        while let Some(p) = up {
            if self
                .ctor_phase
                .get(&p)
                .is_some_and(|ph| !ph.closure_keys.is_empty())
            {
                return true;
            }
            up = self.m.tmpls[p as usize].parent;
        }
        if self.m.tmpls[tm as usize].native_parent.is_some()
            || self.parent_pending(tm)
            || self.realm_parent_leaks(tm)
        {
            return true;
        }
        let mut cur = self.m.tmpls[tm as usize].parent;
        while let Some(p) = cur {
            if self.ctor_phase.get(&p).is_some_and(|ph| ph.leaks)
                || self.stand_in_cc.get(&p).is_some_and(|c| c.ctor_leaks)
                || self.m.tmpls[p as usize].native_parent.is_some()
                || self.realm_parent_leaks(p)
            {
                return true;
            }
            cur = self.m.tmpls[p as usize].parent;
        }
        false
    }

    /// After the solve (§17.2 item 3, DECISIONS 47): each class whose parent
    /// value settled to one class takes that class's keys as its layout
    /// prefix, and every class below it in turn; its own keys are definite
    /// only when no constructor up the chain leaks `this`. The solve ran
    /// with no definite keys for them (`compute_birth_keys`): it saw every
    /// read as possibly `undefined`, which holds for any settled parent.
    pub(crate) fn settle_parent_layouts(&mut self) {
        self.m.settle_parent_values();
        let modules = self.modules.clone();
        let mut redo: Vec<(TmplId, usize)> = Vec::new();
        for mi in 0..modules.len() {
            for c in self.classes_of(mi) {
                let tm = self.mods[mi].classes[&c.name];
                if self.value_parent_on_chain(tm) {
                    redo.push((tm, mi));
                }
            }
        }
        for &(tm, _) in &redo {
            self.m.tmpls[tm as usize].birth_keys.clear();
            self.m.tmpls[tm as usize].definite.clear();
            self.m.tmpls[tm as usize].reserved.clear();
        }
        for (tm, mi) in redo {
            self.compute_birth_keys(tm, modules[mi].1);
        }
    }
}
