//! What leaves the unit through a published value: its fields, nested layouts and forwarded tokens.

use super::*;

impl<'a> Gen<'a> {
    pub(super) fn contract_class(&self, tm: TmplId) -> bool {
        self.class_contracts.iter().any(|&(_, _, c)| c == tm)
    }

    /// A token a value contract describes (`contract_check`).
    pub(super) fn value_tok_ok(&self, t: Tok) -> bool {
        // A runtime-owned object: the contract names it (`ValueContract::shared`).
        if self.m.named_shared(t).is_some() {
            return true;
        }
        if let Some((tm, _)) = crate::bindings::sealed_owner(&self.m, t) {
            let d = &self.m.tmpls[tm as usize].display;
            if !self
                .unit_in
                .is_some_and(|u| u.broken(&format!("owner:{d}")))
            {
                return true;
            }
        }
        match self.m.toks[t as usize] {
            // DECISIONS 137: a marker with values beside it (DECISIONS 74)
            // stands for no value of its own: the callers' values arrive
            // with it in every node it reaches and are checked themselves.
            _ if self.m.is_marker(t) && self.m.beside_values(t) => true,
            // DECISIONS 162: the callers' argument, stored by their units.
            _ if self.cell_flow_toks.contains(&t) && self.m.own_param_members(t).is_some() => true,
            // A global builtin is the same object in every unit (§17.16 (a)).
            TokKind::Top | TokKind::BuiltinFn(_) | TokKind::Builtin(_) => true,
            TokKind::Obj(tm, _) | TokKind::ClassObj(tm) if self.stand_in_cc.contains_key(&tm) => {
                !self
                    .unit_in
                    .is_some_and(|u| u.broken(&self.m.tmpls[tm as usize].display))
            }
            TokKind::Obj(tm, _) | TokKind::ClassObj(tm) if self.contract_class(tm) => true,
            // DECISIONS 116: an instance of a class to give.
            TokKind::Obj(tm, _) if self.class_to_give(tm) => true,
            TokKind::Obj(..) if self.arr_ok(t) => true,
            TokKind::Obj(tm, _) => self.literal_ok(tm),
            // A contract function of this unit (made one above).
            TokKind::Func(f, _) if self.m.funcs[f as usize].func_id != u32::MAX => {
                self.m.contract_fns.contains(&f)
            }
            _ => self.stand_in_fc.contains_key(&t),
        }
    }

    /// A published literal object's fields go to another unit: their
    /// contents leave once (accessors run with unknown receivers). True when
    /// this added constraints.
    ///
    /// What its accessors return leaves beyond any contract (DECISIONS
    /// 203/212): a reader's read under an accessor key runs the owner's
    /// code and finds an unknown value (a stand-in's getter,
    /// `Gen::stand_in_getter`; an accessor field of a literal layout is ⊤
    /// there), so nothing it reaches is read through a contract. Another
    /// unit's stand-in a getter returns is this unit's report, not a value
    /// forwarded with its owner's contract.
    ///
    /// A literal of this unit (`nested_ok`) leaves key by key instead
    /// (design §18.7, `nested_fields`): what it holds under a computed key
    /// leaves here, each named field once `contract_check` decided it.
    pub(super) fn fields_leave(&mut self, t: Tok, r: u32) -> bool {
        if !self.ret_ok.insert(t) {
            return false;
        }
        match self.m.toks[t as usize] {
            TokKind::Obj(tm, _) if self.keyed_leave(tm) => {
                self.leaving.push(t);
                let any = self.m.field_any(t);
                self.m.sink_node(any, r);
            }
            _ => self.sink_all_fields(t, r),
        }
        // What its accessors return leaves beyond any contract, except its
        // contract getters' results, which their contracts check
        // (`note_own_getters`).
        let rl = self.m.node_l("contract-ret-load");
        let ra = self.m.beyond_contract(r);
        self.m.sink_node(rl, ra);
        self.m.leave_walk(t, rl);
        true
    }

