use super::*;

impl Model {
    /// Record a write under `key` onto `t` that may reach
    /// `%Function.prototype%`'s `bind`, `name` or `length`
    /// ([`Model::fnproto_write`]). `key` `None`: an operation whose key the
    /// model does not keep (a definition, a deletion).
    pub fn note_fnproto_write(&mut self, t: Tok, key: Option<Key>, what: &str) {
        if self.fnproto_write.is_some()
            || !(t == TOP || matches!(self.toks[t as usize], TokKind::ObjectProto))
        {
            return;
        }
        let hits = match key {
            None | Some(Key::Any) => true,
            Some(Key::Named(a)) => matches!(
                self.atoms[a as usize].as_str(),
                "bind" | "name" | "length" | crate::STAR_KEY
            ),
        };
        if hits {
            let on = if t == TOP {
                "a value from unmodeled code"
            } else {
                "a builtin prototype"
            };
            self.fnproto_write = Some(self.reason(&format!("{what} on {on}")));
        }
    }

    /// The element key: an element read of an array-like (iteration, array
    /// callbacks) sees its elements, not the builtin methods a computed key
    /// could also name; on anything else it is a computed read (`Key::Any`).
    pub fn elem_key(&mut self) -> Key {
        Key::Named(self.atom(ELEM))
    }

    pub fn is_elem_key(&self, k: Key) -> bool {
        matches!(k, Key::Named(a) if self.atoms[a as usize] == ELEM)
    }

    /// The iteration key: what iterating a value yields WITHOUT its
    /// protocol (`Gen::iterate` models `[Symbol.iterator]().next().value`
    /// beside it): an array-like's elements; a program object or function
    /// yields nothing that way; a runtime object is a computed read.
    pub fn iter_key(&mut self) -> Key {
        Key::Named(self.atom(ITER_ELEM))
    }

    pub fn is_iter_key(&self, k: Key) -> bool {
        matches!(k, Key::Named(a) if self.atoms[a as usize] == ITER_ELEM)
    }

    pub fn atom(&mut self, s: &str) -> Atom {
        if let Some(&a) = self.atom_index.get(s) {
            return a;
        }
        let a = self.atoms.len() as Atom;
        self.atoms.push(s.to_string());
        self.atom_index.insert(s.to_string(), a);
        a
    }

    /// Record one relation line, tagged with the current phase.
    #[inline]
    pub fn rel(&mut self, line: impl FnOnce() -> String) {
        if let Some(r) = self.relations.as_mut() {
            let mut l = String::with_capacity(48);
            l.push(self.rel_phase as char);
            l.push(' ');
            l.push_str(&line());
            r.push(l);
        }
    }

    pub fn reason(&mut self, s: &str) -> u32 {
        if let Some(&r) = self.reason_index.get(s) {
            return r;
        }
        let r = self.reasons.len() as u32;
        self.reasons.push(s.to_string());
        self.reason_index.insert(s.to_string(), r);
        r
    }

    pub fn constructor_origin(&self, mut class: TmplId) -> TmplId {
        for _ in 0..self.constructor_origins.len() {
            let Some(&origin) = self.constructor_origins.get(&class) else {
                break;
            };
            class = origin;
        }
        class
    }

    pub fn tok(&mut self, k: TokKind) -> Tok {
        if let Some(&t) = self.tok_index.get(&k) {
            return t;
        }
        let t = self.toks.len() as Tok;
        if let TokKind::Obj(tm, ctx) = k {
            if ctx != NO_CTX {
                self.site_toks.entry(tm).or_default().push(t);
            }
        }
        self.toks.push(k.clone());
        self.tok_index.insert(k, t);
        t
    }

    /// The template token of `tm` and every birth-site token of it.
    pub fn tmpl_toks(&self, tm: TmplId) -> Vec<Tok> {
        let mut v: Vec<Tok> = self
            .tok_index
            .get(&TokKind::Obj(tm, NO_CTX))
            .copied()
            .into_iter()
            .collect();
        if let Some(s) = self.site_toks.get(&tm) {
            v.extend_from_slice(s);
        }
        v
    }

    pub fn node(&mut self, name: impl FnOnce() -> String) -> NodeId {
        self.node_named("other", name)
    }

    /// A node with a static label (diagnostics: the biggest points-to sets
    /// are reported by label).
    pub fn node_named(&mut self, label: &'static str, name: impl FnOnce() -> String) -> NodeId {
        let id = self.nodes.len() as NodeId;
        if self.max_nodes != 0 && self.nodes.len() > self.max_nodes {
            // Work bound: the program gets no facts (checked by the solver).
            self.aborted = true;
        }
        self.nodes.push(Node::default());
        self.node_fn.push(NO_FN);
        self.labels.push(label);
        if self.relations.is_some() {
            self.node_names.push(name());
        }
        id
    }

    /// `n` belongs to the body generation is walking.
    pub fn own(&mut self, n: NodeId) {
        self.node_fn[n as usize] = self.cur_fn;
    }

    pub fn node_l(&mut self, label: &'static str) -> NodeId {
        self.node_named(label, || label.to_string())
    }

    /// A key as one whitespace-free relation token.
    pub fn key_name(&self, k: Key) -> String {
        match k {
            Key::Named(a) => esc_key(&self.atoms[a as usize]),
            Key::Any => "*".to_string(),
        }
    }

    /// The function of a function token (`NO_FN` for any other token).
    pub fn tok_fn(&self, t: Tok) -> FnIdx {
        match self.toks[t as usize] {
            TokKind::Func(f, _) => f,
            _ => NO_FN,
        }
    }

    pub fn tok_name(&self, t: Tok) -> String {
        match &self.toks[t as usize] {
            TokKind::Top => "TOP".into(),
            TokKind::Obj(tm, _) => format!("obj:{}", self.tmpls[*tm as usize].display),
            TokKind::Func(f, _) => format!("fn:{}", self.funcs[*f as usize].name),
            TokKind::Bound(b) => format!("bound:{b}"),
            TokKind::ClassObj(tm) => format!("class:{}", self.tmpls[*tm as usize].display),
            TokKind::Proto(tm) => format!("proto:{}", self.tmpls[*tm as usize].display),
            TokKind::ObjectProto => "Object.prototype".into(),
            TokKind::Builtin(b) => format!("builtin:{b:?}"),
            TokKind::BuiltinFn => "builtin-fn".into(),
            TokKind::ModNs(m) => format!("ns:{m}"),
            TokKind::NativeProto(n) => format!("native:{n}"),
        }
    }

