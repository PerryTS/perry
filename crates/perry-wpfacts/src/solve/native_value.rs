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
        let TokKind::BuiltinFn(Native::Member { identity }) = self.toks[t as usize] else {
            return false;
        };
        if builtins::native_namespace_call(self, identity, t, call) {
            return true;
        }
        let Some((fam, what, name)) = builtins::native_model(identity) else {
            return false;
        };
        let key = self.atom(&name.replace("@@", builtins::WK_PREFIX));
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
            self.with_b(|m| builtins::native_prototype_call(m, fam, r, key, call));
        }
        true
    }

    /// `Cons::ProtoKey` for receiver `t` (`E.prototype.key`): builtin
    /// namespace `F` reads its intrinsic prototype, whose own method `key`
    /// is final (DEC 250: a store there is refused), so the read is that
    /// native (`builtins::proto_own_native`); any other value, or a key
    /// the prototype does not hold as a final method, is the two reads.
    pub(super) fn proto_key(&mut self, t: Tok, key: Atom, dst: NodeId) {
        if let TokKind::Builtin(ns) = self.toks[t as usize] {
            if let Some(n) = builtins::proto_own_native(self, ns, key, dst) {
                let f = self.tok(TokKind::BuiltinFn(n));
                self.add_tok(dst, f);
                return;
            }
        }
        let r = self.proto_key_relay(key, dst);
        let pa = self.atom("prototype");
        self.load_from(t, t, Key::Named(pa), r);
    }

    /// The prototypes node of `Cons::ProtoKey` into `dst`: it loads `key`
    /// into `dst` (one per `dst`, in `dst`'s frame).
    pub(super) fn proto_key_relay(&mut self, key: Atom, dst: NodeId) -> NodeId {
        if let Some(&r) = self.proto_key_relays.get(&dst) {
            return r;
        }
        let r = self.node_l("protokey-protos");
        self.nodes[r as usize].frame = self.nodes[dst as usize].frame;
        self.nodes[r as usize].obs = self.nodes[dst as usize].obs;
        self.proto_key_relays.insert(dst, r);
        self.cons(
            r,
            Cons::Load {
                key: Key::Named(key),
                dst,
            },
        );
        r
    }

    /// `Cons::NativeThis`: the receiver values of `call` into named native
    /// `f`. A primitive of the member's family runs the primitive's method;
    /// `undefined` / `null` throw (no intrinsic prototype method accepts
    /// them as `this`); any other primitive is an unknown callee's call.
    pub(super) fn native_this(&mut self, call: CallId, f: Tok, toks: Tokens<'_>, prim: u16) {
        for &r in toks {
            self.call_tok(call, f, Some(r));
        }
        let TokKind::BuiltinFn(Native::Member { identity }) = self.toks[f as usize] else {
            return;
        };
        let Some((fam, _, name)) = builtins::native_model(identity) else {
            return;
        };
        let key = self.atom(&name.replace("@@", builtins::WK_PREFIX));
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
