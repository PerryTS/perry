//! A call through a builtin function value that names its native
//! (`Native::Member`): on a receiver of the member's family it runs the
//! model a call of the member by name on that receiver runs
//! (`builtins::own_method`, `builtins::prim_method`); on any other receiver,
//! or with none, it is the call of an unknown function value it was before.

use crate::builtins;
use crate::model::*;
use crate::solve::Tokens;

impl Model {
    /// A callee that runs per (call, receiver) where a method lookup bound
    /// the call (`BuiltinCallee`), not through `this_to` / `call_args`: a
    /// builtin namespace value, a named native.
    pub(super) fn per_receiver_callee(&self, t: Tok) -> bool {
        matches!(
            self.toks[t as usize],
            TokKind::Builtin(_) | TokKind::BuiltinFn(Native::Member { .. })
        )
    }

    /// `call` into named native `t` with receiver `this_tok` (else the
    /// call's own `this`). `false`: not a named native, or a receiver its
    /// model does not cover: the caller runs the unknown callee's arm.
    pub(super) fn native_call(&mut self, call: CallId, t: Tok, this_tok: Option<Tok>) -> bool {
        let TokKind::BuiltinFn(Native::Member { fam, what, key }) = self.toks[t as usize] else {
            return false;
        };
        let r = match (this_tok, &self.calls[call as usize].this) {
            (Some(r), _) | (None, &ThisArg::Tok(r)) => r,
            (None, &ThisArg::Node(n)) => {
                // Each value of the receiver node runs on its own.
                if self.bind_done.insert((call, t, u32::MAX - 5)) {
                    self.cons(n, Cons::NativeThis { call, f: t });
                }
                return true;
            }
            (None, ThisArg::Undef) => return false,
        };
        if !builtins::native_receiver(self, fam, what, r) {
            return false;
        }
        if self.bind_done.insert((call, t, r)) {
            self.diagnostic_callee(call, t);
            self.with_b(|m| builtins::own_method(m, r, Key::Named(key), call));
        }
        true
    }

    /// `Cons::NativeThis`: the receiver values of `call` into named native
    /// `f`. A primitive of the member's family runs the primitive's method;
    /// `undefined` / `null` throw (no intrinsic prototype method accepts
    /// them as `this`); any other primitive is an unknown callee's call.
    pub(super) fn native_this(&mut self, call: CallId, f: Tok, toks: Tokens<'_>, prim: u16) {
        for &r in toks {
            self.call_tok(call, f, Some(r));
        }
        let TokKind::BuiltinFn(Native::Member { fam, key, .. }) = self.toks[f as usize] else {
            return;
        };
        let own = prim & builtins::native_prims(fam);
        if own != 0 {
            self.with_b(|m| builtins::prim_method(m, own, Key::Named(key), call));
        }
        if prim & !own & !(prim::UNDEF | prim::NULL) != 0
            && self.bind_done.insert((call, f, u32::MAX - 2))
        {
            self.unknown_callee_args(call, f, false);
        }
    }
}
