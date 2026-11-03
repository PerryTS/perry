//! DECISIONS 252: a work budget per subject. A subject over its budget gives
//! up: its nodes hold ⊤, what they hold or receive escapes, their rules are
//! retired; no other subject loses a fact. The budget is a constant
//! (`Budget::DEFAULT`), not a knob: a test that needs a give-up sets a small
//! one on its `FactsConfig` or `Model`.

use super::*;

impl Model {
    /// The charge of `s` (functions by index, shared tokens by token, the thrown node).
    pub fn charge_mut(&mut self, s: Subject) -> &mut Charge {
        if s == SUBJECT_THROWN {
            return &mut self.thrown_charge;
        }
        let (v, i) = if s & SUBJECT_TOK != 0 {
            (&mut self.tok_charge, (s & !SUBJECT_TOK) as usize)
        } else {
            (&mut self.fn_charge, s as usize)
        };
        if v.len() <= i {
            v.resize(i + 1, Charge::default());
        }
        &mut v[i]
    }

    pub fn charge(&self, s: Subject) -> Option<&Charge> {
        if s == SUBJECT_THROWN {
            return Some(&self.thrown_charge);
        }
        if s & SUBJECT_TOK != 0 {
            self.tok_charge.get((s & !SUBJECT_TOK) as usize)
        } else {
            self.fn_charge.get(s as usize)
        }
    }

    /// The subject a token's own nodes (its fields, prototype, closures) belong
    /// to: the body that births it (a module namespace: the module's `<init>`
    /// body), or the shared token itself.
    pub fn tok_subject(&self, t: Tok) -> Subject {
        match self.toks[t as usize] {
            TokKind::Obj(tm, _) | TokKind::Proto(tm) | TokKind::ClassObj(tm) => {
                self.tmpl_fn.get(&tm).copied().unwrap_or(SUBJECT_TOK | t)
            }
            TokKind::Func(f, _) => f,
            TokKind::ModNs(mi) => self
                .mod_fn
                .get(mi as usize)
                .copied()
                .filter(|&f| f != NO_FN)
                .unwrap_or(SUBJECT_TOK | t),
            _ => SUBJECT_TOK | t,
        }
    }

    /// The subject of a call site: the body its result (or first argument)
    /// node was born in.
    pub fn call_subject(&self, call: CallId) -> Subject {
        let c = &self.calls[call as usize];
        let n = if c.ret != NO_NODE {
            c.ret
        } else if let Some(&a) = c.args.first() {
            a
        } else {
            return self.cur_subject;
        };
        self.node_subject[n as usize]
    }

    pub fn subject_name(&self, s: Subject) -> String {
        if s == SUBJECT_THROWN {
            return "thrown".into();
        }
        if s & SUBJECT_TOK != 0 {
            return self.tok_name(s & !SUBJECT_TOK);
        }
        self.funcs
            .get(s as usize)
            .map_or_else(|| format!("fn#{s}"), |f| f.name.clone())
    }

    /// Charge the work and points-to entries since the last mark to `x`.
    fn settle_charge(&mut self, x: Subject) {
        let (w0, p0) = self.mark;
        let (dw, dp) = (self.work - w0, self.pts_entries - p0);
        self.mark = (self.work, self.pts_entries);
        if dw == 0 && dp == 0 {
            return;
        }
        let c = self.charge_mut(x);
        c.work += dw;
        c.pts += dp;
    }

    /// Charge the work and points-to entries since the last mark to the
    /// current subject; from now on charge `s`. Gives up the previous subject
    /// when it is over.
    pub fn subject_mark(&mut self, s: Subject) {
        let prev = self.cur_subject;
        self.settle_charge(prev);
        self.cur_subject = s;
        let budget = self.budget.0;
        let kind = match self.charge(prev) {
            Some(c) if c.gave_up => None,
            Some(c) if c.work > budget => Some("work"),
            Some(c) if c.pts > budget * 2 => Some("pts"),
            _ => None,
        };
        if let Some(kind) = kind {
            self.give_up(prev, kind);
        }
    }