    /// What this solve can observe of the shared objects ([`SharedReads`]).
    pub fn shared_reads(&self) -> SharedReads {
        let mut r = SharedReads::default();
        // An object's writes reach this solve through its field nodes, by
        // key (its any-key field: every key); a builtin function value has
        // none: through its property reads, any of which a write of any
        // builtin function value turns opaque (`write_builtin_fns`).
        for (k, &t) in &self.tok_index {
            let read = match k {
                TokKind::ObjectProto | TokKind::Builtin(_) | TokKind::NativeProto(_) => {
                    if self.field_any.contains_key(&t) {
                        Some(None)
                    } else {
                        self.field_keys.get(&t).map(|ks| {
                            Some(ks.iter().map(|&a| self.atoms[a as usize].clone()).collect())
                        })
                    }
                }
                TokKind::BuiltinFn => (!self.bf_reads.is_empty()).then_some(None),
                _ => None,
            };
            if let (Some(s), Some(keys)) = (self.shared_of(t), read) {
                r.objects.insert(s, keys);
            }
        }
        for &(_, a, _) in &self.native_missing {
            r.keys.insert(self.atoms[a as usize].clone());
        }
        r
    }

    pub fn anon_post_site(&mut self, desc: &str) -> SiteId {
        let id = self.stores.len() as SiteId;
        self.stores.push(StoreSite {
            module: u32::MAX,
            func: u32::MAX,
            desc: desc.to_string(),
            ctor_phase_of: None,
            birth: false,
            ctor_fn: None,
            rooted: false,
            deep: false,
            key_value: None,
        });
        id
    }

    pub fn anon_birth_site(&mut self, desc: &str) -> SiteId {
        let id = self.stores.len() as SiteId;
        self.stores.push(StoreSite {
            module: u32::MAX,
            func: u32::MAX,
            desc: desc.to_string(),
            ctor_phase_of: None,
            birth: true,
            ctor_fn: None,
            rooted: false,
            deep: false,
            key_value: None,
        });
        id
    }

    /// Metadata for the independent checker: every token, template,
    /// function, call site, store site and reason (tag `M`).
    pub fn dump_meta(&mut self) {
        if self.relations.is_none() {
            return;
        }
        let mut out: Vec<String> = Vec::new();
        for (i, a) in self.atoms.iter().enumerate() {
            out.push(format!("M atom {i} {}", esc_key(a)));
        }
        for (i, r) in self.reasons.iter().enumerate() {
            out.push(format!("M reason {i} {}", r.replace(['\n', '\r'], " ")));
        }
        for (i, k) in self.toks.iter().enumerate() {
            let d = match k {
                TokKind::Top => "top -".to_string(),
                TokKind::Obj(t, NO_CTX) => format!("obj {t}"),
                TokKind::Obj(t, c) => format!("obj {t}@{c}"),
                TokKind::Func(f, NO_CTX) => format!("func {f}"),
                TokKind::Func(f, c) => format!("func {f}@{c}"),
                TokKind::Bound(b) => format!("bound {b}"),
                TokKind::ClassObj(t) => format!("classobj {t}"),
                TokKind::Proto(t) => format!("proto {t}"),
                TokKind::ObjectProto => "objectproto -".into(),
                TokKind::Builtin(b) => format!("builtin {b:?}"),
                TokKind::BuiltinFn => "builtinfn -".into(),
                TokKind::ModNs(m) => format!("modns {m}"),
                TokKind::NativeProto(n) => format!("nativeproto {n}"),
            };
            out.push(format!("M tok {i} {d}"));
        }
        let join = |v: &[u32]| {
            if v.is_empty() {
                "-".to_string()
            } else {
                v.iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            }
        };
        for (i, t) in self.tmpls.iter().enumerate() {
            let kind = match &t.kind {
                TmplKind::Class { class_id, .. } => format!("class {class_id}"),
                TmplKind::Lit { .. } => "lit -".into(),
                TmplKind::FnInst { func } => format!("fninst {func}"),
                TmplKind::ObjCreate { .. } => "objcreate -".into(),
                TmplKind::Arr { .. } => "arr -".into(),
                TmplKind::BObj { what, .. } => format!("bobj {what}"),
            };
            out.push(format!(
                "M tmpl {i} {kind} birth={} definite={} parent={} ctor={} native={} acc={} computed={} fieldinit={} pm={} display={}",
                join(&t.birth_keys),
                join(&t.definite),
                t.parent.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                t.ctor.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                t.native_parent.as_ref().map(|(_, n)| n.clone()).unwrap_or_else(|| "-".into()),
                join(&t.accessor_keys),
                t.has_computed_members as u8,
                t.field_init.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                join(&t.proto_members),
                t.display.replace(' ', "\\s"),
            ));
        }
        for (i, f) in self.funcs.iter().enumerate() {
            out.push(format!(
                "M func {i} tok={} this={} ret={} args={} arguments={} rest={} lexical={} ctor_of={} params={}",
                f.token,
                f.this,
                f.ret,
                f.args_all,
                f.has_arguments as u8,
                f.rest.map(|r| r.to_string()).unwrap_or_else(|| "-".into()),
                f.lexical_this as u8,
                f.ctor_of
                    .map(|r| r.to_string())
                    .unwrap_or_else(|| "-".into()),
                join(&f.params),
            ));
        }
        for (f, (b, d)) in &self.fn_birth {
            out.push(format!(
                "M fnbirth {f} birth={} definite={}",
                join(b),
                join(d)
            ));
        }
        for (i, c) in self.calls.iter().enumerate() {
            let this = match c.this {
                ThisArg::Undef => "u".to_string(),
                ThisArg::Node(n) => format!("n{n}"),
                ThisArg::Tok(t) => format!("t{t}"),
            };
            out.push(format!(
                "M call {i} this={this} ret={} args={} spread={}",
                c.ret,
                join(&c.args),
                join(&c.spread)
            ));
        }
        for (i, s) in self.stores.iter().enumerate() {
            out.push(format!(
                "M store {i} birth={} ctor_phase_of={} ctor_fn={}",
                s.birth as u8,
                s.ctor_phase_of
                    .map(|r| r.to_string())
                    .unwrap_or_else(|| "-".into()),
                s.ctor_fn
                    .map(|r| r.to_string())
                    .unwrap_or_else(|| "-".into()),
            ));
        }
        for (i, (t, th, a)) in self.bound.iter().enumerate() {
            out.push(format!("M bound {i} target={t} this={th} args={}", join(a)));
        }
        out.push(format!(
            "M special thrown={} toptok={}",
            self.thrown, self.top_tok_node
        ));
        self.relations.as_mut().unwrap().extend(out);
    }

    /// The accessors installed by generation (class getters/setters …),
    /// logged before solving as checker inputs.
    pub fn log_seed_accessors(&mut self) {
        if self.relations.is_none() {
            return;
        }
        let mut v: Vec<String> = Vec::new();
        for ((h, k), (g, s)) in &self.accessors {
            for &t in g {
                v.push(format!("G accseed {h} {k} {} 0", self.tok_fn(t)));
            }
            for &t in s {
                v.push(format!("G accseed {h} {k} {} 1", self.tok_fn(t)));
            }
        }
        v.sort();
        self.relations.as_mut().unwrap().extend(v);
    }

