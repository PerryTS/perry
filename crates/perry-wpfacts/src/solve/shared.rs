//! The runtime's own objects every unit shares (decision 151): their tokens,
//! what a unit records of its writes into them, and the replay of the
//! other units' writes.

use super::*;

impl Model {
    /// The shared object (decision 151) token `t` is, if any.
    pub(crate) fn shared_of(&self, t: Tok) -> Option<Shared> {
        if t == TOP {
            return Some(Shared::Top);
        }
        match &self.toks[t as usize] {
            TokKind::ObjectProto => Some(Shared::ObjectProto),
            TokKind::Builtin(ns) => Some(Shared::Builtin(*ns)),
            TokKind::BuiltinFn => Some(Shared::BuiltinFn),
            TokKind::NativeProto(n) => Some(Shared::NativeProto(n.clone())),
            _ => None,
        }
    }

    /// The token of shared object `s` (`shared_of` inverted).
    pub(crate) fn shared_tok(&mut self, s: &Shared) -> Tok {
        match s {
            Shared::Top => TOP,
            Shared::ObjectProto => self.tok(TokKind::ObjectProto),
            Shared::Builtin(ns) => self.tok(TokKind::Builtin(*ns)),
            Shared::BuiltinFn => self.tok(TokKind::BuiltinFn),
            Shared::NativeProto(n) => self.tok(TokKind::NativeProto(n.clone())),
        }
    }

    /// The shared object a value contract names for token `t`
    /// (`ValueContract::shared`): every one but ⊤ and the global object,
    /// which escaped puts ⊤ in reach of stores into ⊤ (`global_in_top`).
    pub(crate) fn named_shared(&self, t: Tok) -> Option<Shared> {
        self.shared_of(t)
            .filter(|s| !matches!(s, Shared::Top | Shared::Builtin(BuiltinNs::GlobalThis)))
    }

    pub(crate) fn note_shared(&mut self, w: SharedWrite) {
        self.top_write(&w);
        if !self.shared_replay {
            self.shared_out.insert(w);
        }
    }

    /// DECISIONS 231(c): a write through ⊤ other than a store (which
    /// `store_into` notes) may give a key of an escaped instance any value
    /// (`top_store`): an accessor under its key, a hard operation or
    /// unmodeled code under every key.
    fn top_write(&mut self, w: &SharedWrite) {
        match w {
            SharedWrite::Accessor(Shared::Top, k) if k != crate::STAR_KEY => {
                let a = self.atom(k);
                self.top_store(Some(a));
            }
            SharedWrite::Accessor(Shared::Top, _)
            | SharedWrite::Unmodeled(Shared::Top)
            | SharedWrite::Hard(Shared::Top, _) => self.top_store(None),
            _ => {}
        }
    }

    /// Another unit's write of a shared object, as if this unit's code did
    /// it with a value it does not know: a store of ⊤, an accessor of a
    /// function this unit cannot see (on `Object.prototype` E23, as for any
    /// accessor there), an unmodeled reach. The value and the function are the other
    /// unit's: only the write's target and key are this unit's to see.
    pub(crate) fn replay_shared(&mut self, w: &SharedWrite) {
        self.shared_replay = true;
        self.top_write(w);
        match w {
            SharedWrite::Store(s, k) => {
                let t = self.shared_tok(s);
                let key = match k {
                    Some(k) if k == NUM_KEY => self.elem_key(),
                    Some(k) => Key::Named(self.atom(k)),
                    None => Key::Any,
                };
                let v = self.node_l("shared-store");
                self.add_top(v, |_| "stored by another unit into a shared object".into());
                self.add_prim(v, prim::ALL);
                let site = self.anon_post_site("another unit's store into a shared object");
                let saved = self.store_at.take();
                self.store_into(t, key, v, site);
                self.store_at = saved;
            }
            SharedWrite::Accessor(s, k) => {
                let t = self.shared_tok(s);
                let key = self.atom(k);
                self.note_fnproto_write(t, Some(Key::Named(key)), "another unit's accessor");
                builtins::note_native_write(
                    self,
                    t,
                    Some(Key::Named(key)),
                    "another unit's accessor",
                );
                self.reshape(Some(key));
                match s {
                    Shared::Top => {}
                    // E23: every lookup that reaches it may run the accessor.
                    // Under a computed key it is what the installing unit
                    // (and the whole program) makes of it: an escaped
                    // accessor whose holder reads anything (below).
                    Shared::ObjectProto if k != crate::STAR_KEY => {
                        if self.explain && !self.objproto_accessor {
                            eprintln!(
                                "perry: wpfacts explain: E23 another unit's accessor on Object.prototype key {k}"
                            );
                        }
                        self.objproto_accessor = true
                    }
                    // A lookup of the key there runs that unit's function as
                    // getter or setter (`builtins::other_unit_accessor`).
                    _ => {
                        let f = builtins::other_unit_accessor(self);
                        let holder = self.node_l("shared-accessor-holder");
                        self.add_tok(holder, t);
                        for setter in [false, true] {
                            self.cons(holder, Cons::AccessorInstall { key, f, setter });
                        }
                    }
                }
            }
            SharedWrite::Unmodeled(s) => {
                let t = self.shared_tok(s);
                let what = "another unit's object reaches unmodeled code";
                if matches!(s, Shared::ObjectProto) {
                    self.note_fnproto_write(t, None, what);
                }
                if matches!(s, Shared::ObjectProto | Shared::NativeProto(_)) {
                    builtins::note_native_write(self, t, None, what);
                }
                if matches!(s, Shared::Builtin(BuiltinNs::GlobalThis)) {
                    builtins::note_global_in_top(self);
                }
            }
            // Its effect is the stores and accessors the driver derives
            // (`units::shared_closure`).
            SharedWrite::Handed(_) => {}
            // DECISIONS 234: another unit's defineProperty through ⊤ under a
            // computed key (⊤ may be a builtin prototype); its delete,
            // defineProperty or `with` through ⊤ or on a parameter: a key of
            // an escaped object may be gone.
            SharedWrite::Hard(Shared::Top, Hard::E5DefineProperty) => self.reshape(None),
            SharedWrite::Removed(k) => {
                let a = k.as_ref().map(|k| self.atom(k));
                self.top_reshape(a)
            }
            SharedWrite::Hard(s, kind) => {
                let t = self.shared_tok(s);
                let r = self.reason(&format!("{} by another unit", kind.code()));
                self.mark_hard(t, *kind, r);
            }
        }
        self.shared_replay = false;
    }
}
