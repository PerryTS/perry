//! Callee binding: `this` and arguments of the calls a lookup's callee reaches.

use crate::model::*;

impl Model {
    /// Bind `call` to every function that shows up in `callee`, with `this`
    /// fixed to `origin` (the relation `callthis callee call origin`).
    ///
    /// The rule splits in two so a megamorphic site does not pay
    /// receivers × methods per call: `this` binds once per (method node,
    /// receiver), shared by every site that looks the same key up on that
    /// receiver (`ThisTo`), and arguments/result bind once per (call,
    /// function) through the call's relay node (`CallArgs`). A builtin
    /// namespace value runs per (call, receiver) as before (`BuiltinCallee`).
    /// The result is the same as one `CallThis` per (callee, call, origin).
    pub(super) fn bind_via(&mut self, callee: NodeId, call: CallId, origin: Tok) {
        let pc = self.prov_call;
        if self.prov.is_some() {
            self.prov_call = call + 1;
        }
        self.bind_via_bound(callee, call, origin);
        self.prov_call = pc;
    }

    pub(super) fn bind_via_bound(&mut self, callee: NodeId, call: CallId, origin: Tok) {
        if !self.callthis_done.insert((callee, call, origin)) {
            return;
        }
        self.rel(|| format!("callthis {callee} {call} {origin}"));
        self.via_calls
            .entry((callee, origin))
            .or_default()
            .push(call);
        if self.this_done.insert((callee, origin)) {
            self.cons(callee, Cons::ThisTo { origin });
        } else {
            let mut toks = self.bufs.pop().unwrap_or_default();
            toks.clear();
            self.nodes[callee as usize].pts.sorted_into(&mut toks);
            // A replay per new call is solve work.
            self.charge_work(toks.len() as u64);
            for &t in &toks {
                // Each token's binding replays rules: the bound holds here.
                if self.past_work_bound() {
                    break;
                }
                self.this_calls(t, origin, &[call]);
            }
            self.bufs.push(toks);
        }
        let relay = match self.relays.get(&call) {
            Some(&r) => r,
            None => {
                let r = self.node_l("callee-relay");
                self.nodes[r as usize].frame = self.call_run(call);
                self.relays.insert(call, r);
                self.cons(r, Cons::CallArgs { call });
                r
            }
        };
        if self.relay_done.insert((call, callee)) {
            self.edge_quiet(callee, relay);
        }
        let sites = self.callee_sites.entry(callee).or_default();
        let first = sites.is_empty();
        sites.push((call, origin));
        if first {
            self.cons(callee, Cons::BuiltinCallee);
        } else if self.builtin_callee.contains(&callee) {
            let mut toks = self.bufs.pop().unwrap_or_default();
            toks.clear();
            self.nodes[callee as usize].pts.sorted_into(&mut toks);
            // A replay per new call is solve work.
            self.charge_work(toks.len() as u64);
            for &t in &toks {
                if self.past_work_bound() {
                    break;
                }
                if self.per_receiver_callee(t) {
                    self.call_tok(call, t, Some(origin));
                }
            }
            self.bufs.push(toks);
        }
    }

    /// The `this` half of `call_tok(_, t, Some(origin))`.
    pub(super) fn this_to(&mut self, t: Tok, origin: Tok, calls: &[CallId]) {
        match self.toks[t as usize] {
            // It runs per (call, receiver) (`BuiltinCallee`).
            TokKind::BuiltinFn(Native::Member { .. }) => {}
            TokKind::Top | TokKind::BuiltinFn(_) => {
                if let Some(d) = self.diagnostic.as_mut() {
                    let n = d.cause_node;
                    for &c in calls {
                        d.unknown_calls.entry(c).or_insert((n, t == TOP));
                        d.unknown_callee.entry(c).or_insert(t);
                    }
                }
                let r = if self.explain {
                    let fs: Vec<String> = calls
                        .iter()
                        .map(|&c| {
                            let f = self.node_fn[self.calls[c as usize].ret as usize];
                            let p = if self.protocol_calls.contains(&c) {
                                " protocol"
                            } else {
                                ""
                            };
                            let name = self
                                .funcs
                                .get(f as usize)
                                .map_or("(no function)", |f| f.name.as_str());
                            format!("{name}#{c}{p}")
                        })
                        .collect();
                    let why = format!(
                        "receiver of a call of an unknown function value ({} callee, receiver {}, calls [{}])",
                        self.callee_name(t),
                        self.tok_name(origin),
                        fs.join(", ")
                    );
                    self.reason(&why)
                } else if self.diagnostic.is_some() {
                    let cs: Vec<String> = calls.iter().map(|c| c.to_string()).collect();
                    self.reason(&format!(
                        "receiver of a call of an unknown function value [call {}]",
                        cs.join(",")
                    ))
                } else {
                    self.reason("receiver of a call of an unknown function value")
                };
                if self.keysnap.is_some() {
                    if let Some(&c) = calls.first() {
                        self.keysnap_note(c, t, origin);
                    }
                }
                self.escape(origin, r);
            }
            TokKind::Func(f, _) => {
                if !self.funcs[f as usize].lexical_this {
                    self.this_lookup(t, origin);
                }
            }
            _ => {}
        }
    }

    /// The instance half of `this_to`: each call bound through the method
    /// node binds its instance of `t` with this receiver.
    pub(super) fn this_calls(&mut self, t: Tok, origin: Tok, calls: &[CallId]) {
        if let TokKind::Func(f, _) = self.toks[t as usize] {
            if !self.funcs[f as usize].lexical_this && self.summarized(f) {
                for &call in calls {
                    self.this_into(call, t, Some(origin));
                }
            }
        }
    }

    /// The argument half of `call_tok(call, t, Some(_))` (builtin namespace
    /// values excepted: they run per receiver, `BuiltinCallee`).
    pub(super) fn call_args(&mut self, call: CallId, t: Tok) {
        self.diagnostic_callee(call, t);
        match self.toks[t as usize] {
            TokKind::BuiltinFn(Native::Member { .. }) => {}
            TokKind::Top | TokKind::BuiltinFn(_) => {
                if self.bind_done.insert((call, t, u32::MAX - 2)) {
                    self.unknown_callee_args(call, t, false);
                }
            }
            TokKind::Func(_, _) => self.bind_args(call, t),
            TokKind::Bound(_) => {
                if self.bind_done.insert((call, t, u32::MAX)) {
                    self.call_bound(call, t);
                }
            }
            // A bare marker under a key of this unit's object (`o.k()` with
            // `k` holding a parameter's own marker, DECISIONS 119): both
            // halves at once. `call_tok` records the call for the caller's
            // unit with the receiver described as its `this` (DECISIONS
            // 140), or escapes marker, receiver and arguments as a call of
            // an unknown function value, as whole does. (`this_to` has no
            // arm for a marker.)
            _ if self.bare_marker(t) => self.call_tok(call, t, None),
            _ => {}
        }
    }
}
