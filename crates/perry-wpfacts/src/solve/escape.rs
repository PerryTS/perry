//! Escaping tokens: what an escape publishes (classes, function instances, properties) and the hard markers it raises.

use super::*;

impl Model {
    /// Escapes `t`, and whether that took effect: a stand-in published
    /// onward does not escape (`escape`), so only an escape that took
    /// effect is a change of a unit's contract check (DECISIONS 180).
    pub fn escape_changed(&mut self, t: Tok, reason: u32) -> bool {
        if self.is_escaped(t) {
            return false;
        }
        self.escape(t, reason);
        self.is_escaped(t)
    }

    pub fn escape(&mut self, t: Tok, reason: u32) {
        // DECISIONS 225: unknown code may call an escaped function.
        if self.act.is_some() {
            if let TokKind::Func(f, _) = self.toks[t as usize] {
                self.activate(f, "escape");
            }
        }
        if self.diagnostic.is_none() {
            return self.escape_body(t, reason);
        }
        let first = self.diagnostic_escape_begin(t, reason);
        self.escape_body(t, reason);
        self.diagnostic_escape_end(t, first);
    }

    fn escape_body(&mut self, t: Tok, reason: u32) {
        if t == TOP {
            return;
        }
        // Unknown code given the Function constructor may call it, and ⊤
        // may then be it (DECISIONS 235). Until then a ⊤ callee is not it:
        // the program is closed, and no native returns the constructor; the
        // token enters a value only as the global `Function`, a function's
        // `constructor` (a computed key that may be it included), or here.
        if matches!(self.toks[t as usize], TokKind::Builtin(BuiltinNs::Function)) {
            crate::builtins::interpreted_code_runs(self);
        }
        // Another unit's function the census broke, forwarded where no
        // contract describes it: its owner already lets unknown code call it
        // (any receiver, any arguments, DECISIONS 209), so its leaving tells
        // nothing more, and no class it is a member of leaves with it
        // (DECISIONS 212).
        if let TokKind::Func(f, _) = self.toks[t as usize] {
            let fi = &self.funcs[f as usize];
            if fi.func_id == u32::MAX
                && !self.broken_fns.is_empty()
                && self.broken_fns.contains(&format!("fn:{}", fi.name))
                && self.reasons[reason as usize] == crate::gen::UNDESCRIBED
            {
                return;
            }
        }
        if let Some(&old) = self.escaped.get(&t) {
            let contract = |m: &Self, r: u32| {
                m.pub_reason == Some(r)
                    || m.field_fn_reason == Some(r)
                    || m.closed_pub_reason == Some(r)
            };
            if contract(self, old) && !contract(self, reason) {
                if let TokKind::Func(f, _) = self.toks[t as usize] {
                    // DECISIONS 212: a contract function that left through
                    // its contract only and now also through no contract (a
                    // value no contract describes, DECISIONS 203; unknown
                    // code) is called by that code with any receiver and
                    // any arguments, which no unit publishes: its `this` and
                    // parameters take ⊤ beside what the contract gave them.
                    if self.contract_fns.contains(&f) && self.funcs[f as usize].func_id != u32::MAX
                    {
                        self.escaped.insert(t, reason);
                        if !self.funcs[f as usize].lexical_this {
                            self.escape_this(t, reason);
                        }
                        self.escape_fn_params(t, reason);
                    }
                    // DECISIONS 210: one that left with its properties
                    // (`escape_props`): that code may construct it, which no
                    // unit publishes (`ctor_runs`).
                    if self.props_escaped.contains_key(&t) {
                        self.construct_unknown(t, f, reason);
                    }
                }
            }
            return;
        }
        // Another unit's class, instance, prototype or function (a
        // stand-in) published onward leaves with its owner's contract, as a
        // result forwards it (DECISIONS 79/104; DECISIONS 120: every
        // publication): the next unit reads it through that contract,
        // nothing of this unit leaves with it, and it is no census report
        // of this unit. What this unit stores into it is (`Gen::reports`).
        // A closed contract's users are known to its owner: it escapes.
        // Interpreted code would hold it beyond any contract (decision 150).
        if self.interp_free
            && self.dyn_eval.is_none()
            && (self.pub_reason == Some(reason) || self.closed_pub_reason == Some(reason))
            && !self.is_cell_arr(t)
            && self.stand_in(t)
        {
            return;
        }
        // DECISIONS 174: another unit's value read through cell `thrown`
        // reaches interpreted code as that unit threw it. The first unit to
        // throw it escaped it (each thrower sinks what it throws), and that
        // is its write; the others re-escaped it, each publishing ⊤ for
        // what it reaches (opencode: every unit ⊤ for the ~210 arrays the
        // program throws, 27M cell writes in round 3).
        if self.thrown_reason == Some(reason) && self.thrown_in.contains(&t) && self.stand_in(t) {
            return;
        }
        if self.closed_pub_reason == Some(reason) {
            if let TokKind::Func(f, _) = self.toks[t as usize] {
                if self.closed_params.contains_key(&f) {
                    if self.closed_pub.insert(t) {
                        self.escape_fn_params(t, reason);
                    }
                    return;
                }
            }
        }
        // DECISIONS 210: a function of this unit published through its
        // contract (`contract_fns`; an export, a published literal's field,
        // a contract result): its code is the contract (markers on its
        // parameters, any receiver), and its own properties leave with it
        // (`escape_props`), all but `prototype`: a reader reaches that only
        // as the owner's published `new <f>` layout's (`foreign_ctor`), a
        // store into it or a read of it before the layout exists is its
        // census report (`fn_touched`). So the instances this unit
        // constructs keep their chain, as a published class's do.
        if self.pub_reason == Some(reason) || self.field_fn_reason == Some(reason) {
            if let TokKind::Func(f, _) = self.toks[t as usize] {
                if self.contract_fns.contains(&f) && self.funcs[f as usize].func_id != u32::MAX {
                    self.mark_use(t, crate::model::MARK_ESCAPE);
                    self.mark_escaped(t, reason);
                    self.rel(|| format!("escaped {t}"));
                    // Its own properties are read by code no contract
                    // describes (DECISIONS 212).
                    let rp = self.beyond_contract(reason);
                    self.escape_props(t, rp);
                    if !self.funcs[f as usize].lexical_this {
                        self.escape_this(t, reason);
                    }
                    self.escape_fn_params(t, reason);
                    return;
                }
            }
        }
        // A builtin prototype as the operand of a compiled construct the
        // model does not follow (`Gen::fail_closed`) may get any key written
        // (`fnproto_write` fails closed). The runtime's own operations and
        // callbacks are not that: they never write the intrinsics, and a
        // compiled function called from unknown code sees ⊤ parameters, whose
        // writes `store_into` records.
        if self.array_member_write.is_none()
            && t != TOP
            && self.tok_on_array_chain(t)
            && self.reasons[reason as usize].starts_with(crate::gen_expr::UNMODELED_OPERAND)
        {
            let what = format!("reaches unmodeled code ({})", self.reasons[reason as usize]);
            self.note_array_member_write(t, None, None, &what);
        }
        if (self.fnproto_write.is_none() || self.fnproto_borrow_write.is_none())
            && matches!(self.toks[t as usize], TokKind::ObjectProto)
            && self.reasons[reason as usize].starts_with(crate::gen_expr::UNMODELED_OPERAND)
        {
            let what = format!("reaches unmodeled code ({})", self.reasons[reason as usize]);
            self.note_fnproto_write(t, None, &what);
        }
        if matches!(
            self.toks[t as usize],
            TokKind::ObjectProto | TokKind::NativeProto(_)
        ) && self.reasons[reason as usize].starts_with(crate::gen_expr::UNMODELED_OPERAND)
        {
            let what = format!("reaches unmodeled code ({})", self.reasons[reason as usize]);
            crate::builtins::note_native_write(self, t, None, &what);
            if let Some(s) = self.shared_of(t) {
                self.note_shared(SharedWrite::Unmodeled(s));
            }
        }
        if matches!(
            self.toks[t as usize],
            TokKind::Builtin(BuiltinNs::GlobalThis)
        ) {
            crate::builtins::note_global_in_top(self);
            if !self.unit_mode {
                crate::builtins::note_native_write(
                    self,
                    t,
                    None,
                    "global object given to unknown code",
                );
            }
            self.note_shared(SharedWrite::Unmodeled(Shared::Builtin(
                BuiltinNs::GlobalThis,
            )));
        }
        // Per unit, unknown code includes the other units' code: a
        // runtime-owned object escaping reaches code that holds it as ⊤, and
        // that code's stores into ⊤ may land in it (`units::shared_closure`).
        // A value contract names one instead (`ValueContract::shared`): its
        // readers hold it, nothing escapes. The runtime's own code never
        // writes one (above). One reached along an escaped value's prototype
        // chain is not given: a store into ⊤ is no store into a builtin
        // prototype, as whole-program facts hold of ⊤ from unknown code. The
        // global object takes no part: escaped, it already puts ⊤ in reach
        // of stores into ⊤ (`global_in_top`).
        if self.chain_escape && builtins::builtin_function(self, t) {
            return;
        }
        if !self.chain_escape {
            if let Some(s) = self
                .shared_of(t)
                .filter(|s| *s != Shared::Builtin(BuiltinNs::GlobalThis))
            {
                self.note_shared(SharedWrite::Handed(s));
            }
        }
        // Runtime-owned objects do not "escape": unknown code already holds
        // them. Object.prototype's unmodeled mutations are E5/E6 (modeled
        // operations) or E14 (interpreted code), never a side effect of some
        // value reaching unknown code; its user-stored members escape where
        // they are stored (`builtins::tok_store`).
        if !builtins::builtin_function(self, t)
            && matches!(
                self.toks[t as usize],
                TokKind::ObjectProto | TokKind::Builtin(_) | TokKind::NativeProto(_)
            )
        {
            // Handed to unknown code (not reached along a chain): the
            // program fields rule R1 gave it may be read there.
            if !self.chain_escape {
                self.builtin_fields_sink(t, Some(reason));
            }
            // Unknown code given `process` (or the global object) may write
            // the array `process.argv` holds.
            if matches!(
                self.toks[t as usize],
                TokKind::Builtin(BuiltinNs::Process | BuiltinNs::GlobalThis)
            ) {
                let a = self.process_argv_tok;
                self.escape(a, reason);
            }
            return;
        }
        if self.rel_phase != b'S' {
            self.rel(|| format!("escapeseed {t} {reason}"));
        }
        self.with_s(|m| m.escape_inner(t, reason));
        for s in self.wide_sites(t) {
            self.escape(s, reason);
        }
    }