    /// What parameter `i` of `f` holds when code outside its unit calls
    /// it: a hidden capture parameter holds its class environment slot
    /// (`FuncInfo::captures`); `None` for an argument the caller chooses.
    pub fn capture_param(&self, f: FnIdx, i: usize) -> Option<NodeId> {
        let fi = &self.funcs[f as usize];
        let first = fi.params.len().checked_sub(fi.captures.len())?;
        i.checked_sub(first).map(|k| fi.captures[k])
    }

    /// Diagnostics: follow the first-arrival chain of ⊤ back to its origin.
    pub fn top_origin(&self, start: NodeId) -> String {
        let mut n = start;
        let mut seen: crate::pts::FxHashSet<NodeId> = Default::default();
        let mut hops = 0usize;
        loop {
            if !seen.insert(n) {
                return format!("(cycle after {hops} hops at {})", self.labels[n as usize]);
            }
            let Some(&(src, why)) = self.top_pred.get(&n) else {
                return format!("(no record at {})", self.labels[n as usize]);
            };
            if why != u32::MAX {
                return self.reasons[why as usize].replace(['\n', '\r'], " ");
            }
            if src == NO_NODE {
                return format!("(seed at {})", self.labels[n as usize]);
            }
            if !self.top_pred.contains_key(&src) {
                return format!("(rule on {} without TOP)", self.labels[src as usize]);
            }
            n = src;
            hops += 1;
        }
    }

    /// DECISIONS 225: a function's identity across the two analyses of a
    /// program (its module, HIR id, and for a synthetic context its name).
    pub fn act_key(&self, f: FnIdx) -> (u32, u32, String) {
        let fi = &self.funcs[f as usize];
        let name = if fi.func_id == u32::MAX {
            fi.name.clone()
        } else {
            String::new()
        };
        (fi.module, fi.func_id, name)
    }

    /// DECISIONS 225: `f` can run: a transfer of an active body solved to
    /// it, or it escaped.
    pub fn activate(&mut self, f: FnIdx, path: &str) {
        if self
            .act
            .as_ref()
            .is_none_or(|a| a.fixed || a.sabotage.as_deref() == Some(path))
            || f as usize >= self.funcs.len()
        {
            return;
        }
        let k = self.act_key(f);
        let a = self.act.as_mut().unwrap();
        if a.on.insert(k) {
            a.queue.push(f);
        }
    }

    /// DECISIONS 225: generation walks the body of `f` (noted once the
    /// activation set is final).
    pub fn note_walked(&mut self, f: FnIdx) {
        if self.act.as_ref().is_some_and(|a| a.fixed) {
            let k = self.act_key(f);
            self.act.as_mut().unwrap().walked.insert(k);
        }
    }

    /// DECISIONS 225: whether generation walks the body of `f` now.
    pub fn body_active(&self, f: FnIdx) -> bool {
        match &self.act {
            None => true,
            Some(a) => a.on.contains(&self.act_key(f)),
        }
    }

    #[inline]
    pub fn is_escaped(&self, t: Tok) -> bool {
        self.esc_bits
            .get((t >> 6) as usize)
            .is_some_and(|w| w & (1 << (t & 63)) != 0)
    }

    /// DECISIONS 94/184: an array, or a primitive method's container of
    /// primitives (`primbox`, array-like: split/match results): its
    /// elements leave through cell `arr:<name>` (`Gen::arr_leaves`).
    pub fn is_cell_arr(&self, t: Tok) -> bool {
        match self.toks[t as usize] {
            TokKind::Obj(tm, _) => matches!(
                self.tmpls[tm as usize].kind,
                TmplKind::Arr { .. }
                    | TmplKind::BObj {
                        what: "primbox",
                        ..
                    }
            ),
            _ => false,
        }
    }

    /// DECISIONS 184: a primitive method's container (`is_cell_arr`).
    pub fn is_box(&self, t: Tok) -> bool {
        matches!(self.toks[t as usize], TokKind::Obj(tm, _)
            if matches!(self.tmpls[tm as usize].kind, TmplKind::BObj { what: "primbox", .. }))
    }

    pub fn tmpl_of(&self, t: Tok) -> Option<TmplId> {
        match self.toks[t as usize] {
            TokKind::Obj(tm, _) => Some(tm),
            _ => None,
        }
    }
}

impl Model {
    /// Record what the callee did to a parameter marker.
    #[inline]
    pub(crate) fn mark_use(&mut self, t: Tok, bits: u8) {
        if self.param_marker.is_empty() {
            return;
        }
        // An argument marker's use is a use of each marker it stands for.
        if self.is_arg_marker(t) {
            let mut st = vec![(t, bits)];
            while let Some((x, b)) = st.pop() {
                // Each marker the walk visits is solve work (113-CWL3).
                self.work += 1;
                if !self.is_arg_marker(x) {
                    self.mark_use(x, b);
                    continue;
                }
                // A use of what argument marker `p` reaches under one key is
                // recorded on that key's reach marker `x`, the token the use
                // names: a repeated use costs this step alone.
                if let Some(&(p, _)) = self.keyed_parent.get(&x) {
                    let u = self.marker_use.entry(x).or_default();
                    let new = b & !*u;
                    *u |= b;
                    if new != 0 {
                        self.keyed_use(p, new);
                    }
                    continue;
                }
                let u = self.marker_use.entry(x).or_default();
                let new = b & !*u;
                *u |= b;
                if new != 0 {
                    st.extend(self.arg_members[&x].iter().map(|&m| (m, new)));
                }
            }
            return;
        }
        let Some(&(f, i)) = self.param_marker.get(&t) else {
            return;
        };
        // DECISIONS 158: a step's use is a use of its keyed reach marker:
        // the contract publishes uses per key, and the reader applies them
        // to all its argument reaches under that key.
        if self
            .path_parent
            .get(&t)
            .is_some_and(|&(p, _)| self.path_parent.contains_key(&p))
        {
            let r = self.path_root(t);
            self.mark_use(r, bits);
        }
        let u = self.marker_use.entry(t).or_default();
        let new = bits & !*u;
        *u |= bits;
        // An escaped argument is sunk by the contract's consumer, which
        // then reads none of its other uses, results or reached values
        // (gen.rs `stand_in_fn_of`): the marker, and for the argument
        // itself its reach marker, read as ⊤ from now on (DECISIONS 42).
        // A closed function's described parameter keeps its uses: its
        // contract drops the escape (decision 17), and its callers' values
        // are beside the marker, not ⊤.
        let described = self
            .closed_described
            .get(&f)
            .and_then(|d| d.get(i as usize))
            .is_some_and(|&d| d);
        if new & MARK_ESCAPE != 0 {
            self.virt_revoke(t, None);
        }
        if new & MARK_ESCAPE != 0 && !described {
            self.set_marker_top(t);
            if let Some(&r) = self.marker_reach.get(&t) {
                self.set_marker_top(r);
            }
            // What it reaches under a key is what unknown code stored
            // there: no ⊤ beside the marker says so (DECISIONS 119). So is
            // each step below it (DECISIONS 158).
            let mut stack = vec![t];
            while let Some(k) = stack.pop() {
                let rs: Vec<Tok> = self
                    .keyed_reach
                    .get(&k)
                    .into_iter()
                    .flatten()
                    .map(|&(_, r)| r)
                    .chain(self.path_steps.get(&k).into_iter().flatten().copied())
                    .filter(|&r| !self.marker_is_top(r))
                    .collect();
                for r in rs {
                    self.set_marker_top(r);
                    stack.push(r);
                }
            }
        }
    }

