//! A call through a builtin function value that names its native
//! (`Native::Member`): on a receiver of the member's family it runs the
//! model a call of the member by name on that receiver runs
//! (`builtins::own_method`, `builtins::prim_method`); on any other receiver,
//! or with none, it is the call of an unknown function value it was before.

use crate::builtins;
use crate::builtins::Family;
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
        // A module member whose effects the runtime declares at the body
        // its dispatch arm calls: the declaration runs, whatever `this`.
        if let Some(e) = builtins::declared_member(identity) {
            if self.bind_done.insert((call, t, u32::MAX - 7)) {
                self.diagnostic_callee(call, t);
                self.with_b(|m| builtins::run_native(m, e, t, call));
            }
            return true;
        }
        if builtins::native_namespace_call(self, t, call, this_tok) {
            return true;
        }
        // A static of a realm constructor whose effects the runtime
        // declares without naming `this` (`AbortSignal.any`): the
        // declaration runs, whatever `this`.
        if let Some(e) = builtins::declared_static(identity) {
            if self.bind_done.insert((call, t, u32::MAX - 7)) {
                self.diagnostic_callee(call, t);
                self.with_b(|m| builtins::run_function(m, e, call));
            }
            return true;
        }

        let Some((fam, what, name)) = builtins::native_model(self, t) else {
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
            (None, ThisArg::Undef) => return self.declared_proto_call(call, t, identity),
        };
        if !builtins::native_receiver(self, fam, what, r) {
            if fam == Family::Str && r != TOP {
                return self.string_this(call, t, r, key);
            }
            return self.declared_proto_call(call, t, identity);
        }
        if self.bind_done.insert((call, t, r)) {
            self.diagnostic_callee(call, t);
            self.with_b(|m| builtins::native_prototype_call(m, fam, r, key, call));
        }
        true
    }

    /// A prototype method called on a receiver its family model does not
    /// cover (`Performance.prototype.now` on `performance`, a host object
    /// with no family), when the runtime declares what a call does without
    /// naming `this`: the declaration runs, whatever the receiver (one of
    /// another brand throws before any of it: the declaration is a
    /// superset). `false`: no such declaration, the unknown callee's arm.
    fn declared_proto_call(&mut self, call: CallId, t: Tok, identity: &str) -> bool {
        let Some(e) = builtins::declared_proto_method(identity) else {
            return false;
        };
        if self.bind_done.insert((call, t, u32::MAX - 7)) {
            self.diagnostic_callee(call, t);
            self.with_b(|m| builtins::run_function(m, e, call));
        }
        true
    }

    /// A `String.prototype` method called with a receiver `r` of another
    /// kind (`String.prototype.replace.call(o, ...)`): the method starts with
    /// `ToString(RequireObjectCoercible(this))` (ES 22.1.3), which runs `r`'s
    /// own ToPrimitive and nothing else on it; the rest is the method on that
    /// string. `r` is not handed to unknown code. (`toString` and `valueOf`
    /// throw on such an `r` instead: the model is a superset of that.)
    fn string_this(&mut self, call: CallId, t: Tok, r: Tok, key: Atom) -> bool {
        if self.bind_done.insert((call, t, r)) {
            self.diagnostic_callee(call, t);
            let why = self.reason("ToString(this) of a String.prototype method");
            self.implicit(r, why, Protocol::ToPrimitive);
            self.with_b(|m| builtins::prim_method(m, prim::STR, Key::Named(key), call));
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
    /// them as `this`).
    ///
    /// Another primitive on a primitive family's method runs no program
    /// code: a `String.prototype` method starts with `ToString(this)` (ES
    /// 22.1.3), which for a number, boolean or bigint is the realm's own
    /// conversion, so the method runs on a string (`string_this` minus the
    /// ToPrimitive; `toString` / `valueOf` throw instead: a superset); on a
    /// symbol it throws. A `Number`, `Boolean`, `Symbol` or `BigInt` method
    /// starts with `thisNumberValue(this)` (and the like), which throws on a
    /// foreign primitive: no callee, nothing escapes. On any other family's
    /// method (generic `ToObject(this)`) the primitive is an unknown
    /// callee's call.
    pub(super) fn native_this(&mut self, call: CallId, f: Tok, toks: Tokens<'_>, prim: u16) {
        for &r in toks {
            self.call_tok(call, f, Some(r));
        }
        let TokKind::BuiltinFn(Native::Member { .. }) = self.toks[f as usize] else {
            return;
        };
        let Some((fam, _, name)) = builtins::native_model(self, f) else {
            return;
        };
        let key = self.atom(&name.replace("@@", builtins::WK_PREFIX));
        let own = prim & builtins::native_prims(fam);
        if own != 0 {
            self.with_b(|m| builtins::prim_method(m, own, Key::Named(key), call));
        }
        let foreign = prim & !own & !(prim::UNDEF | prim::NULL);
        if foreign == 0 {
            return;
        }
        if builtins::native_prims(fam) == 0 {
            if self.bind_done.insert((call, f, u32::MAX - 2)) {
                self.unknown_callee_args(call, f, false);
            }
        } else if fam == Family::Str
            && foreign & (prim::NUM | prim::BOOL | prim::BIGINT) != 0
            && self.bind_done.insert((call, f, u32::MAX - 6))
        {
            self.diagnostic_callee(call, f);
            self.with_b(|m| builtins::prim_method(m, prim::STR, Key::Named(key), call));
        }
    }
}