    /// Another unit's class, instance, prototype, function or array this
    /// unit holds through a contract (a stand-in).
    fn stand_in(&self, t: Tok) -> bool {
        let fwd = |f: Option<FnIdx>| f.is_some_and(|f| self.funcs[f as usize].forwarded);
        match self.toks[t as usize] {
            TokKind::Obj(tm, _) if self.is_cell_arr(t) => matches!(
                self.tmpls[tm as usize].kind,
                TmplKind::Arr { site: u32::MAX, .. } | TmplKind::BObj { site: u32::MAX, .. }
            ),
            TokKind::Obj(tm, _) | TokKind::ClassObj(tm) | TokKind::Proto(tm) => {
                self.foreign.contains_key(&tm) && fwd(self.tmpls[tm as usize].ctor)
            }
            TokKind::Func(f, _) => fwd(Some(f)),
            _ => false,
        }
    }

    /// DECISIONS 212: the reason what a value escaping with `reason`
    /// reaches escapes with: a publication (`pub_reason`, a published
    /// literal's field function) describes the value itself, not what
    /// unknown code then reads from it (DECISIONS 203, `UNDESCRIBED`).
    pub(crate) fn beyond_contract(&mut self, reason: u32) -> u32 {
        match self.pub_reason == Some(reason) || self.field_fn_reason == Some(reason) {
            true => self.reason(crate::gen::UNDESCRIBED),
            false => reason,
        }
    }