    /// Parameter marker `t`'s reach marker for named key `a` (design
    /// §18.4): the value of `p.a`, and everything read from it. Its uses
    /// are published per key, so the caller applies them to its argument's
    /// `a` alone, not to everything the argument reaches (its prototype,
    /// its class).
    pub(crate) fn keyed_reach_of(&mut self, t: Tok, a: Atom) -> Tok {
        // DECISIONS 158: a path marker's keyed reach is its step.
        if self.is_path_marker(t) {
            return self.path_step_of(t, Some(a));
        }
        if let Some(&(_, r)) = self
            .keyed_reach
            .get(&t)
            .and_then(|v| v.iter().find(|(k, _)| *k == a))
        {
            return r;
        }
        let TokKind::Obj(tm, _) = self.toks[t as usize] else {
            unreachable!("a marker is an object token")
        };
        let display = format!(
            "{}:reach.{}",
            self.tmpls[tm as usize].display, self.atoms[a as usize]
        );
        let rm = self.marker_template(display);
        let r = self.tok(TokKind::Obj(rm, NO_CTX));
        // What an argument marker reaches is of its family (its field
        // nodes are of the same run).
        let home = self.fam_home(t);
        if home != 0 {
            self.set_fam(r, t, home);
        }
        // A parameter's own keyed reach is a path marker (DECISIONS 158):
        // what is read from it is a step of its own. Any other keyed reach
        // reaches itself under every key.
        let path = self.param_marker.contains_key(&t) && !self.reach_marker.contains(&t);
        if path {
            self.path_parent.insert(r, (t, Some(a)));
        }
        self.reach_marker.insert(r);
        self.keyed_reach.entry(t).or_default().push((a, r));
        // An escaped marker's keyed reach reads as ⊤ too (DECISIONS 119).
        if self.marker_is_top(t) {
            self.set_marker_top(r);
        }
        match self.param_marker.get(&t).copied() {
            Some(fi) => {
                self.param_marker.insert(r, fi);
            }
            // An argument marker's keyed reach: its uses are its members'
            // uses under the same key (DECISIONS 44), recorded on this token
            // and settled onto the parameter markers after the solve
            // (`settle_keyed_uses`), not as a token per member.
            None => {
                let w = (r >> 6) as usize;
                if self.arg_bits.len() <= w {
                    self.arg_bits.resize(w + 1, 0);
                }
                self.arg_bits[w] |= 1 << (r & 63);
                self.arg_members.insert(r, Vec::new());
                self.keyed_parent.insert(r, (t, a));
                if self.beside_markers.contains(&t) {
                    self.beside_markers.insert(r);
                }
            }
        }
        // A marker before its field node: that node is of the marker.
        if !path {
            let rany = self.field_any(r);
            self.add_tok(rany, r);
        }
        r
    }

    /// DECISIONS 158: `t` is a parameter's keyed reach marker or a step
    /// below it.
    #[inline]
    pub(crate) fn is_path_marker(&self, t: Tok) -> bool {
        self.path_parent.contains_key(&t)
    }

    /// The keyed reach marker path marker `t` is a step below (itself).
    pub(crate) fn path_root(&self, t: Tok) -> Tok {
        let mut x = t;
        while let Some(&(p, _)) = self.path_parent.get(&x) {
            if !self.path_parent.contains_key(&p) {
                break;
            }
            x = p;
        }
        x
    }

    /// Path marker `t`'s markers and keys from its parameter on.
    pub(crate) fn path_keys(&self, t: Tok) -> Vec<(Tok, Option<Atom>)> {
        let mut v = Vec::new();
        let mut x = t;
        while let Some(&(p, k)) = self.path_parent.get(&x) {
            v.push((x, k));
            x = p;
        }
        v.reverse();
        v
    }

    /// DECISIONS 158: path marker `t`'s step under key `k` (`None`: a
    /// computed key): the values the callee read there, which the reader
    /// finds by walking the same keys on its argument. A key already on
    /// the path folds back to the marker of that earlier step, which then
    /// repeats the keys of the cycle (`x = x.next` in a loop): a path
    /// holds each key once, so a unit's markers are finite, with no depth
    /// cutoff.
    pub(crate) fn path_step_of(&mut self, t: Tok, k: Option<Atom>) -> Tok {
        if let Some(&s) = self.path_step.get(&(t, k)) {
            return s;
        }
        let path = self.path_keys(t);
        let s = match path.iter().position(|&(_, pk)| pk == k) {
            Some(j) => {
                let at = path[j].0;
                let l = self.path_loops.entry(at).or_default();
                for x in path[j + 1..].iter().map(|&(_, pk)| pk).chain([k]) {
                    if !l.contains(&x) {
                        l.push(x);
                    }
                }
                at
            }
            None => {
                let TokKind::Obj(tm, _) = self.toks[t as usize] else {
                    unreachable!("a marker is an object token")
                };
                let key = match k {
                    Some(a) => self.atoms[a as usize].clone(),
                    None => "[]".to_string(),
                };
                let display = format!("{}.{key}", self.tmpls[tm as usize].display);
                let rm = self.marker_template(display);
                let s = self.tok(TokKind::Obj(rm, NO_CTX));
                self.reach_marker.insert(s);
                if let Some(&fi) = self.param_marker.get(&t) {
                    self.param_marker.insert(s, fi);
                }
                self.path_parent.insert(s, (t, k));
                self.path_steps.entry(t).or_default().push(s);
                if self.marker_is_top(t) {
                    self.set_marker_top(s);
                }
                s
            }
        };
        self.path_step.insert((t, k), s);
        s
    }