    /// A value a field of this unit's published literal may forward with
    /// its owner's contract (DECISIONS 88): another unit's unbroken
    /// stand-in, a builtin, an unknown value.
    fn forwarded_tok(&self, t: Tok) -> bool {
        match self.m.toks[t as usize] {
            TokKind::Top | TokKind::BuiltinFn(_) | TokKind::Builtin(_) => true,
            TokKind::Obj(tm, _) | TokKind::ClassObj(tm) => {
                self.stand_in_cc.contains_key(&tm)
                    && !self
                        .unit_in
                        .is_some_and(|u| u.broken(&self.m.tmpls[tm as usize].display))
            }
            _ => self.stand_in_fc.contains_key(&t),
        }
    }

    fn sink_all_fields(&mut self, t: Tok, r: u32) {
        if self.all_sunk.insert(t) {
            let all = self.m.all_fields(t);
            self.m.cons(all, Cons::TopSink { reason: r });
        }
    }

    /// Another unit's layout a published value's field may hold as its
    /// owner publishes it (a stand-in no census broke).
    pub(crate) fn forwarded_layout(&self, tm: TmplId) -> bool {
        self.stand_in_cc.get(&tm).is_some_and(|c| c.literal_shape)
            && !self
                .unit_in
                .is_some_and(|u| u.broken(&self.m.tmpls[tm as usize].display))
    }

    /// A literal layout of this unit a published value's field may hold
    /// as a nested contract (design §18.7): no census broke it, it is no
    /// stand-in or contract class. (A closed callee of another unit that
    /// gets it as an argument sees no nested fields; what it does to them
    /// comes back through its parameter's reach uses, decision 13.)
    fn nested_ok(&self, tm: TmplId) -> bool {
        self.literal_ok(tm) && !self.stand_in_cc.contains_key(&tm) && !self.contract_class(tm)
    }

    /// A published object whose named fields leave key by key
    /// (`nested_fields`): a literal layout of this unit (§18.7) or an
    /// instance of a contract class (DECISIONS 88: its contract describes
    /// each field as a literal's does).
    fn keyed_leave(&self, tm: TmplId) -> bool {
        self.nested_ok(tm) || self.contract_class(tm)
    }