    pub(super) fn mark_escaped(&mut self, t: Tok, reason: u32) {
        // A few hard/contract paths mark directly rather than entering escape.
        // Retain those actual events too; this data never feeds the solver.
        if let Some(d) = &mut self.diagnostic {
            if let Some(c) = &mut d.census {
                c.escapes.insert((
                    t,
                    d.cause_node.unwrap_or(NO_NODE),
                    d.cur_call.unwrap_or(u32::MAX),
                    d.escaping.last().copied().unwrap_or(u32::MAX),
                    reason,
                    c.active_gate,
                ));
            }
        }
        self.escaped.insert(t, reason);
        if matches!(self.toks[t as usize], TokKind::ClassObj(_)) {
            self.flush_shadowed(Some(t));
        }
        let w = (t >> 6) as usize;
        if self.esc_bits.len() <= w {
            self.esc_bits.resize(w + 1, 0);
        }
        self.esc_bits[w] |= 1 << (t & 63);
    }

    /// Unknown code calls `f`: unknown arguments, the result escapes.
    pub(crate) fn escape_fn_params(&mut self, ft: Tok, reason: u32) {
        if !self.sum.ready {
            self.sum
                .deferred
                .push(crate::summary::Deferred::Escape(ft, reason));
            return;
        }
        let f = self.tok_fn(ft);
        if self.summarized(f) {
            self.escape_summarized(ft, reason);
            return;
        }
        let fi = self.funcs[f as usize].clone();
        let why = |m: &Self| format!("param of escaped function: {}", m.reasons[reason as usize]);
        // A closed function's callers are known: what they pass. Only
        // through its publication (decision 19); any other escape lets
        // unknown code call it.
        let closed = if self.closed_pub_reason == Some(reason) {
            self.closed_params.get(&f).cloned()
        } else {
            None
        };
        // DECISIONS 119: published through its contract only, a parameter
        // is its marker, the whole argument: no ⊤, no primitive kinds.
        let contract_only = self.contract_fns.contains(&f) && self.pub_reason == Some(reason);
        for (i, &p) in fi.params.iter().enumerate() {
            // A hidden capture parameter holds its class environment slot
            // whoever calls (DECISIONS 117).
            if let Some(env) = self.capture_param(f, i) {
                self.edge(env, p);
                continue;
            }
            match closed.as_ref().and_then(|c| c.get(i).copied()) {
                Some(a) => self.edge(a, p),
                None => {
                    if !contract_only {
                        self.add_top(p, why);
                        self.add_prim(p, prim::ALL);
                    }
                }
            }
        }
        // The rest parameter is the fresh array the call makes, whoever
        // calls: unknown arguments are its elements, never the array itself
        // (a store into its elements is no store into ⊤).
        if let Some(rest) = fi.rest {
            let r = self.field_any_node_of_node(rest);
            self.add_top(r, why);
            self.add_prim(r, prim::ALL);
        }
        if self.contract_fns.contains(&f) {
            // Design §11.2: other units call it through its contract.
            for (i, &p) in fi.params.iter().enumerate() {
                if self.capture_param(f, i).is_some() {
                    continue;
                }
                let mk = self.param_marker_tok(f, i as u32);
                self.add_tok(p, mk);
                // DECISIONS 148: what the running units pass, beside it.
                if contract_only {
                    if let Some(&a) = self.method_params.get(&f).and_then(|v| v.get(i)) {
                        if a != NO_NODE {
                            self.edge(a, p);
                        }
                    }
                }
            }
            self.add_top(fi.args_all, why);
            self.contract_rets.push((f, fi.ret));
            // Beyond its publication (an export or a published literal's
            // field, DECISIONS 210, or known closed callers, DECISIONS 19),
            // unknown code calls it and retains its result.
            if self.pub_reason != Some(reason)
                && self.field_fn_reason != Some(reason)
                && closed.is_none()
            {
                self.sink_node(fi.ret, reason);
            }
            return;
        }
        self.add_top(fi.args_all, why);
        self.sink_node(fi.ret, reason);
    }