    /// A use `bits` of what argument marker `t` reaches through some named
    /// key (design §18.4). Each marker it stands for takes the bits: a
    /// reach marker as a use of itself (it reaches itself under every key),
    /// an argument marker passes them on, once per bit. A parameter marker
    /// takes nothing here: which keys it was used under is a fact for its
    /// contract alone, settled after the solve (`settle_keyed_uses`). So a
    /// key costs the use that names it, not a walk over every marker the
    /// argument marker stands for.
    pub(crate) fn keyed_use(&mut self, t: Tok, bits: u8) {
        let mut st = vec![(t, bits)];
        while let Some((x, b)) = st.pop() {
            // Each marker the walk visits is solve work (113-CWL3).
            self.work += 1;
            if self.reach_marker.contains(&x) {
                self.mark_use(x, b);
                continue;
            }
            if !self.is_arg_marker(x) {
                continue;
            }
            let u = self.keyed_any.entry(x).or_default();
            let new = b & !*u;
            *u |= b;
            if new != 0 {
                st.extend(self.arg_members[&x].iter().map(|&m| (m, new)));
            }
        }
    }

    /// The keyed uses of each parameter marker that some argument marker
    /// stands for (design §18.4, DECISIONS 44): the keys every argument
    /// marker standing for it, directly or through other argument markers,
    /// was used under. Read once the model is final, in one pass over the
    /// member graph: the markers of a cycle (a recursion passing its
    /// argument on) stand for each other and have one set of uses, so its
    /// strongly connected components are taken in topological order and
    /// each key set moves along each edge between them once.
    pub(crate) fn settle_keyed_uses(&mut self) {
        self.keyed_uses.clear();
        // The argument markers a keyed use walks through (`keyed_use`).
        let inner = |s: &Self, x: Tok| s.is_arg_marker(x) && !s.reach_marker.contains(&x);
        let mut ids: Vec<Tok> = self
            .arg_members
            .keys()
            .copied()
            .filter(|&x| inner(self, x))
            .collect();
        ids.sort_unstable();
        let at: FxHashMap<Tok, usize> = ids.iter().enumerate().map(|(i, &t)| (t, i)).collect();
        let n = ids.len();
        let succ: Vec<Vec<usize>> = ids
            .iter()
            .map(|x| {
                self.arg_members[x]
                    .iter()
                    .filter_map(|m| at.get(m).copied())
                    .collect()
            })
            .collect();
        // Tarjan, iterative: a component gets its number after every
        // component it reaches, so descending numbers are topological.
        const NONE: usize = usize::MAX;
        let (mut index, mut low, mut comp) = (vec![NONE; n], vec![0; n], vec![NONE; n]);
        let mut on = vec![false; n];
        let (mut stack, mut next, mut ncomp) = (Vec::new(), 0usize, 0usize);
        for s0 in 0..n {
            if index[s0] != NONE {
                continue;
            }
            let mut frames = vec![(s0, 0usize)];
            index[s0] = next;
            low[s0] = next;
            next += 1;
            stack.push(s0);
            on[s0] = true;
            while let Some(&(v, e)) = frames.last() {
                if let Some(&w) = succ[v].get(e) {
                    frames.last_mut().unwrap().1 += 1;
                    if index[w] == NONE {
                        index[w] = next;
                        low[w] = next;
                        next += 1;
                        stack.push(w);
                        on[w] = true;
                        frames.push((w, 0));
                    } else if on[w] {
                        low[v] = low[v].min(index[w]);
                    }
                    continue;
                }
                frames.pop();
                if let Some(&(u, _)) = frames.last() {
                    low[u] = low[u].min(low[v]);
                }
                if low[v] == index[v] {
                    while let Some(w) = stack.pop() {
                        on[w] = false;
                        comp[w] = ncomp;
                        if w == v {
                            break;
                        }
                    }
                    ncomp += 1;
                }
            }
        }
        let mut members_of: Vec<Vec<usize>> = vec![Vec::new(); ncomp];
        for v in 0..n {
            members_of[comp[v]].push(v);
        }
        let mut acc: Vec<FxHashMap<Atom, u8>> = vec![FxHashMap::default(); ncomp];
        let mut out: FxHashMap<Tok, FxHashMap<Atom, u8>> = FxHashMap::default();
        let merge = |into: &mut FxHashMap<Atom, u8>, from: &FxHashMap<Atom, u8>| {
            for (&a, &b) in from {
                *into.entry(a).or_default() |= b;
            }
        };
        for c in (0..ncomp).rev() {
            let mut set = std::mem::take(&mut acc[c]);
            // Its own uses: what its keyed reach markers were used for.
            for &v in &members_of[c] {
                for &(a, r) in self.keyed_reach.get(&ids[v]).into_iter().flatten() {
                    let u = self.marker_use.get(&r).copied().unwrap_or(0);
                    if u != 0 {
                        *set.entry(a).or_default() |= u;
                    }
                }
            }
            if set.is_empty() {
                continue;
            }
            for &v in &members_of[c] {
                for &m in &self.arg_members[&ids[v]] {
                    match at.get(&m) {
                        Some(&w) if comp[w] != c => merge(&mut acc[comp[w]], &set),
                        Some(_) => {}
                        None if self.param_marker.contains_key(&m)
                            && !self.reach_marker.contains(&m) =>
                        {
                            merge(out.entry(m).or_default(), &set)
                        }
                        None => {}
                    }
                }
            }
        }
        for (p, set) in out {
            let mut v: Vec<(Atom, u8)> = set.into_iter().collect();
            v.sort_unstable();
            self.keyed_uses.insert(p, v);
        }
    }

    pub(crate) fn set_marker_top(&mut self, t: Tok) {
        let w = (t >> 6) as usize;
        if self.marker_top.len() <= w {
            self.marker_top.resize(w + 1, 0);
        }
        self.marker_top[w] |= 1 << (t & 63);
    }

    /// A marker that reads as ⊤ (DECISIONS 42).
    #[inline]
    pub(crate) fn marker_is_top(&self, t: Tok) -> bool {
        marker_top_bit(&self.marker_top, t)
    }

    /// An escaped value beside ⊤ is that ⊤ (DECISIONS 42's reason, for a
    /// value): unknown code holds it, so its fields are ⊤ and sink what they
    /// get, every function reachable from it runs with ⊤, and its layout is
    /// escaped; a rule on it at a node that holds ⊤ does nothing the rule on
    /// ⊤ does not. Left out there, it never rides ⊤ through the unit. A
    /// marker is decided by `marker_is_top` (its escape may be no sink,
    /// decision 17).
    #[inline]
    pub(crate) fn top_beside(&self, t: Tok) -> bool {
        self.is_escaped(t) && !self.is_marker(t)
    }

