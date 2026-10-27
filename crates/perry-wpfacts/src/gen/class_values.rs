use super::*;

impl<'a> Gen<'a> {
    /// DECISIONS 106: one name, one contract, its defining unit's. A copy
    /// a cell or another unit's contract forwards only names the function:
    /// a copy is a snapshot of some earlier round, and copies circulating
    /// through forwarding units would sustain a stale contract. Read like a
    /// cell: the owner may solve after this unit, which is then solved
    /// again when the contract changes.
    pub(super) fn owner_fn(
        &mut self,
        c: &'a crate::units::FnContract,
    ) -> &'a crate::units::FnContract {
        if !self.fns_read.iter().any(|(x, _)| *x == c.name) {
            let seen = self.unit_in.map_or_else(
                || crate::units::digest(&None::<&crate::units::FnContract>),
                |u| u.own_fn_seen(&c.name),
            );
            self.fns_read.push((c.name.clone(), seen));
        }
        self.unit_in.and_then(|u| u.own_fn(&c.name)).unwrap_or(c)
    }

    /// DECISIONS 107: a published method runs on what the units running it
    /// published (`Published::this_args`, the join over every unit; each
    /// admits only instances of the method's class chain, `admit_this`).
    /// Described as instances of layouts, with no unit over the bound, its
    /// `this` holds them in place of the class's receiver token
    /// (`Model::escape_this`); this unit's own display is its own class's
    /// instance. A changed join solves the owner again (`units::run`).
    /// DECISIONS 123: no receiver published yet (the running units solve
    /// after the owner) is no receiver, as a closed function's parameters
    /// hold nothing before its callers publish (decision 13): the join only
    /// grows from there.
    pub(crate) fn published_receivers(&mut self) {
        let Some(unit_in) = self.unit_in else {
            return;
        };
        if unit_in.this_args_top || unit_in.eval_scopes() {
            return;
        }
        let mut none = None;
        for f in 0..self.m.funcs.len() as FnIdx {
            let fi = &self.m.funcs[f as usize];
            if fi.module != 0
                || fi.func_id == u32::MAX
                || fi.lexical_this
                || fi.ctor_of.is_some()
                || unit_in.broken(&format!("fn:{}", fi.name))
            {
                continue;
            }
            let name = fi.name.clone();
            let Some(vc) = unit_in.this_args.get(&name) else {
                let n = *none.get_or_insert_with(|| self.m.node_l("this-args-none"));
                self.m.this_args.insert(f, n);
                continue;
            };
            let described = !vc.unknown
                && vc.prim == 0
                && !vc.layouts.is_empty()
                && vc.classes.is_empty()
                && vc.fns.is_empty()
                && vc.builtins.is_empty()
                && vc.arrays.is_empty()
                && vc.boxes.is_empty()
                && vc.layouts.iter().all(|c| !unit_in.broken(&c.display));
            if !described {
                continue;
            }
            let n = self
                .m
                .node_named("this-args", || format!("this-args({name})"));
            for cc in &vc.layouts {
                let own = (0..self.m.tmpls.len()).find(|&i| {
                    let t = &self.m.tmpls[i];
                    t.display == cc.display
                        && matches!(t.kind, TmplKind::Class { .. } | TmplKind::Lit { .. })
                        && !self.stand_ins.values().any(|s| s.1 as usize == i)
                        && !self.m.foreign.contains_key(&(i as TmplId))
                });
                let tm = match own {
                    Some(i) => i as TmplId,
                    None => {
                        self.stand_in_class(0, cc);
                        self.stand_ins[&cc.display].1
                    }
                };
                let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
                self.m.add_tok(n, inst);
            }
            self.m.this_args.insert(f, n);
        }
    }

    /// DECISIONS 165: a published method no unit runs (`Published::method_runs`,
    /// the join over every unit) is called only by this unit's code: its
    /// parameter markers stand for no value of another unit (as a closed
    /// function's with no callers, decision 13), so what this unit passes
    /// is what its parameters hold (`Model::beside_values`). The join only
    /// grows, and a newly run method solves the owner again (`units::run`).
    /// A unit over the bound may run any; a call through an escaped method
    /// is a census report that breaks it. An accessor runs on a get or a
    /// set, not a call: not here.
    pub(crate) fn published_args(&mut self) {
        let Some(unit_in) = self.unit_in else {
            return;
        };
        if unit_in.this_args_top || unit_in.eval_scopes() {
            return;
        }
        for f in 0..self.m.funcs.len() as FnIdx {
            let fi = &self.m.funcs[f as usize];
            if fi.module != 0
                || fi.func_id == u32::MAX
                || fi.ctor_of.is_some()
                || fi.params.is_empty()
                || fi.name.contains("::get ")
                || fi.name.contains("::set ")
                || unit_in.broken(&format!("fn:{}", fi.name))
            {
                continue;
            }
            let n = fi.params.len();
            if !unit_in.method_runs.contains(&fi.name) {
                self.m.method_described.insert(f, vec![true; n]);
                continue;
            }
            // DECISIONS 148: a method some unit runs: its parameters hold
            // what the running units pass (`Published::method_args`, their
            // join), beside the marker, as a closed function's do
            // (decision 13, `closed_params`). A position some call passes
            // an undescribed value stays the bare marker; a run that
            // publishes nothing describable (`top`) leaves them all.
            let Some(ac) = unit_in.method_args.get(&fi.name).filter(|a| !a.top) else {
                continue;
            };
            let name = fi.name.clone();
            let described: Vec<bool> = (0..n)
                .map(|i| !ac.params.get(i).is_some_and(|vc| vc.unknown))
                .collect();
            if !described.contains(&true) {
                continue;
            }
            let mut nodes = Vec::with_capacity(n);
            for i in 0..n {
                // A call passing fewer arguments passes undefined: its
                // contract's kinds carry it (DECISIONS 77).
                let node = match ac.params.get(i) {
                    Some(_) if !described[i] => NO_NODE,
                    Some(vc) => self.stand_in_value(0, &format!("{name}:arg{i}"), vc),
                    None => {
                        let node = self.m.node_named("arg", || format!("arg{i}({name})"));
                        self.m.add_prim(node, prim::UNDEF);
                        node
                    }
                };
                nodes.push(node);
            }
            self.m.method_described.insert(f, described);
            self.m.method_params.insert(f, nodes);
        }
    }

    /// This unit's own class named by display: a class has one defining
    /// unit (a literal shape may have several, DECISIONS 112).
    pub(super) fn own_class(&self, display: &str) -> Option<TmplId> {
        self.own_classes
            .get(display)
            .copied()
            .filter(|tm| !self.m.foreign.contains_key(tm))
    }

    pub(super) fn value_contract_for(
        &self,
        path: &str,
        name: &str,
    ) -> Option<&'a crate::units::ValueContract> {
        self.unit_in?.value_contract(path, name)
    }

    /// Another unit's module-scope value as this unit sees it through its
    /// value contract: instances of the stand-ins of the layouts it may hold
    /// (their fields hold unknown values), the stand-ins of its contract
    /// classes and functions, ⊤ when it may hold an unknown value. A broken
    /// layout or class is ⊤ (§11.3). Using it beyond the contract is this
    /// unit's census report, as for any stand-in.
    pub(super) fn stand_in_value(
        &mut self,
        mi: u32,
        local: &str,
        c: &'a crate::units::ValueContract,
    ) -> NodeId {
        let n = self.m.node_named("value", || format!("value({local})"));
        let broken = |g: &Self, d: &str| g.unit_in.is_some_and(|u| u.broken(d));
        for cc in &c.layouts {
            if broken(self, &cc.display) {
                self.m
                    .add_top(n, |_| "broken layout of another unit".into());
                continue;
            }
            // An instance of this unit's class described back to it (a
            // caller passes this unit's instances to its closed
            // constructor, DECISIONS 133): its own template, whose facts
            // this solve makes, as for the class value (DECISIONS 120).
            let tm = match self.own_class(&cc.display) {
                Some(tm) => tm,
                None => {
                    self.stand_in_class(mi, cc);
                    self.stand_ins[&cc.display].1
                }
            };
            let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
            self.m.add_tok(n, inst);
        }
        for cc in &c.classes {
            if broken(self, &cc.display) {
                self.m.add_top(n, |_| "broken class of another unit".into());
                continue;
            }
            let v = self.contract_class_value(mi, cc);
            self.m.edge(v, n);
        }
        for fc in &c.fns {
            if broken(self, &format!("fn:{}", fc.name)) {
                self.m
                    .add_top(n, |_| "broken function of another unit".into());
                continue;
            }
            let v = self.stand_in_fn(mi, fc);
            self.m.edge(v, n);
        }
        // The runtime-owned objects it may hold: one object for the whole
        // program, held here as in the defining unit.
        for s in &c.shared {
            let t = self.m.shared_tok(s);
            self.m.add_tok(n, t);
        }
        if c.unknown {
            self.m.add_top(n, |_| "value of another unit".into());
        }
        if c.prim != 0 {
            self.m.add_prim(n, c.prim);
        }
        for &ns in &c.builtins {
            let t = self.m.tok(TokKind::Builtin(ns));
            self.m.add_tok(n, t);
        }
        // DECISIONS 94: another unit's array.
        for d in &c.arrays {
            let t = self.stand_in_arr(mi, d, false);
            self.m.add_tok(n, t);
        }
        // DECISIONS 184: another unit's primitive method container.
        for d in &c.boxes {
            let t = self.stand_in_arr(mi, d, true);
            self.m.add_tok(n, t);
        }
        n
    }

    /// Another unit's unbroken layout named by display (a function
    /// result, a field). Read like a cell (DECISIONS 103): its publisher
    /// may solve after this unit, so the read is recorded and this unit
    /// is solved again when the layout changes.
    pub(super) fn other_layout(&mut self, d: &str) -> Option<&'a crate::units::ClassContract> {
        if !self.layouts_read.iter().any(|(x, _)| x == d) {
            let seen = self.unit_in.map_or_else(
                || crate::units::digest(&None::<&crate::units::ClassContract>),
                |u| u.layout_seen(d),
            );
            self.layouts_read.push((d.to_string(), seen));
        }
        self.unit_in
            .filter(|u| !u.broken(d))
            .and_then(|u| u.layout(d))
    }

    /// DECISIONS 126: another unit's typed arrays in field `key` of class
    /// `display`, as this unit sees them: one typed array of their element
    /// kind. When it escapes or takes a hard mark here, the census reports
    /// `ta <key> of <display>` and the owner sinks the field.
    pub(super) fn stand_in_ta(
        &mut self,
        mi: u32,
        display: &str,
        key: &str,
        elem: Option<u8>,
    ) -> Tok {
        let rep = format!("ta {key} of {display}");
        if let Some(&(_, t)) = self.stand_in_tas.iter().find(|(r, _)| *r == rep) {
            return t;
        }
        let (t, _) = self.bobj(mi, "TypedArray");
        if let (Some(k), Some(tm)) = (elem, self.m.tmpl_of(t)) {
            self.m.ta_elem.insert(tm, k);
        }
        self.stand_in_tas.push((rep, t));
        t
    }

    /// DECISIONS 94: another unit's array as this unit sees it: an array
    /// (its methods are the builtins) whose elements are cell
    /// `arr:<display>`, what every unit holding it writes there.
    pub(super) fn stand_in_arr(&mut self, mi: u32, display: &str, boxed: bool) -> Tok {
        if let Some(&t) = self.stand_in_arrs.get(display) {
            return t;
        }
        let tm = self.m.tmpls.len() as TmplId;
        self.m.tmpls.push(Template {
            kind: if boxed {
                TmplKind::BObj {
                    module: mi,
                    site: u32::MAX,
                    what: "primbox",
                }
            } else {
                TmplKind::Arr {
                    module: mi,
                    site: u32::MAX,
                }
            },
            display: display.to_string(),
            birth_keys: vec![],
            definite: vec![],
            parent: None,
            ctor: None,
            native_parent: None,
            accessor_keys: vec![],
            has_computed_members: false,
            field_init: None,
            proto_members: vec![],
            parent_value: None,
            parent_links: Vec::new(),
            null_proto: false,
            declared: vec![],
        });
        let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
        self.stand_in_arrs.insert(display.to_string(), t);
        self.arr_leaves(t);
        t
    }

    pub(super) fn contract_for(
        &self,
        path: &str,
        name: &str,
    ) -> Option<&'a crate::units::ClassContract> {
        self.unit_in?.class_contract(path, name)
    }

    /// A second description of a layout this unit already stands in for:
    /// literals of one shape that different units publish, or one class
    /// seen through two contracts. The reader relies only on what every
    /// description says (`ClassContract::join`): the facts join, a field
    /// both describe by their functions holds the functions of both, and a
    /// field, method or static the descriptions give differently holds what
    /// either may hold (⊤ unless both name functions). The stand-in only
    /// grows, so the order the contracts arrive in does not matter.
    pub(super) fn widen_stand_in(
        &mut self,
        mi: u32,
        tm: TmplId,
        c: &'a crate::units::ClassContract,
    ) {
        let old = self.stand_in_cc[&tm].clone();
        let mut joined = old.clone();
        joined.join(c);
        let r = self
            .m
            .reason("another unit's class used beyond its contract");
        let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
        let top = |g: &mut Self, n: NodeId| {
            g.m.add_top(n, |_| "field of another unit's class".into());
            g.m.add_prim(n, prim::ALL);
        };
        for fld in &joined.facts.fields {
            let a = self.m.atom(&fld.key);
            let n = self.m.field(inst, a);
            let fns = |cc: &crate::units::ClassContract| {
                cc.field_fns
                    .iter()
                    .find(|(k, _)| k == &fld.key)
                    .map(|(_, f)| f.clone())
            };
            let layouts = |cc: &crate::units::ClassContract| {
                cc.field_layouts
                    .iter()
                    .find(|(k, _, _)| k == &fld.key)
                    .cloned()
            };
            let kind = |cc: &crate::units::ClassContract| {
                cc.facts
                    .fields
                    .iter()
                    .find(|f| f.key == fld.key)
                    .map(|f| (f.kind.clone(), f.definite))
            };
            // The join keeps field functions only where every description
            // names them: the old ones are wired, the new ones are `c`'s.
            if fns(&joined).is_some() {
                if let Some((_, fcs)) = c.field_fns.iter().find(|(k, _)| k == &fld.key) {
                    for fc in fcs {
                        let v = self.stand_in_fn_of(mi, fc, Some((tm, false)));
                        self.m.edge(v, n);
                    }
                }
                continue;
            }
            if layouts(&joined).is_some() && layouts(&joined) == layouts(&old) {
                continue;
            }
            if layouts(&joined).is_none() && fns(&old).is_none() && kind(&joined) == kind(&old) {
                continue;
            }
            top(self, n);
        }
        if joined.open_keys && !old.open_keys {
            let n = self.m.field_any(inst);
            top(self, n);
        }
        let proto = self.m.tok(TokKind::Proto(tm));
        for (key, fname) in &joined.methods {
            if old.methods.iter().any(|(k, f)| k == key && f == fname) {
                continue;
            }
            let f = self.stub_fn(
                mi,
                fname,
                tm,
                true,
                false,
                c.closed.is_none() || c.forwards,
                r,
            );
            let a = self.m.atom(key);
            let n = self.m.field(proto, a);
            let t = self.m.funcs[f as usize].token;
            self.m.add_tok(n, t);
        }
        let cobj = self.m.tok(TokKind::ClassObj(tm));
        if joined.statics != old.statics || joined.static_fns != old.static_fns {
            for (key, _) in &joined.statics {
                let a = self.m.atom(key);
                let n = self.m.field(cobj, a);
                top(self, n);
            }
        }
        if joined.statics_open && !old.statics_open {
            let any = self.m.field_any(cobj);
            top(self, any);
        }
        self.m.foreign.insert(tm, joined.facts.clone());
        self.stand_in_cc.insert(tm, joined);
    }

    /// Another unit's class as this unit sees it through its contract
    /// (design §11.2): `new` gives an instance of its template, whose facts
    /// are the defining unit's; its methods and constructor are stand-ins
    /// that hand the arguments to that unit's code and return unknown
    /// values; its fields and statics hold unknown values. Using it beyond
    /// that is this unit's census report (`Gen::reports`).
    /// The class value contract `c` names here: this unit's own class
    /// when the contract names it back (a binding this unit wrote, read
    /// through its cell: DECISIONS 120), else its stand-in. The class is
    /// one object; its own unit has its template.
    pub(super) fn contract_class_value(
        &mut self,
        mi: u32,
        c: &'a crate::units::ClassContract,
    ) -> NodeId {
        if let Some(i) = self.own_class(&c.display) {
            let n = self
                .m
                .node_named("own-class", || format!("own-class({})", c.display));
            let t = self.m.tok(TokKind::ClassObj(i));
            self.m.add_tok(n, t);
            return n;
        }
        self.stand_in_class(mi, c)
    }

    pub(super) fn stand_in_class(&mut self, mi: u32, c: &'a crate::units::ClassContract) -> NodeId {
        // DECISIONS 106: the owner's layout over a forwarded copy.
        let c = self.other_layout(&c.display).unwrap_or(c);
        if let Some(&(n, tm, _)) = self.stand_ins.get(&c.display) {
            if self.stand_in_cc.get(&tm) != Some(c) {
                self.widen_stand_in(mi, tm, c);
            }
            return n;
        }
        let r = self
            .m
            .reason("another unit's class used beyond its contract");
        let tm = self.m.tmpls.len() as TmplId;
        // A closed class (design §18.13): its constructor's parameters
        // collect what this unit's constructions pass, published under the
        // class's key. Its body reads no `arguments` and has no rest
        // parameter (`lib::closed_of`), so an argument past them reaches
        // nothing; what a parameter holds goes to the defining unit's
        // constructor through the argument contract: another unit's
        // stand-in stays inside its contract there, anything else escapes
        // (`Cons::HandOver`, DECISIONS 109).
        let ctor = self.stub_fn_of(
            mi,
            &format!("{}.constructor", c.name),
            tm,
            false,
            false,
            c.closed.is_none() || c.forwards,
            r,
            c.closed.is_none(),
        );
        if let Some((path, name, n)) = &c.closed {
            let ct = self.m.funcs[ctor as usize].token;
            let outer_fn = std::mem::replace(&mut self.m.cur_fn, ctor);
            let params: Vec<NodeId> = (0..*n)
                .map(|i| {
                    let p = self
                        .m
                        .node_named("param", || format!("param{i}({}.constructor)", c.name));
                    self.m.own(p);
                    self.m.cons(
                        p,
                        Cons::HandOver {
                            reason: r,
                            ctor: ct,
                            arg: i,
                        },
                    );
                    p
                })
                .collect();
            self.m.cur_fn = outer_fn;
            self.m.funcs[ctor as usize].params = params;
            let t = self.m.funcs[ctor as usize].token;
            self.closed_ctors
                .push(((path.clone(), name.clone()), tm, t));
        }
        let birth_keys = c.birth_keys.iter().map(|k| self.m.atom(k)).collect();
        let definite = c.definite.iter().map(|k| self.m.atom(k)).collect();
        self.m.tmpls.push(Template {
            kind: TmplKind::Class {
                module: mi,
                name: c.name.clone(),
                class_id: c.class_id,
                literal_shape: c.literal_shape,
            },
            display: c.display.clone(),
            birth_keys,
            definite,
            declared: Vec::new(),
            parent: None,
            ctor: Some(ctor),
            native_parent: c.native_parent.clone(),
            accessor_keys: Vec::new(),
            has_computed_members: false,
            field_init: None,
            proto_members: Vec::new(),
            parent_value: None,
            parent_links: Vec::new(),
            null_proto: false,
        });
        self.m.funcs[ctor as usize].ctor_of = Some(tm);
        self.m.foreign.insert(tm, c.facts.clone());
        self.stand_in_cc.insert(tm, c.clone());
        // Its parent from another unit (DECISIONS 104): that class's own
        // stand-in, so an inherited method resolves up the chain as in the
        // defining unit. Unpublished, the chain ends here as before. A
        // parent of this unit is its own class (DECISIONS 181): a method the
        // subclass does not override is this unit's own code, run on the
        // subclass's object as in the whole program, not a replay of the
        // contract this unit published for it.
        if let Some(d) = &c.parent {
            if let Some(ptm) = self.own_class(d) {
                self.m.tmpls[tm as usize].parent = Some(ptm);
            } else if let Some(pc) = self.other_layout(d) {
                self.stand_in_class(mi, pc);
                let ptm = self.stand_ins[&pc.display].1;
                self.m.tmpls[tm as usize].parent = Some(ptm);
            }
        }
        let proto = self.m.tok(TokKind::Proto(tm));
        let cobj = self.m.tok(TokKind::ClassObj(tm));
        let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
        // A literal layout has no class object: its `constructor` is
        // Object's, found up the chain (decision 17: what an argument
        // reaches is not the stand-in's class).
        if !c.literal_shape {
            let pa = self.m.atom("prototype");
            let pf = self.m.field(cobj, pa);
            self.m.add_tok(pf, proto);
            let ca = self.m.atom("constructor");
            let cf = self.m.field(proto, ca);
            self.m.add_tok(cf, cobj);
        }
        let mut toks = vec![inst, cobj, proto, self.m.funcs[ctor as usize].token];
        // Registered before its members are built: a member function whose
        // result is this layout reaches it again (`stand_in_fn_of`).
        let n = self
            .m
            .node_named("classval", || format!("classval({})", c.name));
        self.m.add_tok(n, cobj);
        self.stand_ins
            .insert(c.display.clone(), (n, tm, Vec::new()));
        // Its static members: a static method with a function contract is
        // its stand-in (made below, its result may be this class), another
        // one a stub running on the class object, any other static an
        // unknown value; other keys only when the defining unit's class
        // object is open.
        let mut static_fns: Vec<(NodeId, &'a crate::units::FnContract)> = Vec::new();
        let mut static_values: Vec<(NodeId, &'a (String, crate::units::ValueContract))> =
            Vec::new();
        for (key, m) in &c.statics {
            let a = self.m.atom(key);
            let n = self.m.field(cobj, a);
            if let Some((_, fc)) = c.static_fns.iter().find(|(k, _)| k == key) {
                static_fns.push((n, fc));
                continue;
            }
            // A data static with a value contract (DECISIONS 173): what it
            // describes, made below.
            if let Some(sv) = c.static_values.iter().find(|(k, _)| k == key) {
                static_values.push((n, sv));
                continue;
            }
            match m {
                Some(fname) => {
                    let f = self.stub_fn(
                        mi,
                        fname,
                        tm,
                        true,
                        true,
                        c.closed.is_none() || c.forwards,
                        r,
                    );
                    let t = self.m.funcs[f as usize].token;
                    self.m.add_tok(n, t);
                    toks.push(t);
                }
                None => {
                    self.m
                        .add_top(n, |_| "static member of another unit's class".into());
                    self.m.add_prim(n, prim::ALL);
                }
            }
        }
        if c.statics_open {
            let any = self.m.field_any(cobj);
            self.m
                .add_top(any, |_| "static member of another unit's class".into());
            self.m.add_prim(any, prim::ALL);
        }
        let mut method_fns: Vec<(NodeId, &'a crate::units::FnContract)> = Vec::new();
        for (key, fname) in &c.methods {
            if let Some((_, fc)) = c.method_fns.iter().find(|(k, _)| k == key) {
                let a = self.m.atom(key);
                method_fns.push((self.m.field(proto, a), fc));
                continue;
            }
            let f = self.stub_fn(
                mi,
                fname,
                tm,
                true,
                false,
                c.closed.is_none() || c.forwards,
                r,
            );
            // DECISIONS 165: a call of it runs that method.
            self.m.stub_methods.insert(f);
            let a = self.m.atom(key);
            let n = self.m.field(proto, a);
            let t = self.m.funcs[f as usize].token;
            self.m.add_tok(n, t);
            toks.push(t);
        }
        let mut nested: Vec<(NodeId, &'a (String, Vec<String>, bool))> = Vec::new();
        let mut value_fields: Vec<(NodeId, &'a (String, crate::units::ValueContract))> = Vec::new();
        let mut fn_fields: Vec<(NodeId, &'a Vec<crate::units::FnContract>)> = Vec::new();
        let mut class_fields: Vec<(NodeId, &'a crate::derive::KindFact, bool)> = Vec::new();
        for fld in &c.facts.fields {
            let a = self.m.atom(&fld.key);
            let n = self.m.field(inst, a);
            // DECISIONS 126: a field holding only the defining unit's typed
            // arrays: one stand-in typed array per class and key, whose uses
            // here the census reports (`Gen::reports`).
            if let Some((_, elem, p)) = c.field_tas.iter().find(|(k, _, _)| k == &fld.key) {
                let t = self.stand_in_ta(mi, &c.display, &fld.key, *elem);
                self.m.add_tok(n, t);
                self.m.add_prim(n, *p);
                continue;
            }
            // A field holding only the defining unit's literal layouts:
            // instances of their stand-ins (§18.7), made below.
            if let Some(fl) = c.field_layouts.iter().find(|(k, _, _)| k == &fld.key) {
                nested.push((n, fl));
                continue;
            }
            // A field forwarding other units' stand-ins: what its value
            // contract describes (DECISIONS 88), made below.
            if let Some(fv) = c.field_values.iter().find(|(k, _)| k == &fld.key) {
                value_fields.push((n, fv));
                continue;
            }
            // A field holding only contract functions: their stand-ins,
            // made below.
            if let Some((_, fcs)) = c.field_fns.iter().find(|(k, _)| k == &fld.key) {
                fn_fields.push((n, fcs));
                continue;
            }
            // A data field that holds only primitives (DECISIONS 74): their
            // kinds, as the defining unit's solve found them with every
            // unit's store effects (DECISIONS 73) performed; a read before
            // the field exists is undefined.
            let k = &fld.kind;
            if fld.data_field
                && !k.top
                && k.objs.is_empty()
                && k.funcs == 0
                && k.builtin_objs == 0
                && k.prims != 0
            {
                let undef = if fld.definite { 0 } else { prim::UNDEF };
                self.m.add_prim(n, k.prims | undef);
                continue;
            }
            // A data field that holds only instances of classes (and
            // primitives), as the defining unit's solve found them
            // (DECISIONS 110): made below.
            if fld.data_field && !k.top && !k.objs.is_empty() && k.funcs == 0 && k.builtin_objs == 0
            {
                class_fields.push((n, k, fld.definite));
                continue;
            }
            self.m
                .add_top(n, |_| "field of another unit's class".into());
            self.m.add_prim(n, prim::ALL);
        }
        if c.open_keys {
            let n = self.m.field_any(inst);
            self.m
                .add_top(n, |_| "field of another unit's class".into());
            self.m.add_prim(n, prim::ALL);
        }
        if let Some(e) = self.stand_ins.get_mut(&c.display) {
            e.2.extend(toks);
        }
        // After the layout's stand-in exists: a field function whose result
        // is this layout again (a literal returning itself) finds it
        // (DECISIONS 78).
        for (fnode, fcs) in fn_fields {
            for fc in fcs {
                let v = self.stand_in_fn_of(mi, fc, Some((tm, false)));
                self.m.edge(v, fnode);
            }
        }
        // DECISIONS 110 (73 for objects): a field whose published kind names
        // only this class (a tree's or a list's links) reads as this
        // stand-in's instance. The kind is the defining unit's at the
        // fixpoint, every unit's stores performed (a store of an object is
        // `None`: ⊤ there), so the field holds exactly such instances; using
        // one beyond the contract is this unit's census report. It needs no
        // other contract than this one. Another class it names reads ⊤ as
        // before: its contract would be a read by display, which the driver
        // does not track across rounds (CW2 DECISIONS 103).
        for (fnode, k, definite) in class_fields {
            if c.literal_shape || k.objs.iter().any(|d| *d != c.display) {
                self.m
                    .add_top(fnode, |_| "field of another unit's class".into());
                self.m.add_prim(fnode, prim::ALL);
                continue;
            }
            self.m.add_tok(fnode, inst);
            let undef = if definite { 0 } else { prim::UNDEF };
            self.m.add_prim(fnode, k.prims | undef);
        }
        for (fnode, (_, ds, prims)) in nested {
            for d in ds {
                // A broken layout is ⊤ to every reader (§11.3).
                match self.other_layout(d) {
                    Some(ic) => {
                        self.stand_in_class(mi, ic);
                        let itm = self.stand_ins[&ic.display].1;
                        let it = self.m.tok(TokKind::Obj(itm, NO_CTX));
                        self.m.add_tok(fnode, it);
                    }
                    None => self
                        .m
                        .add_top(fnode, |_| "field of another unit's class".into()),
                }
            }
            if *prims {
                self.m.add_prim(fnode, prim::ALL);
            }
        }
        for (fnode, (key, vc)) in value_fields.into_iter().chain(static_values) {
            let v = self.stand_in_value(mi, key, vc);
            self.m.edge(v, fnode);
        }
        for (field, fc) in static_fns {
            let v = self.stand_in_fn_of(mi, fc, Some((tm, true)));
            self.m.edge(v, field);
            let t = self.stand_in_fns[&(fc.name.clone(), Some((tm, true)))].1;
            if let Some(e) = self.stand_ins.get_mut(&c.display) {
                e.2.push(t);
            }
        }
        // Methods and the constructor with a function contract: their
        // stand-ins pass arguments by their marks and admit a subclass
        // instance as receiver (DECISIONS 57).
        for (field, fc) in method_fns {
            let v = self.stand_in_fn_of(mi, fc, Some((tm, false)));
            self.m.edge(v, field);
            let t = self.stand_in_fns[&(fc.name.clone(), Some((tm, false)))].1;
            if let Some(e) = self.stand_ins.get_mut(&c.display) {
                e.2.push(t);
            }
        }
        if let Some(fc) = &c.ctor_fn {
            self.stand_in_fn_of(mi, fc, Some((tm, false)));
            let t = self.stand_in_fns[&(fc.name.clone(), Some((tm, false)))].1;
            let f = self.m.tok_fn(t);
            self.m.funcs[f as usize].ctor_of = Some(tm);
            self.m.funcs[ctor as usize].ctor_of = None;
            self.m.tmpls[tm as usize].ctor = Some(f);
            // DECISIONS 198: what a `new` (or a subclass's `super`) passes
            // the constructor still reaches the stub's parameters: a closed
            // class's constructions publish them (`closed_ctors`) and hand
            // over the markers among them (DECISIONS 152).
            let (fp, sp) = (
                self.m.funcs[f as usize].params.clone(),
                self.m.funcs[ctor as usize].params.clone(),
            );
            for (&a, &b) in fp.iter().zip(sp.iter()) {
                if a != NO_NODE && b != NO_NODE {
                    self.m.edge(a, b);
                }
            }
            let stub = self.m.funcs[ctor as usize].token;
            if let Some(e) = self.stand_ins.get_mut(&c.display) {
                e.2.retain(|&x| x != stub);
                e.2.push(t);
            }
        }
        n
    }

    /// A function of another unit seen through a contract: its arguments go
    /// to that unit's code, `this` admits only `class`'s instances (any other
    /// receiver escapes), the result is unknown (`ret_top`).
    pub(super) fn stub_fn(
        &mut self,
        mi: u32,
        name: &str,
        class: TmplId,
        ret_top: bool,
        statics: bool,
        forwarded: bool,
        reason: u32,
    ) -> FnIdx {
        self.stub_fn_of(mi, name, class, ret_top, statics, forwarded, reason, true)
    }

    /// `stub_fn`; `sink_args` false for a closed class's constructor, whose
    /// parameters hand their values over instead (`stand_in_class`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn stub_fn_of(
        &mut self,
        mi: u32,
        name: &str,
        class: TmplId,
        ret_top: bool,
        statics: bool,
        forwarded: bool,
        reason: u32,
        sink_args: bool,
    ) -> FnIdx {
        let fx = self.m.funcs.len() as FnIdx;
        let tok = self.m.tok(TokKind::Func(fx, NO_CTX));
        let outer_fn = std::mem::replace(&mut self.m.cur_fn, fx);
        let this = self.m.node_named("this", || format!("this({name})"));
        let ret = self.m.node_named("ret", || format!("ret({name})"));
        let args_all = self.m.node_named("args", || format!("args({name})"));
        self.m.own(ret);
        self.m.own(this);
        self.m.cur_fn = outer_fn;
        self.m.funcs.push(FuncInfo {
            module: mi,
            func_id: u32::MAX,
            name: name.to_string(),
            params: Vec::new(),
            rest: None,
            this,
            ret,
            args_all,
            has_arguments: true,
            token: tok,
            ctor_of: None,
            lexical_this: false,
            no_summary: true,
            gen_step: false,
            captures: Vec::new(),
            forwards_super: false,
            parent: NO_FN,
            forwarded,
        });
        if sink_args {
            self.m.sink_node(args_all, reason);
        }
        if ret_top {
            self.m
                .add_top(ret, |_| "result of another unit's method".into());
            self.m.add_prim(ret, prim::ALL);
        }
        self.m.cons(
            this,
            Cons::ContractThis {
                tmpl: class,
                reason,
                statics,
                this: u32::MAX,
            },
        );
        fx
    }
}