    /// A class published to other units as a contract (design §11.2), not
    /// an escape: they may construct it with unknown arguments, read every
    /// field of what they construct (getters run, contents leave), call its
    /// methods and accessors on those instances, and use its static members
    /// as unknown code does. Its methods run with `this` = ⊤ (any receiver)
    /// and that instance. Everything else they do to it (stores, opening
    /// operations, the prototype or a method used as a value, passing it to
    /// code outside the program) is their census report; a broken class is
    /// published as an escape instead.
    ///
    /// A method called by another unit runs on one of those instances: its
    /// `this` holds the class's receiver token. Reading through it gives ⊤
    /// (as reading through ⊤ does: the method sees whatever other units did),
    /// and storing into it, an opening operation on it or its escape happen
    /// to the constructed instance (`contract_recv`). Reading the instance's
    /// own contents there would run every method over every value other
    /// units can put in it (commander's Command: 217 receiver layouts per
    /// method).
    ///
    /// When a class of another unit extends it (`extended`), its
    /// constructor is a function contract as well, run on any receiver (a
    /// subclass instance's `super()`): `this` is ⊤ there, so its sites are
    /// proven behind the entry guard only (DECISIONS 57). A class with no
    /// constructor code publishes an empty constructor contract.
    ///
    /// A `sealed` class (DECISIONS 116) is one no other unit names: it is
    /// published because this unit gives its instances out. No other unit
    /// constructs or extends it, so its methods run on those instances
    /// only (`Model::escape_this`), never on an unknown receiver.
    pub(crate) fn publish_class(
        &mut self,
        tm: TmplId,
        reason: u32,
        extended: bool,
        no_code: bool,
        sealed: bool,
    ) -> Vec<(Atom, NodeId)> {
        if sealed {
            self.sealed.insert(tm);
        }
        let cobj = self.tok(TokKind::ClassObj(tm));
        let inst = self.tok(TokKind::Obj(tm, NO_CTX));
        let rt = self.tmpls.len() as TmplId;
        let display = format!("{}:receiver", self.tmpls[tm as usize].display);
        self.tmpls.push(Template {
            kind: TmplKind::BObj {
                module: 0,
                site: 0,
                what: "contract receiver",
            },
            display,
            birth_keys: Vec::new(),
            definite: Vec::new(),
            declared: Vec::new(),
            reserved: Vec::new(),
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
            chain_miss: false,
            chain_miss_open: false,
            species: None,
            per_evaluation: false,
            static_methods: Vec::new(),
        });
        let recv = self.tok(TokKind::Obj(rt, NO_CTX));
        let any = self.field_any(recv);
        self.add_top(any, |m| {
            format!(
                "receiver of a method another unit calls: {}",
                m.reasons[reason as usize]
            )
        });
        self.add_prim(any, prim::ALL);
        // The instance other units construct has the class's prototype.
        self.proto_node(inst);
        // A sealed class's receiver stands for the instances given out
        // only: no other unit constructs one.
        self.contract_recv
            .insert(recv, if sealed { Vec::new() } else { vec![inst] });
        self.contract_recv_of.insert(tm, recv);
        self.recv_class.insert(recv, tm);
        // `this.m()` through the receiver finds the class's `m` (DECISIONS 59).
        self.register_virt(recv, &[tm]);
        // Static members and the class object's own fields. A static
        // private field (`#x`, `Model::private_atom`) is named by the
        // class body only: it is no member of the published class.
        let pa = self.atom("prototype");
        let mut named: Vec<(Atom, NodeId)> = self
            .fields
            .iter()
            .filter(|(&(h, a), _)| h == cobj && a != pa && !self.is_private_atom(a))
            .map(|(&(_, a), &n)| (a, n))
            .collect();
        named.sort_unstable();
        let mut statics: Vec<NodeId> = named.iter().map(|&(_, n)| n).collect();
        statics.extend(self.field_any.get(&cobj).copied());
        statics.sort_unstable();
        // Other units call a static method on the class object, through
        // its function contract (§11.2: a static member is part of the
        // class contract; its result is checked like any contract result).
        for &n in &statics {
            for t in self.nodes[n as usize].pts.to_sorted() {
                if let TokKind::Func(f, _) = self.toks[t as usize] {
                    let broken = self
                        .broken_fns
                        .contains(&format!("fn:{}", self.funcs[f as usize].name));
                    if !self.is_escaped(t) && self.funcs[f as usize].func_id != u32::MAX && !broken
                    {
                        self.contract_fns.insert(f);
                        // A static function leaves through its contract
                        // (markers on its parameters), whatever else the
                        // static holds (DECISIONS 173).
                        self.escape(t, reason);
                    }
                    // Its receiver is the class object, whatever call
                    // runs it: every instance of a summarized body sees it.
                    if !self.funcs[f as usize].lexical_this {
                        let this = self.this_any(t);
                        self.add_tok(this, cobj);
                    }
                }
            }
        }
        // A named static leaves as a published value binding does (DECISIONS
        // 173): the caller checks it after the solve (`Gen::contract_check`),
        // so an instance of this class it holds is given out, not escaped
        // with its prototype chain. What a computed key holds is unknown.
        if let Some(&any) = self.field_any.get(&cobj) {
            self.sink_node(any, reason);
        }
        // Methods along the chain inside the unit: they run on `inst` and on
        // unknown receivers.
        let ca = self.atom("constructor");
        let mut cur = Some(tm);
        let mut members: Vec<NodeId> = Vec::new();
        while let Some(c) = cur {
            // Another unit's class (a stand-in parent): its methods are that
            // unit's, called through its contract (DECISIONS 57).
            if self.foreign.contains_key(&c) {
                break;
            }
            let proto = self.tok(TokKind::Proto(c));
            members.extend(
                self.fields
                    .iter()
                    .filter(|(&(h, a), _)| h == proto && a != ca)
                    .map(|(_, &n)| n),
            );
            members.extend(self.field_any.get(&proto).copied());
            cur = self.tmpls[c as usize].parent;
        }
        members.sort_unstable();
        // Each method (and the constructor) is also a function contract
        // (DECISIONS 57): other units pass its arguments by their marks,
        // and its receiver marker publishes what it does to `this`, so a
        // subclass instance of another unit can run it.
        for &n in &members {
            for t in self.nodes[n as usize].pts.to_sorted() {
                if let TokKind::Func(f, _) = self.toks[t as usize] {
                    let rs = self.published_this.entry(f).or_default();
                    if !rs.contains(&recv) {
                        rs.push(recv);
                    }
                    let broken = self
                        .broken_fns
                        .contains(&format!("fn:{}", self.funcs[f as usize].name));
                    if !self.is_escaped(t) && self.funcs[f as usize].func_id != u32::MAX && !broken
                    {
                        self.contract_fns.insert(f);
                    }
                }
            }
        }
        let t = &self.tmpls[tm as usize];
        match t.ctor {
            Some(ctor) if extended => {
                let ft = self.funcs[ctor as usize].token;
                let broken = self
                    .broken_fns
                    .contains(&format!("fn:{}", self.funcs[ctor as usize].name));
                if !self.is_escaped(ft) && self.funcs[ctor as usize].func_id != u32::MAX && !broken
                {
                    self.published_this.entry(ctor).or_default().push(recv);
                    self.contract_fns.insert(ctor);
                    self.escape_fn_params(ft, reason);
                    self.escape_this(ft, reason);
                    self.ctor_published.insert(tm);
                }
            }
            // No constructor code: what construction does to the receiver
            // is the class's own shape (its field initializers), whoever
            // extends it; an extension through a value (a CommonJS
            // `require`) names no import the census could match
            // (DECISIONS 99).
            None if no_code => {
                self.ctor_published.insert(tm);
            }
            _ => {}
        }
        for n in members {
            self.sink_node(n, reason);
        }
        if sealed {
            return named;
        }
        // Other units construct it with unknown arguments.
        let ret = self.node_l("contract-new");
        let args_n = self.node_l("contract-args");
        self.add_top(args_n, |m| {
            format!(
                "constructor args from another unit: {}",
                m.reasons[reason as usize]
            )
        });
        self.add_prim(args_n, prim::ALL);
        // A closed class (design §18.13): what the constructing units pass.
        let ctor = self.tmpls[tm as usize].ctor;
        let mut args = match ctor.and_then(|f| self.closed_params.get(&f)) {
            Some(nodes) => nodes.clone(),
            None => vec![args_n; 8],
        };
        // Its hidden capture parameters take the class's environment, not
        // what the constructing unit passes (DECISIONS 117).
        if let Some(f) = ctor {
            for i in 0..self.funcs[f as usize].params.len() {
                if let Some(env) = self.capture_param(f, i) {
                    if args.len() <= i {
                        args.resize(i + 1, args_n);
                    }
                    args[i] = env;
                }
            }
        }
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            this: ThisArg::Undef,
            args,
            spread: vec![],
            ret,
            site: 0,
        });
        self.contract_new.insert(call);
        self.construct(call, cobj);
        // ... and read its fields: they leave key by key, described by the
        // class contract (`Gen::fields_leave`, DECISIONS 88).
        named
    }

    /// DECISIONS 210/211: unknown code constructs a function of this unit
    /// that escaped, with unknown arguments, and its instance escapes, as
    /// for an escaped class (`escape_inner`). The one path for "unknown code
    /// constructs an escaped function", in both modes: whole program, code
    /// nothing models holds it (an escaped `F` nothing here constructs
    /// still has instances of its layout); per unit, other units' code holds
    /// it too and may construct it (the whole program sees that `new`;
    /// fastify `new Reply`, escaped through `Module._cache`: the units
    /// constructing it hold it as an unknown value, so only the units
    /// writing `Reply.prototype` hold the layout, with no instance of their
    /// own). Callers: an escape (`escape_inner`), a template made after the
    /// escape (`fn_inst_template`), and a contract-only escape that later
    /// leaves through no contract (`escape`). Only a function with a construction layout: one
    /// whose body stores into `this` (`fn_birth`) or whose `new <f>`
    /// template exists (`fn_inst_template` constructs it when made later);
    /// any other one's instances would hold nothing a layout claims. Never
    /// an arrow, a generator step, a class constructor (its class escapes)
    /// or another unit's stand-in (its owner constructs it on the census
    /// report, `Gen::published_ctors`).
    pub(crate) fn construct_unknown(&mut self, t: Tok, f: FnIdx, reason: u32) {
        let fi = &self.funcs[f as usize];
        let np = fi.params.len().max(1);
        // Published through its contract, the units constructing it pass
        // their arguments (`ctor_runs`): constructed by `Gen::published_ctors`.
        let published = self.pub_reason == Some(reason) || self.field_fn_reason == Some(reason);
        if published
            || fi.lexical_this
            || fi.gen_step()
            || fi.ctor_of.is_some()
            || fi.func_id == u32::MAX
            || !(self.fn_birth.contains_key(&f) || self.fn_inst.contains_key(&f))
            || !self.unknown_new.insert(f)
        {
            return;
        }
        let ret = self.node_l("top-new");
        let args_n = self.node_l("top-args");
        self.add_top(args_n, |m| {
            format!(
                "constructor args from unknown code: {}",
                m.reasons[reason as usize]
            )
        });
        self.add_prim(args_n, prim::ALL);
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            this: ThisArg::Undef,
            args: vec![args_n; np],
            spread: vec![],
            ret,
            site: 0,
        });
        self.construct(call, t);
        self.sink_node(ret, reason);
    }

    /// DECISIONS 210: unknown code holds function `t` (published through
    /// its contract): it reads and writes any own property but `prototype`
    /// (that one is the published layout's chain, `escape`). Each named
    /// property leaves; what a computed key stores leaves; a read under an
    /// unknown key reads what unknown code wrote; its accessors run as for
    /// any escaped object.
    pub(crate) fn escape_props(&mut self, t: Tok, reason: u32) {
        if self.props_escaped.insert(t, reason).is_some() {
            return;
        }
        let published = self.unit_mode && self.reasons[reason as usize] == crate::gen::UNDESCRIBED;
        let pa = self.atom("prototype");
        let keys: Vec<Atom> = self.field_keys.get(&t).cloned().unwrap_or_default();
        for a in keys {
            if a != pa {
                let n = self.fields[&(t, a)];
                match published {
                    true => self.cons(n, Cons::TopSink { reason }),
                    false => self.escape_prop(t, n, reason),
                }
            }
        }
        let any = self.field_any(t);
        self.cons(any, Cons::TopSink { reason });
        // DECISIONS 241: published to another unit (`UNDESCRIBED`), its own
        // properties hold what the units store there, each publishing its
        // stores (`fn_stored`): every holder reads their join. Only unknown
        // code makes them unknown (`fn_props_top`).
        if !published {
            self.fn_props_top.insert(t);
            let all = self.all_fields(t);
            self.add_top(all, |m| {
                format!(
                    "property of an escaped function: {}",
                    m.reasons[reason as usize]
                )
            });
            self.add_prim(all, prim::ALL);
        }
        let w = self.node_l("escaped-store");
        self.add_top(w, |m| {
            format!(
                "stored into an escaped function: {}",
                m.reasons[reason as usize]
            )
        });
        self.add_prim(w, prim::ALL);
        self.accessor_walk(t, t, Key::any(), ChainAct::Store(w));
        let r = self.node_l("escaped-load");
        self.sink_node(r, reason);
        self.accessor_walk(t, t, Key::any(), ChainAct::Load(r));
    }

    /// A named property `n` of function `t` whose properties left
    /// (`escape_props`): unknown code writes it and reads what it holds.
    pub(crate) fn escape_prop(&mut self, t: Tok, n: NodeId, reason: u32) {
        self.add_top(n, |m| {
            format!(
                "property of an escaped function {t}: {}",
                m.reasons[reason as usize]
            )
        });
        self.add_prim(n, prim::ALL);
        self.cons(n, Cons::TopSink { reason });
    }

    /// DECISIONS 210: the instances of this unit's constructor function
    /// `f` (template `tm`), constructed here for the units that construct
    /// it, are a layout those units hold (`contracts::fn_inst_contract`).
    /// The functions on its prototype (`Proto(tm)`, what `f.prototype`
    /// holds) run on `inst` and on unknown receivers, each a contract
    /// function, as a published class's methods do (`publish_class`); any
    /// other value there leaves.
    pub(crate) fn publish_fn_inst(&mut self, f: FnIdx, tm: TmplId, inst: Tok, reason: u32) {
        let ca = self.atom("constructor");
        let proto = self.tok(TokKind::Proto(tm));
        let mut members: Vec<NodeId> = self
            .field_keys
            .get(&proto)
            .into_iter()
            .flatten()
            .filter(|&&a| a != ca)
            .map(|&a| self.fields[&(proto, a)])
            .collect();
        members.extend(self.field_any.get(&proto).copied());
        members.sort_unstable();
        members.dedup();
        for &n in &members {
            for t in self.nodes[n as usize].pts.to_sorted() {
                if let TokKind::Func(g, _) = self.toks[t as usize] {
                    let broken = self
                        .broken_fns
                        .contains(&format!("fn:{}", self.funcs[g as usize].name));
                    if !self.is_escaped(t) && self.funcs[g as usize].func_id != u32::MAX && !broken
                    {
                        self.contract_fns.insert(g);
                    }
                    if !self.funcs[g as usize].lexical_this {
                        for this in self.this_nodes(t) {
                            self.add_tok(this, inst);
                        }
                    }
                }
            }
        }
        for n in members {
            self.sink_node(n, reason);
        }
        let _ = f;
    }

    /// This unit gives `t`, an instance of its published class `tm`, to
    /// other units (a contract result or value): they call its methods, so
    /// what those do to their receiver happens to `t` as well, replayed
    /// from the receiver's log and forwarded from now on. True when `t` is
    /// new here.
    pub(crate) fn give_out(&mut self, tm: TmplId, t: Tok) -> bool {
        let Some(&recv) = self.contract_recv_of.get(&tm) else {
            return false;
        };
        let given = self.contract_recv.entry(recv).or_default();
        if given.contains(&t) {
            return false;
        }
        given.push(t);
        // A sealed class's methods (DECISIONS 116) and the methods run on
        // published receivers (DECISIONS 125) run on `t` from now on.
        let sealed = self.sealed.contains(&tm);
        let ds: Vec<NodeId> = self
            .given_this
            .iter()
            .filter(|(f, _)| {
                self.published_this
                    .get(f)
                    .is_some_and(|rs| rs.contains(&recv))
            })
            .filter(|(&f, _)| sealed || self.join_holds(f, t))
            .flat_map(|(_, ds)| ds.iter().copied())
            .collect();
        for d in ds {
            self.add_tok(d, t);
        }
        for e in self.recv_log.get(&recv).cloned().unwrap_or_default() {
            match e {
                crate::model::RecvEffect::Store(key, src, site) => {
                    self.store_into(t, key, src, site)
                }
                crate::model::RecvEffect::Hard(kind, reason) => self.mark_hard(t, kind, reason),
                crate::model::RecvEffect::Escape(reason) => self.escape(t, reason),
            }
        }
        true
    }

    pub(super) fn escape_inner(&mut self, t: Tok, reason: u32) {
        // DECISIONS 212: a value that escapes beyond any contract under the
        // publication reason (no stand-in forwarded, no contract function)
        // is held by code no contract describes: what it reaches leaves
        // beyond any contract too (DECISIONS 203), its contract functions
        // included (`escape`).
        let reason = self.beyond_contract(reason);
        self.mark_escape_why(t, &|m: &Self| match m
            .diagnostic
            .as_ref()
            .and_then(|d| d.esc_ctx.last())
        {
            Some(o) => format!("{o} / escape: {}", m.reasons[reason as usize]),
            None => format!("escape: {}", m.reasons[reason as usize]),
        });
        self.virt_escape(t);
        self.note_inst(t, false);
        if let Some(is) = self.contract_recv.get(&t).cloned() {
            self.recv_log
                .entry(t)
                .or_default()
                .push(crate::model::RecvEffect::Escape(reason));
            for i in is {
                self.escape(i, reason);
            }
        }
        // A marker: unknown code gets what it stands for, and what that
        // reaches (its fields are its reach markers). The values sit beside
        // it and escape there (DECISIONS 111).
        if self.is_marker(t) {
            let r = self.reach_of(t);
            self.mark_escape_why(r, &|m: &Self| match m
                .diagnostic
                .as_ref()
                .and_then(|d| d.esc_ctx.last())
            {
                Some(o) => format!("{o} / escape: {}", m.reasons[reason as usize]),
                None => format!("escape: {}", m.reasons[reason as usize]),
            });
            self.mark_escaped(t, reason);
            return;
        }
        if matches!(self.toks[t as usize], TokKind::Proto(_)) {
            self.mark_escaped(t, reason);
            self.resume_stopped(t);
        }
        self.mark_escaped(t, reason);
        self.rel(|| format!("escaped {t}"));
        // DECISIONS 241: unknown code may store any key of a function.
        if matches!(self.toks[t as usize], TokKind::Func(..)) {
            self.fn_props_top.insert(t);
        }
        // Overlapping abstract native tokens contribute read values, never
        // writes into each other's storage: the overlap is not transitive.
        if builtins::builtin_function(self, t) {
            let read = self.node_l("escaped-native-fields");
            self.builtin_field_use(t, Key::any(), ChainAct::Load(read));
            if !self.unit_mode {
                self.proto_value(t, read);
            }
            self.sink_node(read, reason);
        }
        // Unknown code reads every field and can write any key.
        let all = self.all_fields(t);
        self.cons(all, Cons::TopSink { reason });
        // A module namespace object is not writable: [[Set]] and
        // [[DefineOwnProperty]] fail, [[Delete]] of an export fails (ES
        // 10.4.6); its keys hold what its module's bindings hold.
        if matches!(self.toks[t as usize], TokKind::ModNs(_)) {
            return;
        }
        let any = self.any_key_node(t);
        self.add_top(any, |m| {
            format!(
                "field of escaped {}: {}",
                m.tok_kind_word(t),
                m.reasons[reason as usize]
            )
        });
        self.add_prim(any, prim::ALL);
        if let Some(tm) = self.tmpl_of(t) {
            *self.adds_keys.entry(tm).or_default() += 1;
        }
        // Its prototypes are reachable (`Object.getPrototypeOf`).
        let pn = self.proto_node(t);
        self.chain_sinks.insert(pn);
        self.cons(pn, Cons::TopSink { reason });
        // Unknown code calls the functions it reaches through `t` (its
        // fields and prototype sink above, so each escapes): they run in
        // their unknown-code instance with `this` = ⊤, never in the
        // instance of `t`'s layout (an escaped value binds no layout).
        // Unknown code can also write any key of it: every setter on its
        // chain (its own accessors included) runs with `this` = it and an
        // unknown value (a direct eval writes the caller's locals exactly so,
        // through the setters of its bindings object); and read any key:
        // every getter runs and its result reaches unknown code.
        let w = self.node_l("escaped-store");
        self.add_top(w, |m| {
            format!(
                "stored into an escaped {}: {}",
                m.tok_kind_word(t),
                m.reasons[reason as usize]
            )
        });
        self.add_prim(w, prim::ALL);
        self.accessor_walk(t, t, Key::any(), ChainAct::Store(w));
        let r = self.node_l("escaped-load");
        self.sink_node(r, reason);
        self.accessor_walk(t, t, Key::any(), ChainAct::Load(r));
        match self.toks[t as usize].clone() {
            TokKind::Func(f, _) => {
                let fi = self.funcs[f as usize].clone();
                if !fi.lexical_this {
                    self.escape_this(t, reason);
                }
                self.escape_fn_params(t, reason);
                self.construct_unknown(t, f, reason);
            }
            TokKind::ClassObj(tm) => {
                let p = self.tok(TokKind::Proto(tm));
                self.escape(p, reason);
                // Unknown code may construct it with unknown arguments.
                let ret = self.node_l("top-new");
                let args_n = self.node_l("top-args");
                self.add_top(args_n, |m| {
                    format!(
                        "constructor args from unknown code: {}",
                        m.reasons[reason as usize]
                    )
                });
                self.add_prim(args_n, prim::ALL);
                let call = self.calls.len() as CallId;
                self.calls.push(CallSite {
                    this: ThisArg::Undef,
                    args: vec![args_n; 8],
                    spread: vec![],
                    ret,
                    site: 0,
                });
                self.construct(call, t);
                let esc = self.escaped_classes;
                self.add_tok(esc, t);
                self.sink_node(ret, reason);
                // DEC221: only what unknown construction returns escapes.
                // NO_CTX is a template-wide mark, which would also expose
                // every existing private birth of this layout.
            }
            TokKind::Obj(tm, _) => {
                // `.constructor` reaches the class value.
                if let TmplKind::Class { .. } = self.tmpls[tm as usize].kind {
                    let c = self.tok(TokKind::ClassObj(tm));
                    self.escape(c, reason);
                }
                if let TmplKind::FnInst { func } = self.tmpls[tm as usize].kind {
                    let ft = self.funcs[func as usize].token;
                    self.escape(ft, reason);
                }
            }
            TokKind::Proto(tm) => {
                let c = self.tok(TokKind::ClassObj(tm));
                // `.constructor` on the prototype.
                if matches!(self.tmpls[tm as usize].kind, TmplKind::Class { .. }) {
                    self.escape(c, reason);
                }
            }
            TokKind::Bound(b) => {
                let (target, bthis, bargs) = self.bound[b as usize].clone();
                for n in [target, bthis].into_iter().chain(bargs) {
                    self.sink_node(n, reason);
                }
            }
            _ => {}
        }
    }

    /// DECISIONS 242(1): `Object.prototype` holds an accessor under any key
    /// (`STAR_KEY` in its `accessor_keys`): a key read there may run unknown
    /// code and yield anything.
    pub(crate) fn objproto_any_accessor(&mut self) {
        let op = self.tok(TokKind::ObjectProto);
        let star = self.atom(crate::STAR_KEY);
        let ks = self.accessor_keys.entry(op).or_default();
        if ks.contains(&star) {
            return;
        }
        ks.push(star);
        let fa = self.any_key_node(op);
        self.add_top(fa, |_| "accessor under a computed key".into());
        self.add_prim(fa, prim::ALL);
    }

    pub fn mark_hard(&mut self, t: Tok, kind: Hard, reason: u32) {
        // DEC 250: a delete that reaches the runtime's prototypes (the
        // collapsed `ObjectProto`) removes only a key the lock admits: the
        // runtime refuses (TypeError) to delete a member the image gives a
        // locked family, except `constructor` (`builtin_proto::
        // locked_violation`), and a family it leaves open may lose any key.
        // That is a native write of every key (below), which a final read
        // never sees (`members::locked_read`, the same decision). It removes
        // no key of a program object and installs no setter: no layout on a
        // chain through it changes, no own key stops shadowing it.
        let lock_delete =
            kind == Hard::E7Delete && matches!(self.toks[t as usize], TokKind::ObjectProto);
        self.virt_revoke(t, None);
        // DECISIONS 234: under a computed key, a defineProperty on a
        // prototype of the runtime, or through ⊤ (which may be one), may
        // redefine any key there (`reshape`); a delete or `with` through ⊤
        // may remove any key of an escaped object (`top_reshape`). Every
        // unit sees it: the other units' objects escape into ⊤ too. On an
        // object it may remove its own keys; on a receiver marker, those of
        // the other units' instances of its class, which are ⊤ here.
        if matches!(
            kind,
            Hard::E5DefineProperty | Hard::E7Delete | Hard::E15With
        ) {
            if t == TOP {
                if kind == Hard::E5DefineProperty {
                    self.reshape(None);
                    self.note_shared(SharedWrite::Hard(Shared::Top, kind));
                }
                self.top_reshape(None);
                self.note_shared(SharedWrite::Removed(None));
            } else if self.is_chain_proto(t) {
                if !lock_delete {
                    self.reshape(None);
                }
            } else {
                self.note_inst(t, true);
            }
        }
        if matches!(
            kind,
            Hard::E5DefineProperty | Hard::E7Delete | Hard::E15With
        ) {
            if let Some(tm) = self.tmpl_of(t) {
                if self.keys_unsure.insert(tm) {
                    self.flush_present_reads(tm);
                }
            }
        }
        if kind == Hard::E6SetPrototypeOf {
            self.super_relink.insert(t);
        }
        if matches!(
            kind,
            Hard::E5DefineProperty | Hard::E6SetPrototypeOf | Hard::E15With
        ) {
            self.note_array_member_write(t, None, None, kind.code());
        }
        if matches!(
            kind,
            Hard::E5DefineProperty | Hard::E7Delete | Hard::E15With
        ) {
            self.note_fnproto_write(t, None, kind.code());
        }
        if matches!(
            kind,
            Hard::E5DefineProperty | Hard::E6SetPrototypeOf | Hard::E7Delete | Hard::E15With
        ) {
            let what = match self.explain {
                true => format!("{} ({})", kind.code(), self.reasons[reason as usize]),
                false => kind.code().to_string(),
            };
            // `setPrototypeOf` writes no own key: the runtime object's new
            // chain is noted by the prototype it is given (`ProtoFrom`,
            // `settle_native_given`). Another unit's gives one unknown here.
            if kind != Hard::E6SetPrototypeOf || self.shared_replay {
                crate::builtins::note_native_write(self, t, None, &what);
            }
        }
        if t == TOP {
            // Another unit passes the same runtime objects: an E6 on ⊤ is
            // its E6 too (`replay_shared`); before, it reached no other unit.
            if kind == Hard::E6SetPrototypeOf {
                self.note_shared(SharedWrite::Hard(Shared::Top, kind));
            }
            return;
        }
        if let Some(is) = self.contract_recv.get(&t).cloned() {
            self.recv_log
                .entry(t)
                .or_default()
                .push(crate::model::RecvEffect::Hard(kind, reason));
            for i in is {
                self.mark_hard(i, kind, reason);
            }
        }
        self.mark_escape_why(t, &|_| "hard: reflective operation".into());
        // `Object.setPrototypeOf(Object.prototype, p)` throws unless `p` is
        // null (its [[Prototype]] already): nothing changes (immutable
        // prototype exotic object). A ⊤ target that may be it is no E6 on it.
        if kind == Hard::E6SetPrototypeOf && matches!(self.toks[t as usize], TokKind::ObjectProto) {
            return;
        }
        // DECISIONS 217: another unit's lookups see a HARD shared object.
        if let Some(s) = self.shared_of(t) {
            self.note_shared(SharedWrite::Hard(s, kind));
        }
        // DECISIONS 242(1): a defineProperty under a computed key on
        // `Object.prototype` is an accessor under any key there, in every
        // mode (another unit's arrives as `Hard(ObjectProto, E5)` and runs
        // here too): it reaches an object only through a key the object does
        // not own (derive), never as a HARD chain.
        if kind == Hard::E5DefineProperty && matches!(self.toks[t as usize], TokKind::ObjectProto) {
            self.objproto_any_accessor();
            return;
        }
        if lock_delete {
            return;
        }
        if self.hard.contains_key(&t) {
            return;
        }
        if self.rel_phase != b'S' {
            self.rel(|| format!("hardseed {t} {} {reason}", kind.code()));
        }
        if matches!(self.toks[t as usize], TokKind::Proto(_)) {
            self.hard.insert(t, (kind, reason));
            self.with_s(|m| m.resume_stopped(t));
        }
        self.hard.insert(t, (kind, reason));
        if matches!(self.toks[t as usize], TokKind::ClassObj(_)) {
            self.flush_shadowed(Some(t));
        }
        self.rel(|| format!("hardmark {t} {}", kind.code()));
        // Deleting / redefining can move values or install accessors: the
        // object's fields are now written by unknown code as well.
        let any = self.any_key_node(t);
        self.add_prim(any, prim::ALL);
        if let Some(tm) = self.tmpl_of(t) {
            *self.adds_keys.entry(tm).or_default() += 1;
        }
        for s in self.wide_sites(t) {
            self.mark_hard(s, kind, reason);
        }
    }
}