    /// A marker that stands for no value of its own (DECISIONS 74): one of
    /// a closed function's described parameter (decision 17), what it
    /// reaches, or an argument marker all of whose members are such
    /// markers (DECISIONS 75), and what that reaches. The callers' values
    /// are the contract's, beside it in every node it reaches (they arrive
    /// together), so it adds no kinds.
    /// DECISIONS 165: a method no unit runs (`Gen::published_args`), once
    /// the solve knows it is a method of a published class
    /// (`published_this`): its parameter markers stand for no value.
    /// DECISIONS 224: so for a function published to a known closed callee
    /// (`callable_pub`), whose runners publish what they pass as a method's
    /// do: a run one's described parameters hold that join beside them.
    pub(crate) fn method_described(&self, f: FnIdx) -> Option<&Vec<bool>> {
        (self.published_this.contains_key(&f) || self.callable_pub.contains(&f))
            .then(|| self.method_described.get(&f))
            .flatten()
    }

    /// DECISIONS 165: a parameter marker of a published method no unit
    /// runs (`method_described`), not of a closed function: nothing is
    /// beside it because nothing calls it, so the calls through it are
    /// still recorded for a unit that will run it (DECISIONS 140): the
    /// assumption narrows what its sites read, never its contract.
    pub(crate) fn unrun_param(&self, t: Tok) -> bool {
        self.param_marker.get(&t).is_some_and(|&(f, i)| {
            i != u32::MAX
                && !self.closed_described.contains_key(&f)
                && !self.method_params.contains_key(&f)
                && self.method_described(f).is_some()
        })
    }

    pub(crate) fn beside_values(&self, t: Tok) -> bool {
        match self.param_marker.get(&t) {
            // A published method's receiver marker whose receivers the
            // running units published (DECISIONS 107): they are beside it.
            Some(&(f, u32::MAX)) if self.this_args.contains_key(&f) => true,
            // A closed function's or a published method's described
            // parameter (decision 13, DECISIONS 165): what the callers
            // pass is beside it.
            Some(&(f, i)) => self
                .closed_described
                .get(&f)
                .or_else(|| self.method_described(f))
                .and_then(|d| d.get(i as usize))
                .is_some_and(|&d| d),
            None => self.beside_markers.contains(&t),
        }
    }

    /// A marker with no values beside it (DECISIONS 119): it stands for
    /// another unit's argument whole, and every emitter reads it as ⊤ with
    /// every primitive kind.
    pub(crate) fn bare_marker(&self, t: Tok) -> bool {
        self.is_marker(t) && !self.beside_values(t)
    }

    /// A marker standing only for parameters whose callers' values are
    /// beside it: a closed function's (decision 13), a published method's
    /// receiver with published receivers (DECISIONS 107) or a described
    /// parameter of a run method (DECISIONS 148). The marker only records
    /// the uses for its contract.
    pub(crate) fn closed_marker(&self, t: Tok) -> bool {
        self.is_marker(t)
            && self.marker_members(t).iter().all(|x| {
                self.param_marker.get(x).is_some_and(|&(f, i)| {
                    self.closed_params.contains_key(&f)
                        || (i == u32::MAX && self.this_args.contains_key(&f))
                        || (i != u32::MAX
                            && self
                                .method_described(f)
                                .and_then(|d| d.get(i as usize))
                                .is_some_and(|&d| d))
                })
            })
    }

    /// A parameter marker or an argument marker (DECISIONS 44).
    #[inline]
    pub(crate) fn is_marker(&self, t: Tok) -> bool {
        self.param_marker.contains_key(&t) || self.is_arg_marker(t)
    }

    #[inline]
    pub(crate) fn is_arg_marker(&self, t: Tok) -> bool {
        marker_top_bit(&self.arg_bits, t)
    }

    /// The parameter markers token `t` stands for: itself, unless it is an
    /// argument marker (DECISIONS 44). What an argument marker reaches under
    /// a key (its keyed reach marker, whose uses are settled per key and
    /// which has no members of its own) is part of what it reaches: it
    /// stands for its reach marker's members.
    ///
    /// Settled once per marker over the member graph's strongly connected
    /// components (a recursion's markers stand for each other and have one
    /// set), each component's set the union of its members' leaves and its
    /// successors' sets, shared by its members: the readers after a solve
    /// ask for the same markers at every site, result and contract.
    pub(crate) fn marker_members(&self, t: Tok) -> Vec<Tok> {
        if !self.is_arg_marker(t) {
            return vec![t];
        }
        if let Some(s) = self.members_settled.borrow().get(&t) {
            return s.to_vec();
        }
        self.settle_members_from(t);
        self.members_settled.borrow()[&t].to_vec()
    }

    /// The successors of argument marker `x` in the member graph: its
    /// members, or for a keyed reach marker its argument marker's reach.
    pub(super) fn member_succ(&self, x: Tok) -> Vec<Tok> {
        if let Some(&(p, _)) = self.keyed_parent.get(&x) {
            return vec![self.marker_reach[&p]];
        }
        self.arg_members.get(&x).cloned().unwrap_or_default()
    }

    /// Tarjan from `root` over the argument markers not settled yet; a
    /// settled marker and a parameter marker are leaves of the walk.
    pub(super) fn settle_members_from(&self, root: Tok) {
        use crate::pts::{FxHashMap, FxHashSet};
        let mut index: FxHashMap<Tok, (u32, u32)> = FxHashMap::default();
        let mut on: FxHashSet<Tok> = FxHashSet::default();
        let mut stack: Vec<Tok> = Vec::new();
        let mut next = 0u32;
        // (marker, its successors, the next successor to visit)
        let mut frames: Vec<(Tok, Vec<Tok>, usize)> = Vec::new();
        let succ = self.member_succ(root);
        index.insert(root, (0, 0));
        next += 1;
        stack.push(root);
        on.insert(root);
        frames.push((root, succ, 0));
        while let Some(frame) = frames.last_mut() {
            let (v, ref ss, ref mut i) = *frame;
            if *i < ss.len() {
                let w = ss[*i];
                *i += 1;
                if !self.is_arg_marker(w) || self.members_settled.borrow().contains_key(&w) {
                    continue;
                }
                match index.get(&w) {
                    None => {
                        index.insert(w, (next, next));
                        next += 1;
                        stack.push(w);
                        on.insert(w);
                        let ws = self.member_succ(w);
                        frames.push((w, ws, 0));
                    }
                    Some(&(wi, _)) if on.contains(&w) => {
                        let e = index.get_mut(&v).unwrap();
                        e.1 = e.1.min(wi);
                    }
                    Some(_) => {}
                }
                continue;
            }
            let (vi, vlow) = index[&v];
            frames.pop();
            if let Some(&(u, _, _)) = frames.last() {
                let e = index.get_mut(&u).unwrap();
                e.1 = e.1.min(vlow);
            }
            if vlow != vi {
                continue;
            }
            // The component is complete: its leaves and its successors'
            // settled sets, one set for every member.
            let mut comp: Vec<Tok> = Vec::new();
            while let Some(w) = stack.pop() {
                on.remove(&w);
                comp.push(w);
                if w == v {
                    break;
                }
            }
            let mut set: Vec<Tok> = Vec::new();
            {
                let settled = self.members_settled.borrow();
                for &w in &comp {
                    for s in self.member_succ(w) {
                        if !self.is_arg_marker(s) {
                            set.push(s);
                        } else if let Some(ss) = settled.get(&s) {
                            set.extend(ss.iter().copied());
                        }
                        // else: a member of this component.
                    }
                }
            }
            set.sort_unstable();
            set.dedup();
            let rc: std::rc::Rc<[Tok]> = set.into();
            let mut settled = self.members_settled.borrow_mut();
            for w in comp {
                settled.insert(w, rc.clone());
            }
        }
    }

