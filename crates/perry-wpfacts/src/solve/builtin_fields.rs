//! Program-owned fields on runtime objects use the ordinary field graph.
//! Native token overlap is read in one direction; field storage is never
//! merged merely because the anonymous native may denote two distinct values.

use super::*;

impl Model {
    /// A non-callable namespace's program-owned property (rule R1).
    pub(crate) fn builtin_field(&mut self, t: Tok, key: Key) -> Option<NodeId> {
        let Key::Named(a) = key else { return None };
        if builtins::builtin_function(self, t) || !builtins::undeclared(self, t, a) {
            return None;
        }
        let n = self.field(t, a);
        if self.r1_holders.insert(t) && self.interp_runs {
            self.builtin_fields_sink(t, None);
        }
        Some(n)
    }

    /// Join the fields of each token that may denote this native. `visits`
    /// keeps the ordinary read/call dependencies for native tokens minted
    /// later; every edge ends at the use, never another token's storage.
    pub(crate) fn builtin_field_use(&mut self, t: Tok, key: Key, act: ChainAct) {
        if !builtins::builtin_function(self, t) {
            return;
        }
        if !self.visits.entry(t).or_default().insert((t, key, act)) {
            return;
        }
        let holders: Vec<_> = (0..self.toks.len() as Tok)
            .filter(|&h| builtins::native_overlap(self, t, h))
            .collect();
        for h in holders {
            self.native_field_at(t, h, key, act);
        }
    }

    fn native_field_at(&mut self, origin: Tok, holder: Tok, key: Key, act: ChainAct) {
        match act {
            ChainAct::Load(dst) => {
                let n = if self.is_elem_key(key) {
                    self.num_fields(holder)
                } else {
                    match key {
                        Key::Named(a) => self.field(holder, a),
                        Key::Any(_) => self.key_fields(holder, key),
                    }
                };
                self.edge(n, dst);
            }
            ChainAct::Call(call) => {
                let n = if self.is_elem_key(key) {
                    self.num_fields(holder)
                } else {
                    match key {
                        Key::Named(a) => self.field(holder, a),
                        Key::Any(_) => self.key_fields(holder, key),
                    }
                };
                self.bind_via(n, call, origin);
            }
            ChainAct::Store(_) => {}
        }
        self.accessor_walk(origin, holder, key, act);
    }

    /// A newly minted native joins the reads already registered on tokens
    /// whose denotations overlap it. Reuse the ordinary holder visits rather
    /// than recording a separate native-field registry.
    pub(crate) fn builtin_token_created(&mut self, t: Tok) {
        if !builtins::builtin_function(self, t) {
            if self.interp_runs && matches!(self.toks[t as usize], TokKind::Builtin(_)) {
                self.builtin_fields_sink(t, None);
            }
            return;
        }
        let uses: Vec<_> = self
            .visits
            .iter()
            .filter_map(|(&holder, uses)| {
                builtins::native_overlap(self, holder, t).then_some((holder, uses))
            })
            .flat_map(|(holder, uses)| {
                uses.iter()
                    .copied()
                    .filter(move |&(origin, _, _)| origin == holder)
            })
            .collect();
        for (origin, key, act) in uses {
            self.native_field_at(origin, t, key, act);
        }
        if self.interp_runs {
            let reason = self.reason("built-in function object interpreted code may read");
            self.escape(t, reason);
        }
    }

    /// Non-callable runtime objects retain the namespace escape rule.
    /// Callable objects enter `escape_inner`, just like program objects.
    pub(crate) fn builtin_fields_sink(&mut self, t: Tok, reason: Option<u32>) {
        if self.unit_mode || t == TOP {
            return;
        }
        if builtins::builtin_function(self, t) {
            let r = reason.unwrap_or_else(|| {
                self.reason("built-in function object interpreted code may read")
            });
            self.escape(t, r);
            return;
        }
        let holder = match self.toks[t as usize] {
            TokKind::Builtin(BuiltinNs::GlobalThis) => return,
            TokKind::Builtin(_) => t,
            _ => return,
        };
        self.r1_sinks[0] += 1;
        if self.r1_holders.contains(&holder) {
            self.r1_sinks[1] += 1;
        }
        // Unknown code can install an own member even when the intrinsic
        // prototype refuses additions. Ordinary reads replay that write.
        builtins::note_native_write(self, holder, None, "built-in object given to unknown code");
        let r = reason
            .unwrap_or_else(|| self.reason("field of a built-in object interpreted code may read"));
        let all = self.all_fields(holder);
        self.sink_node(all, r);
    }

    /// Interpreted code can obtain native functions by their realm paths.
    pub(crate) fn builtin_fields_interp(&mut self) {
        let mut hs: Vec<Tok> = self.r1_holders.iter().copied().collect();
        hs.extend((0..self.toks.len() as Tok).filter(|&t| {
            matches!(self.toks[t as usize], TokKind::Builtin(_))
                || builtins::builtin_function(self, t)
        }));
        hs.sort_unstable();
        hs.dedup();
        for h in hs {
            self.builtin_fields_sink(h, None);
        }
    }
}