    /// The current subject is over its work budget (checked inside long pops).
    pub fn over_work(&self) -> bool {
        self.charge(self.cur_subject)
            .is_some_and(|c| !c.gave_up && c.work + (self.work - self.mark.0) > self.budget.0)
    }

    /// `s` gives up: unseen code from now on (DECISIONS 252 §3).
    pub fn give_up(&mut self, s: Subject, kind: &'static str) {
        if self.charge(s).is_some_and(|c| c.gave_up) {
            return;
        }
        // What accrued since the last mark is the current subject's; the
        // retiring below is `s`'s own last work (one escape per held token),
        // whichever subject was current.
        let cur = self.cur_subject;
        self.settle_charge(cur);
        let name = self.subject_name(s);
        let budget = self.budget.0;
        let r = self.reason(&format!("gave-up-budget: {name}"));
        let c = self.charge_mut(s);
        c.gave_up = true;
        c.reason = r;
        let w = c.work;
        eprintln!("perry: wpfacts gave-up {name} work={w} budget={budget} kind={kind}");
        self.gave_up.push((name, w, kind));
        self.cur_subject = s;
        for n in 0..self.nodes.len() as NodeId {
            if self.node_subject[n as usize] == s && !self.nodes[n as usize].retired {
                self.retire(n, r);
            }
        }
        self.settle_charge(s);
        self.cur_subject = cur;
    }

    /// `n` is unseen code's: its rules are retired, every token it holds
    /// escapes, it holds ⊤ (delivered along its edges by its next pop), and
    /// from now on what arrives escapes (`add_tokens`).
    pub fn retire(&mut self, n: NodeId, reason: u32) {
        let node = &mut self.nodes[n as usize];
        node.cons.clear();
        node.delta.clear();
        node.dprim = 0;
        self.sunk_nodes.insert(n);
        let mut toks = self.bufs.pop().unwrap_or_default();
        toks.clear();
        self.nodes[n as usize].pts.sorted_into(&mut toks);
        let c = Cons::TopSink { reason };
        self.nodes[n as usize].cons.push(c);
        let saved = (self.cur_src, self.cur_why);
        (self.cur_src, self.cur_why) = (n, reason);
        self.work += toks.len() as u64 + 1;
        self.apply(n, &c, &toks, self.nodes[n as usize].prim);
        self.bufs.push(toks);
        self.cur_src = NO_NODE;
        self.add(n, &[TOP], prim::ALL);
        (self.cur_src, self.cur_why) = saved;
        // Last: the ⊤ above went in as a token, not as an arrival to escape.
        self.nodes[n as usize].retired = true;
    }

    /// A token's own node with a static label (`node_l`).
    pub fn tok_node_l(&mut self, t: Tok, label: &'static str) -> NodeId {
        self.tok_node_named(t, label, || label.to_string())
    }

    /// A token's own node, charged to the token's subject.
    pub fn tok_node_named(
        &mut self,
        t: Tok,
        label: &'static str,
        name: impl FnOnce() -> String,
    ) -> NodeId {
        let prev = self.cur_subject;
        let s = self.tok_subject(t);
        self.subject_mark(s);
        let n = self.node_named(label, name);
        self.subject_mark(prev);
        n
    }
}

/// The settle stall guard (DESIGN §2): a settle pass that reports more grew
/// a done-set, so a pop or work follows before the next pass can. One that
/// reports more with `(pops, work)` unchanged since the last pass that did
/// recorded nothing: a stall, which no budget bounds. `last` is that pair at
/// the last pass that reported more, `now` the pair before this pass.
pub(crate) fn settle_stall(last: Option<(u64, u64)>, now: (u64, u64), more: bool) -> bool {
    more && last == Some(now)
}