    /// Parameter node `n`'s argument marker, with its own reach marker.
    pub(crate) fn arg_marker(&mut self, n: NodeId) -> Tok {
        if let Some(&t) = self.arg_marker_of.get(&n) {
            return t;
        }
        let tm = self.marker_template(format!("args{n}"));
        let t = self.tok(TokKind::Obj(tm, NO_CTX));
        let rm = self.marker_template(format!("args{n}:reach"));
        let r = self.tok(TokKind::Obj(rm, NO_CTX));
        for x in [t, r] {
            let w = (x >> 6) as usize;
            if self.arg_bits.len() <= w {
                self.arg_bits.resize(w + 1, 0);
            }
            self.arg_bits[w] |= 1 << (x & 63);
            self.arg_members.insert(x, Vec::new());
        }
        self.marker_reach.insert(t, r);
        self.arg_reach_base.insert(r, t);
        self.reach_marker.insert(r);
        self.arg_marker_of.insert(n, t);
        self.arg_marker_node.insert(t, n);
        // A relative parameter's marker is the caller's argument at each
        // call's result while it stays in its run (DECISIONS 44 (b)).
        if self.nodes[n as usize].rel {
            let home = self.nodes[n as usize].frame;
            self.set_fam(t, t, home);
            self.set_fam(r, t, home);
        }
        // It stands for its members alone (DECISIONS 75): beside while it
        // has none, and while each one that links in is beside
        // (`link_marker`). The caller links the arriving markers before
        // the marker reaches any node.
        self.beside_markers.insert(t);
        self.beside_markers.insert(r);
        let any = self.field_any(t);
        self.add_tok(any, r);
        let rany = self.field_any(r);
        self.add_tok(rany, r);
        t
    }

    /// DECISIONS 200: drops the settled member sets of `r` and of every
    /// argument marker whose set includes it (`member_succ` reversed: the
    /// markers `r` is a member of, a keyed reach's holders, an argument
    /// reach's keyed reaches). A settled set is built from its successors'
    /// settled sets, so the settled markers are closed downward: the walk
    /// ends at a marker with no settled set. Clearing every set at each
    /// link made a solve that asks between links (`own_param_members`,
    /// `closed_marker`) settle the whole member graph again each time
    /// (fastify_inject per unit never finished; main 4.6 s).
    pub(super) fn unsettle_up(&mut self, r: Tok) {
        let mut st = vec![r];
        while let Some(y) = st.pop() {
            if self.members_settled.get_mut().remove(&y).is_none() {
                continue;
            }
            st.extend(self.arg_parents.get(&y).into_iter().flatten().copied());
            if let Some(&(p, a)) = self.keyed_parent.get(&y) {
                st.extend(
                    self.arg_keyed_parents
                        .get(&p)
                        .into_iter()
                        .flatten()
                        .filter(|&&(k, _)| k == a)
                        .map(|&(_, q)| q),
                );
            }
            if let Some(&p) = self.arg_reach_base.get(&y) {
                st.extend(
                    self.keyed_reach
                        .get(&p)
                        .into_iter()
                        .flatten()
                        .map(|&(_, kr)| kr),
                );
            }
        }
    }

    /// Argument marker `r` stands for marker `m` too: `m` takes every use
    /// of `r`, and what `m` reaches takes every use of what `r` reaches.
    pub(crate) fn link_marker(&mut self, m: Tok, r: Tok) {
        if m == r || !self.arg_linked.insert((r, m)) {
            return;
        }
        self.arg_members.get_mut(&r).unwrap().push(m);
        // A new link changes the sets settled for `r` and the markers above
        // it (`marker_members`), no others (DECISIONS 200).
        if !self.members_settled.get_mut().is_empty() {
            self.unsettle_up(r);
        }
        // DECISIONS 193: `r` takes `m`'s readers, and from now on what `m`
        // gains (`replay_marker_links`).
        if self.is_arg_marker(m) {
            match self.keyed_parent.get(&m).copied() {
                Some((p, a)) => self.arg_keyed_parents.entry(p).or_default().push((a, r)),
                None => self.arg_parents.entry(m).or_default().push(r),
            }
        }
        self.marker_links.push((m, r));
        if self.beside_markers.contains(&r) {
            match self.beside_values(m) {
                true if !self.param_marker.contains_key(&m) => {
                    self.beside_up.entry(m).or_default().push(r)
                }
                true => {}
                false => self.leave_beside(r),
            }
        }
        let u = self.marker_use.get(&r).copied().unwrap_or(0);
        if u != 0 {
            self.mark_use(m, u);
        }
        let reach = |s: &Self, x: Tok| s.marker_reach.get(&x).copied().unwrap_or(x);
        let (mr, rr) = (reach(self, m), reach(self, r));
        if (mr, rr) != (m, r) {
            self.link_marker(mr, rr);
        }
        // And per key (§18.4): what `m` reaches through a key takes the
        // uses of what `r` reaches through it so far, as bits (later ones:
        // `keyed_use`; the keys: `settle_keyed_uses`).
        let k = self.keyed_any.get(&r).copied().unwrap_or(0);
        if k != 0 {
            self.keyed_use(m, k);
        }
    }

    /// Argument marker `t` stands for a value of its own now (DECISIONS
    /// 75): so do what it reaches and the argument markers it is a member
    /// of, and each arithmetic result that skipped it takes the object
    /// kinds it would have taken. Beside only ever turns off: monotone.
    pub(crate) fn leave_beside(&mut self, t: Tok) {
        let mut st = vec![t];
        while let Some(x) = st.pop() {
            if !self.beside_markers.remove(&x) {
                continue;
            }
            for (dst, out) in self.beside_arith.remove(&x).unwrap_or_default() {
                self.add_prim(dst, out);
            }
            st.extend(self.beside_up.remove(&x).unwrap_or_default());
            st.extend(self.marker_reach.get(&x).copied());
            if let Some(ks) = self.keyed_reach.get(&x) {
                st.extend(ks.iter().map(|&(_, r)| r));
            }
        }
    }

