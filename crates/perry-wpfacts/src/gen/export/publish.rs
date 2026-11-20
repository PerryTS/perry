//! Publishing to other units: classes given out, function results and the contract-check on a result token.

use super::*;

impl<'a> Gen<'a> {
    /// A class of this unit becomes a contract (§11.2): published, its
    /// instance's fields leave. `key` is its export; none for a class whose
    /// instance a contract result or value gives out (DECISIONS 116).
    pub(super) fn publish_contract_class(
        &mut self,
        key: Option<(u32, Atom)>,
        n: NodeId,
        tm: TmplId,
        extended: bool,
        r: u32,
    ) -> bool {
        // No constructor code runs on the receiver but the field
        // initializers, which store the class's own keys and do
        // not leak `this` (DECISIONS 58).
        let t = &self.m.tmpls[tm as usize];
        let no_code = t.ctor.is_none()
            && t.parent.is_none()
            && t.parent_value.is_none()
            && t.native_parent.is_none()
            && self.ctor_phase.get(&tm).is_some_and(|ph| !ph.leaks);
        let statics = self
            .m
            .publish_class(tm, r, extended, no_code, key.is_none());
        for (a, sn) in statics {
            if !self.static_values.contains(&(tm, a, sn)) {
                self.static_values.push((tm, a, sn));
            }
        }
        self.class_contracts.push((key, n, tm));
        let inst = self.m.tok(TokKind::Obj(tm, NO_CTX));
        self.fields_leave(inst, r)
    }

    /// DECISIONS 116: a class of this unit that no other unit names, whose
    /// instance a contract result or value gives out (AWFY's
    /// `exports.newInstance = () => new Richards()`): the reader calls its
    /// methods on what it gets, as on an exported class's instance, so it
    /// is the same contract, published under its display. No other unit
    /// can name it, so none extends it. A census report breaks it as any.
    pub(super) fn class_to_give(&self, tm: TmplId) -> bool {
        let t = &self.m.tmpls[tm as usize];
        matches!(
            t.kind,
            TmplKind::Class {
                literal_shape: false,
                ..
            }
        ) && !self.stand_in_cc.contains_key(&tm)
            && self.unit_in.is_some_and(|u| !u.broken(&t.display))
    }

    /// Gives `t` (an instance of contract class `tm`, or of a class to
    /// give, published here first) out to other units. True when this
    /// added constraints.
    pub(super) fn give_class(&mut self, tm: TmplId, t: Tok, r: u32) -> bool {
        let mut changed = false;
        if !self.contract_class(tm) {
            self.publish_contract_class(None, u32::MAX, tm, false, r);
            changed = true;
        }
        changed | self.m.give_out(tm, t)
    }

    /// DECISIONS 241: function token `t` is held by other units by name:
    /// another unit's (a stand-in) or a contract function of this one.
    pub(crate) fn fn_published(&self, t: Tok) -> bool {
        match self.m.toks[t as usize] {
            TokKind::Func(f, _) => {
                self.stand_in_fc.contains_key(&t)
                    || self.m.contract_fns.contains(&f)
                        && self.m.funcs[f as usize].func_id != u32::MAX
            }
            _ => false,
        }
    }