    /// Design §18.7: the named fields of the published literals that leave
    /// key by key, decided per template and key over every instance that
    /// leaves. A field holding only instances of literal layouts this unit
    /// may publish (`nested_ok`, none escaped) is described by those
    /// layouts, which leave the same way; instances of another unit's
    /// layout no census broke (a stand-in) are described by its owner's
    /// contract, forwarded: nothing of this unit leaves through their
    /// fields (DECISIONS 79, as for a published value and a contract
    /// result, `value_tok_ok`); any other content and the field
    /// is sunk for good. A field holding only primitives so far stays
    /// undecided (nothing leaves through it; a reader sees ⊤). Re-run to
    /// the fixpoint by `contract_check`: a described field whose contents
    /// grow is checked again. True when this added constraints.
    pub(super) fn nested_fields(&mut self, r: u32) -> bool {
        let mut changed = false;
        loop {
            let n0 = self.leaving.len();
            let h0 = self.handed.len();
            let mut by: FxHashMap<TmplId, Vec<Tok>> = FxHashMap::default();
            let lv: crate::pts::FxHashSet<Tok> = self.leaving.iter().copied().collect();
            for &t in self
                .leaving
                .iter()
                .chain(self.handed.iter().filter(|t| !lv.contains(t)))
            {
                if let Some(tm) = self.m.tmpl_of(t) {
                    by.entry(tm).or_default().push(t);
                }
            }
            let mut tms: Vec<TmplId> = by.keys().copied().collect();
            tms.sort_unstable();
            for tm in tms {
                let ts = by.remove(&tm).unwrap_or_default();
                // Only a leaving token's fields are sunk; a handed one's
                // (`handed`) are described or read as unknown by the callee.
                let leaves: Vec<Tok> = ts.iter().copied().filter(|t| lv.contains(t)).collect();
                let handed_only = leaves.is_empty();
                if !self.keyed_leave(tm) {
                    // Broken by the census: every field leaves as before.
                    self.field_layouts.retain(|&(t2, _), _| t2 != tm);
                    self.field_values.retain(|&(t2, _), _| t2 != tm);
                    self.field_tas.retain(|&(t2, _), _| t2 != tm);
                    for &t in &leaves {
                        if !self.all_sunk.contains(&t) {
                            self.sink_all_fields(t, r);
                            changed = true;
                        }
                    }
                    continue;
                }
                let mut keys: Vec<Atom> = ts
                    .iter()
                    .flat_map(|t| self.m.field_keys.get(t).into_iter().flatten().copied())
                    .collect();
                keys.sort_unstable();
                keys.dedup();
                for a in keys {
                    // A hidden capture field (`__perry_cap_*`) is the class's
                    // environment, not the instance's data (DECISIONS 117,
                    // 128): reflection never shows it (the runtime's
                    // `is_class_capture_key`), and only the class's own
                    // members read it, in this unit. It does not leave.
                    if perry_hir::cap_fields::cap_field_outer_id(&self.m.atoms[a as usize])
                        .is_some()
                    {
                        continue;
                    }
                    // A class private field (`#x`) has no name outside its
                    // class body: no other unit's code names it, and no
                    // reflection lists it (`Model::private_atom`). Only the
                    // class's own members read it, all in this unit; what it
                    // holds leaves only as they let it. It does not leave.
                    if self.m.is_private_atom(a) {
                        continue;
                    }
                    let mut ok = !self.sunk_fields.contains(&(tm, a));
                    let mut inner: Vec<(TmplId, Tok)> = Vec::new();
                    let mut prim = false;
                    let mut fwd = false;
                    let mut arrs: Vec<Tok> = Vec::new();
                    // DECISIONS 126: this unit's typed arrays, none escaped
                    // or hard, in a field no reader reported.
                    let mut tas: Vec<Tok> = Vec::new();
                    // DECISIONS 133: instances of this unit's contract
                    // classes, none escaped.
                    let mut cls: Vec<Tok> = Vec::new();
                    let mut pbits: u16 = 0;
                    let reported = self.ta_reported.contains(&(
                        self.m.tmpls[tm as usize].display.clone(),
                        self.m.atoms[a as usize].clone(),
                    ));
                    if ok {
                        'toks: for &t in &ts {
                            let Some(&n) = self.m.fields.get(&(t, a)) else {
                                continue;
                            };
                            prim |= self.m.nodes[n as usize].prim != 0;
                            pbits |= self.m.nodes[n as usize].prim;
                            for x in self.m.nodes[n as usize].pts.to_sorted() {
                                match self.m.toks[x as usize] {
                                    TokKind::Obj(xtm, _)
                                        if !reported
                                            && matches!(
                                                self.m.tmpls[xtm as usize].kind,
                                                TmplKind::BObj {
                                                    what: "TypedArray",
                                                    ..
                                                }
                                            )
                                            && !self.stand_in_tas.iter().any(|&(_, s)| s == x)
                                            && !self.m.is_escaped(x)
                                            && !self.m.hard.contains_key(&x) =>
                                    {
                                        tas.push(x)
                                    }
                                    TokKind::Obj(itm, _)
                                        if self.nested_ok(itm) && !self.m.is_escaped(x) =>
                                    {
                                        inner.push((itm, x))
                                    }
                                    // An instance of this unit's contract
                                    // class (one a caller handed back to
                                    // its constructor, DECISIONS 133): the
                                    // field's published kind names the
                                    // class (DECISIONS 110), and the
                                    // instance leaves key by key itself.
                                    TokKind::Obj(itm, _)
                                        if self.contract_class(itm)
                                            && !self.stand_in_cc.contains_key(&itm)
                                            && !self.m.is_escaped(x) =>
                                    {
                                        cls.push(x)
                                    }
                                    // An array: its elements are a cell
                                    // (DECISIONS 94).
                                    _ if self.arr_ok(x) => {
                                        fwd = true;
                                        arrs.push(x);
                                    }
                                    // Another unit's stand-in (a builtin, an
                                    // unknown value): forwarded with its
                                    // owner's contract (DECISIONS 88).
                                    _ if self.forwarded_tok(x) => fwd = true,
                                    // DECISIONS 164: a marker with values
                                    // beside it (a published method's
                                    // receiver reach, DECISIONS 107, or an
                                    // argument marker's reach over such)
                                    // stands for no value of its own
                                    // (DECISIONS 74): the values beside it
                                    // are judged here themselves.
                                    _ if self.m.is_marker(x) && self.m.beside_values(x) => {}
                                    _ => {
                                        ok = false;
                                        break 'toks;
                                    }
                                }
                            }
                        }
                    }
                    // DECISIONS 126: only typed arrays (and `undefined`
                    // before the field is written): the owner's typed arrays,
                    // described; mixed with anything else, sunk.
                    if ok && !tas.is_empty() {
                        if fwd
                            || !inner.is_empty()
                            || !cls.is_empty()
                            || pbits & !crate::model::prim::UNDEF != 0
                        {
                            ok = false;
                        } else {
                            let mut elem = None;
                            let mut one = true;
                            for &x in &tas {
                                let k = self
                                    .m
                                    .tmpl_of(x)
                                    .and_then(|xtm| self.m.ta_elem.get(&xtm).copied());
                                match (elem, k) {
                                    (_, None) => one = false,
                                    (None, Some(k)) => elem = Some(k),
                                    (Some(e), Some(k)) if e != k => one = false,
                                    _ => {}
                                }
                            }
                            let v = (elem.filter(|_| one), pbits);
                            if self.field_tas.insert((tm, a), v) != Some(v) {
                                changed = true;
                            }
                            self.field_layouts.remove(&(tm, a));
                            self.field_values.remove(&(tm, a));
                            continue;
                        }
                    }
                    // A handed literal's instance of a contract class would
                    // leave key by key: not described here.
                    if handed_only && !cls.is_empty() {
                        ok = false;
                    }
                    if ok {
                        for x in std::mem::take(&mut cls) {
                            changed |= self.fields_leave(x, r);
                        }
                    }
                    // Forwarded stand-ins (beside this unit's layouts, which
                    // leave as nested contracts): one value contract
                    // describes the field (DECISIONS 88).
                    if ok && fwd {
                        for x in arrs {
                            changed |= self.arr_leaves(x);
                        }
                        let ns: Vec<NodeId> = ts
                            .iter()
                            .filter_map(|&t| self.m.fields.get(&(t, a)).copied())
                            .collect();
                        self.field_values.insert((tm, a), ns);
                        self.field_layouts.remove(&(tm, a));
                        for (_, x) in inner {
                            changed |= self.inner_leave(x, handed_only, r);
                        }
                        continue;
                    }
                    if ok && inner.is_empty() {
                        continue;
                    }
                    if ok {
                        let mut itms: Vec<TmplId> = inner.iter().map(|&(i, _)| i).collect();
                        itms.sort_unstable();
                        itms.dedup();
                        self.field_layouts.insert((tm, a), itms);
                        if prim {
                            self.field_prims.insert((tm, a));
                        }
                        for (_, x) in inner {
                            changed |= self.inner_leave(x, handed_only, r);
                        }
                        continue;
                    }
                    // Its readers find an unknown value: what it holds leaves
                    // beyond any contract (DECISIONS 203, 212), unless it
                    // holds only functions they call through their
                    // contracts (`note_field_fns`).
                    self.sunk_fields.insert((tm, a));
                    self.field_layouts.remove(&(tm, a));
                    self.field_values.remove(&(tm, a));
                    self.field_tas.remove(&(tm, a));
                    self.field_prims.remove(&(tm, a));
                    let ru = self.m.reason(UNDESCRIBED);
                    for &t in &leaves {
                        if let Some(&n) = self.m.fields.get(&(t, a)) {
                            if self.sunk_field_toks.insert((t, a)) {
                                let rf = match self.field_fns.contains_key(&(t, a)) {
                                    true => r,
                                    false => ru,
                                };
                                self.m.sink_node(n, rf);
                                changed = true;
                            }
                        }
                    }
                }
            }
            if self.leaving.len() == n0 && self.handed.len() == h0 {
                break;
            }
        }
        changed
    }

    /// A literal of this unit a described field holds: it leaves with its
    /// holder (`fields_leave`), or is handed with it (`handed`).
    fn inner_leave(&mut self, x: Tok, handed: bool, r: u32) -> bool {
        if !handed {
            return self.fields_leave(x, r);
        }
        if !self.handed.contains(&x) {
            self.handed.push(x);
        }
        false
    }
}