    pub(super) fn set_fam(&mut self, t: Tok, base: Tok, home: u32) {
        let i = t as usize;
        if self.fam.len() <= i {
            self.fam.resize(i + 1, (0, 0));
        }
        self.fam[i] = (base, home);
    }

    /// The home of token `t`'s argument marker family (+ 1), 0 for any
    /// other token (DECISIONS 44 (b)).
    #[inline]
    pub(crate) fn fam_home(&self, t: Tok) -> u32 {
        self.fam.get(t as usize).map_or(0, |&(_, h)| h)
    }

    /// Marker `t` arriving at node `n` crosses into it as a member of
    /// `n`'s own marker (MRK; DECISIONS 44 for parameters): a marker is a
    /// token only inside its run. A parameter takes every marker; a node
    /// of no run of `t`'s family (another run's node, storage) takes its
    /// family tokens; a result or storage takes the markers of no run
    /// (another node's marker, a contract parameter's). Storage is every
    /// node of no run but an observation node, a sink and a marker's own
    /// field nodes. Inside its run a family token stays itself, so a
    /// result can return it relative to each call (44 (b)).
    #[inline]
    pub(crate) fn crosses(&self, n: NodeId, t: Tok) -> bool {
        let node = &self.nodes[n as usize];
        if node.param {
            return self.is_marker(t);
        }
        if node.obs || node.sink || node.of_marker || !self.is_marker(t) {
            return false;
        }
        match self.fam_home(t) {
            0 => node.result || node.frame == 0,
            h => h != node.frame,
        }
    }

    /// What a load from marker `m` gives (solve.rs `field`): under a named
    /// key its keyed reach marker, under a computed key its reach marker;
    /// a reach marker reaches itself under every key.
    pub(crate) fn marker_load(&mut self, m: Tok, key: Option<Atom>) -> Tok {
        if self.reach_marker.contains(&m) {
            return m;
        }
        match key {
            Some(a) => self.keyed_reach_of(m, a),
            None => self.marker_reach[&m],
        }
    }

    /// What marker `m` reaches under a computed key: its reach marker, or
    /// itself for a reach marker.
    pub(crate) fn reach_of(&self, m: Tok) -> Tok {
        if self.reach_marker.contains(&m) {
            return m;
        }
        self.marker_reach.get(&m).copied().unwrap_or(m)
    }

    /// Token `t` of argument marker `b`'s family, with marker `m` in place
    /// of `b`: `m` itself, what `m` reaches, or what it reaches through the
    /// same key (DECISIONS 44 (b)).
    pub(crate) fn lift(&mut self, m: Tok, b: Tok, t: Tok) -> Tok {
        if t == b {
            return m;
        }
        let key = self.keyed_parent.get(&t).map(|&(_, a)| a);
        self.marker_load(m, key)
    }

    /// The run of `call` (`Node::frame`): the run its result node belongs
    /// to (a call an instance made has its own result slot). A temporary
    /// the solver makes for one call is of that run.
    pub(crate) fn call_run(&self, call: CallId) -> u32 {
        let Some(c) = self.calls.get(call as usize) else {
            return 0;
        };
        // Its operands are nodes of the run that made it.
        let this = match c.this {
            ThisArg::Node(n) => Some(n),
            _ => None,
        };
        std::iter::once(c.ret)
            .chain(this)
            .chain(c.args.iter().copied())
            .filter(|&n| n != NO_NODE)
            .map(|n| self.nodes[n as usize].frame)
            .find(|&f| f != 0)
            .unwrap_or(0)
    }

    /// The tokens of argument marker `b`'s family.
    pub(crate) fn family(&self, b: Tok) -> Vec<Tok> {
        let mut v = vec![b];
        v.extend(self.marker_reach.get(&b).copied());
        v.extend(
            self.keyed_reach
                .get(&b)
                .into_iter()
                .flatten()
                .map(|&(_, r)| r),
        );
        v
    }

    pub(super) fn marker_template(&mut self, display: String) -> TmplId {
        let tm = self.tmpls.len() as TmplId;
        self.tmpls.push(Template {
            kind: TmplKind::BObj {
                module: 0,
                site: 0,
                what: "parameter marker",
            },
            display,
            birth_keys: Vec::new(),
            definite: Vec::new(),
            declared: Vec::new(),
            parent: None,
            ctor: None,
            native_parent: None,
            accessor_keys: Vec::new(),
            has_computed_members: false,
            field_init: None,
            proto_members: Vec::new(),
            parent_value: None,
            parent_links: Vec::new(),
            null_proto: false,
        });
        tm
    }

    /// The marker of parameter `i` of contract function `f`.
    pub(crate) fn param_marker_tok(&mut self, f: FnIdx, i: u32) -> Tok {
        if let Some(&t) = self.marker_of.get(&(f, i)) {
            return t;
        }
        let tm = self.tmpls.len() as TmplId;
        // `u32::MAX`: the receiver of a published method (DECISIONS 57).
        let display = match i {
            u32::MAX => format!("{}:this", self.funcs[f as usize].name),
            _ => format!("{}:param{i}", self.funcs[f as usize].name),
        };
        self.tmpls.push(Template {
            kind: TmplKind::BObj {
                module: 0,
                site: 0,
                what: "parameter marker",
            },
            display,
            birth_keys: Vec::new(),
            definite: Vec::new(),
            declared: Vec::new(),
            parent: None,
            ctor: None,
            native_parent: None,
            accessor_keys: Vec::new(),
            has_computed_members: false,
            field_init: None,
            proto_members: Vec::new(),
            parent_value: None,
            parent_links: Vec::new(),
            null_proto: false,
        });
        let t = self.tok(TokKind::Obj(tm, NO_CTX));
        // What the argument reaches has its own marker (fields of fields
        // reach it again), so a result can tell the argument itself from
        // values reachable from it (§11.2 "the return").
        let rm = self.tmpls.len() as TmplId;
        let mut reach = self.tmpls[tm as usize].clone();
        reach.display = format!("{}:reach", self.tmpls[tm as usize].display);
        self.tmpls.push(reach);
        let r = self.tok(TokKind::Obj(rm, NO_CTX));
        self.param_marker.insert(t, (f, i));
        self.param_marker.insert(r, (f, i));
        self.reach_marker.insert(r);
        self.marker_reach.insert(t, r);
        self.marker_of.insert((f, i), t);
        // Markers before their field nodes: those nodes are of the marker.
        let any = self.field_any(t);
        self.add_tok(any, r);
        let rany = self.field_any(r);
        self.add_tok(rany, r);
        t
    }
}