    /// After a solve: a published class export holds its class value only
    /// (a module may rebind the name); otherwise it escapes after all. The
    /// primitives beside it are the contract's `binding_prim` (DECISIONS
    /// 115): no object, so the class is what every reader may find. True
    /// when something changed (solve again).
    /// A value contract function `f` hands the caller's unit (its result,
    /// or an argument of a call through its parameter, DECISIONS 140), node
    /// `n`: `None` if no contract describes what it holds, else whether
    /// publishing it changed anything (its fields leave, classes go out).
    pub(super) fn publish_result(&mut self, f: FnIdx, n: NodeId, r: u32) -> Option<bool> {
        let mut changed = false;
        // A marker of no family of this run (the result's own marker,
        // standing for what crossed into it, MRK) is another call's value: a
        // parameter of `f` it stands for was this run's argument in some
        // other call, not this one's. It says no more than ⊤ only when
        // everything it stands for reads as ⊤.
        let frame = self.m.nodes[n as usize].frame;
        let held = self.m.nodes[n as usize].pts.to_sorted();
        let foreign = held.iter().any(|&t| {
            self.m.is_arg_marker(t)
                && (self.m.fam_home(t) == 0 || self.m.fam_home(t) != frame)
                && !self
                    .m
                    .marker_members(t)
                    .iter()
                    .all(|&x| self.m.marker_is_top(x))
        });
        if foreign {
            return None;
        }
        // An argument marker reads as the markers it stands for (DECISIONS 44).
        let toks: Vec<Tok> = held
            .into_iter()
            .flat_map(|t| self.m.marker_members(t))
            .collect();
        // Another function's marker with nothing beside it (DECISIONS
        // 119) is another unit's value the caller cannot follow: it
        // escapes, then reads as ⊤ (DECISIONS 42).
        for &t in &toks {
            if self.m.param_marker.get(&t).is_some_and(|&(g, _)| g != f)
                && !self.m.beside_values(t)
                && !self.m.marker_is_top(t)
            {
                self.m.escape(t, r);
                changed = true;
            }
        }
        // A value no contract describes escapes alone (DECISIONS 174, 175):
        // the result still describes the others, and its contract says
        // unknown beside them (`describe_nodes`). Nothing described at all
        // and the result is unknown (`ret_top`).
        let (toks, bad): (Vec<Tok>, Vec<Tok>) =
            toks.into_iter().partition(|&t| self.result_tok_ok(f, t));
        if !bad.is_empty() && toks.is_empty() {
            return None;
        }
        for t in bad {
            changed |= self.m.escape_changed(t, r);
        }
        for t in toks {
            if matches!(
                self.m.toks[t as usize],
                TokKind::Top | TokKind::BuiltinFn(_) | TokKind::Builtin(_)
            ) || self.m.param_marker.contains_key(&t)
            {
                continue;
            }
            if self.arr_ok(t) {
                changed |= self.arr_leaves(t);
                continue;
            }
            // DECISIONS 147: the caller's unit calls a function it is
            // handed through its contract (§11.2, as a published binding's,
            // `check_value`): its parameters carry markers, its result is
            // checked; a closed one leaves through its argument contract.
            // Another unit's function leaves with its owner's contract.
            if let TokKind::Func(f, _) = self.m.toks[t as usize] {
                if self.m.funcs[f as usize].func_id != u32::MAX {
                    let new = self.m.contract_fns.insert(f);
                    changed |= new;
                    let rf = match self.m.closed_params.contains_key(&f) {
                        true => self.m.closed_pub_reason.unwrap_or(r),
                        false => r,
                    };
                    // One that escaped already, as no contract function,
                    // gets its markers now (as a published field function,
                    // `note_field_fns`).
                    match self.m.escaped.get(&t).copied() {
                        Some(r0) if new => self.m.escape_fn_params(t, r0),
                        _ => self.m.escape(t, rf),
                    }
                }
                continue;
            }
            if self.stand_in_fc.contains_key(&t) {
                continue;
            }
            if let TokKind::Obj(tm, _) = self.m.toks[t as usize] {
                if self.contract_class(tm) || self.class_to_give(tm) {
                    changed |= self.give_class(tm, t, r);
                } else if self.stand_in_cc.contains_key(&tm) {
                    // Another unit's layout, forwarded with its owner's
                    // contract: nothing of this unit leaves through its
                    // fields, as for a published value (DECISIONS 79).
                    continue;
                } else if !self.ret_ok.contains(&t) {
                    // A literal result is the same layout contract as a
                    // published literal: its field functions are contract
                    // functions the caller calls through their contract
                    // (DECISIONS 78, as the value path below).
                    changed |= self.note_field_fns(t);
                }
            }
            changed |= self.fields_leave(t, r);
        }
        Some(changed)
    }

    /// One value a contract function `f` hands the caller's unit
    /// (`publish_result`): whether a contract describes it.
    pub(crate) fn result_tok_ok(&self, f: FnIdx, t: Tok) -> bool {
        match self.m.toks[t as usize] {
            _ if self.m.param_marker.get(&t).is_some_and(|&(g, _)| g == f) => true,
            // An escaped marker reads as ⊤ (DECISIONS 42), of any
            // contract function: another unit's value.
            _ if self.m.marker_is_top(t) => true,
            // An unknown value or a builtin function in the result is
            // the caller's ⊤, nothing of this unit leaves through it.
            TokKind::Top | TokKind::BuiltinFn(_) => true,
            // A builtin, a program object, an array (DECISIONS 94): one
            // runtime object every unit names alike, or an array whose
            // elements are a cell (`arr_leaves` below).
            TokKind::Builtin(_) => true,
            // DECISIONS 147: a function of this unit is a contract function
            // (below); another unit's leaves with its owner's contract, as
            // a stand-in instance does, unless it is closed: a closed
            // contract's users are known to its owner (`Model::escape`).
            TokKind::Func(f, _) if self.m.funcs[f as usize].func_id != u32::MAX => true,
            TokKind::Func(f, _) if self.stand_in_fc.contains_key(&t) => {
                self.m.funcs[f as usize].forwarded
            }
            TokKind::Obj(..) if self.arr_ok(t) => true,
            // An instance of a contract class: given out (`give_out`),
            // or of a class to give (DECISIONS 116).
            TokKind::Obj(tm, _) if self.contract_class(tm) || self.class_to_give(tm) => true,
            // Another unit's instance (a stand-in): forwarded with that
            // unit's contract (`fn_contract_of`), its proof (§17.10 4).
            TokKind::Obj(tm, _) if self.stand_in_cc.contains_key(&tm) => !self
                .unit_in
                .is_some_and(|u| u.broken(&self.m.tmpls[tm as usize].display)),
            TokKind::Obj(tm, _) => self.literal_ok(tm),
            _ => false,
        }
    }
}
