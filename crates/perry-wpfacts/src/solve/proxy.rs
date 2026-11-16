//! PROXY2: `new Proxy( t, h )` (ECMA-262 10.5.14 ProxyCreate) is a Proxy
//! exotic object: `TokKind::Proxy`, one per birth context of the call (as a
//! bound function, `builtins/collections.rs`). Its target and handler are
//! its own nodes, which every call that makes it feeds.
//!
//! The token reaches no rule but `proxy_apply` (the one hook in
//! `apply_tokens`). [[Get]] (10.5.8), [[Set]] (10.5.9), [[HasProperty]]
//! (10.5.7) and [[Delete]] (10.5.10) read their trap from `h` with the
//! ordinary [[Get]] (GetMethod 7.3.11: the named read of `h`, so a trap
//! stored later or a handler chosen at run time is in the read values),
//! call it with `this = h` and the spec arguments, and also forward the
//! operation to `t`: perry's runtime forwards when the trap is undefined,
//! null or not callable (proxy/get.rs: `is_callable(trap)` else forward).
//! The trap's result is the operation's result; the invariant checks
//! (10.5.8 step 9, 10.5.7 step 9) may throw a fresh TypeError instead, which
//! runs no program code and carries no program value (the arm of every
//! native TypeError). PROXY4: a test of the value itself (strict equality,
//! ToBoolean, `??`, typeof, ToObject) runs no internal method: the token is
//! an object there like any other. Every other operation on the token, and
//! an escape of it, takes the path the solver had before (`degrade`): the
//! target is hard E9 and goes to ⊤, the handler escapes, and the operation
//! sees ⊤. The hold's reason names that first operation.

use super::*;

/// The HIR form `new Proxy( t, h )` as a `Cons::NewNamed` call: a solver
/// spelling (`#`), never a program name.
pub(crate) const PROXY_NEW: &str = "#proxy-new";

#[derive(Clone, Copy, Debug)]
pub struct ProxyRec {
    pub target: NodeId,
    pub handler: NodeId,
    pub degraded: bool,
}

impl Model {
    /// `new Proxy( t, h )` at `call`: the Proxy object of the call's birth
    /// context, its parts fed by this call's arguments.
    pub(crate) fn proxy_new(&mut self, call: CallId) {
        // A summary body's own call: its instances make the Proxy (as
        // `Deferred::Construct`); the body's operands are its markers.
        if self.sum.templates.contains(&call) {
            return;
        }
        let c = self.calls[call as usize].clone();
        let key = self.birth_ctx(call);
        let p = match self.proxy_index.get(&key) {
            Some(&p) => p,
            None => {
                let target = self.node_l("proxy-target");
                let handler = self.node_l("proxy-handler");
                let p = self.proxies.len() as u32;
                self.proxies.push(ProxyRec {
                    target,
                    handler,
                    degraded: false,
                });
                self.proxy_index.insert(key, p);
                p
            }
        };
        let rec = self.proxies[p as usize];
        for (i, n) in [rec.target, rec.handler].into_iter().enumerate() {
            match c.arg(i) {
                Some(&a) => self.edge(a, n),
                // ProxyCreate step 1-2: a missing argument is `undefined`,
                // a TypeError: no object is made, nothing runs.
                None => return,
            }
        }
        for &s in &c.spread {
            let r = self.reason("new Proxy with spread arguments");
            self.sink_node(s, r);
        }
        let t = self.tok(TokKind::Proxy(p));
        self.add_tok(c.ret, t);
    }

    /// Today's path for Proxy `p` (before PROXY2): target hard E9 and ⊤,
    /// handler escapes. Once per Proxy.
    /// `cause` names the first operation that sent it here (the hold's
    /// reason reads "E9 Proxy target (<cause>)").
    pub(crate) fn proxy_degrade(&mut self, p: u32, cause: &str) {
        if self.proxies[p as usize].degraded {
            return;
        }
        self.proxies[p as usize].degraded = true;
        let ProxyRec {
            target, handler, ..
        } = self.proxies[p as usize];
        let r = self.reason(&format!("E9 Proxy target ({cause})"));
        self.cons(
            target,
            Cons::Hard {
                kind: Hard::E9Proxy,
                reason: r,
            },
        );
        self.cons(target, Cons::TopSink { reason: r });
        let rh = self.reason("E9 Proxy handler");
        self.sink_node(handler, rh);
    }

    /// The key argument a trap receives: ToPropertyKey's result (a String
    /// or a Symbol). Named: that string (or the well-known symbol); else
    /// the key value itself when the operation has one, or any string.
    fn proxy_key_arg(&mut self, key: Key, idx: NodeId) -> NodeId {
        if idx != NO_NODE {
            return idx;
        }
        let k = self.node_l("proxy-key");
        match key {
            Key::Named(a) if self.atoms[a as usize].starts_with(builtins::WK_PREFIX) => {
                self.add_prim(k, prim::SYMBOL);
            }
            // The String and its one entry arrive together, as at a string
            // literal (`Gen::str_literal`): a String added without an entry
            // is any string (`Model::add_s`), which no later join narrows.
            Key::Named(a) => {
                let s = self.str_set(&[a]);
                self.add_s(k, &[], prim::STR, s);
            }
            Key::Any(_) => self.add_prim(k, prim::STR | prim::SYMBOL),
        }
        k
    }

    /// Call the trap `name` of Proxy `p` with `this = handler` and `args`,
    /// its result into `ret`.
    fn proxy_trap(&mut self, p: u32, name: &str, args: Vec<NodeId>, ret: NodeId) {
        let h = self.proxies[p as usize].handler;
        let trap = self.node_l("proxy-trap");
        let a = self.atom(name);
        self.cons(
            h,
            Cons::Load {
                key: Key::Named(a),
                dst: trap,
            },
        );
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            first_arg_fixed: true,
            this: ThisArg::Node(h),
            args,
            spread: vec![],
            ret,
            site: 0,
            region: self.thrown,
        });
        self.why_mint(call, None, u32::MAX);
        self.cons(trap, Cons::Call { call });
    }

    fn proxy_recv(&mut self, p: u32) -> NodeId {
        let r = self.node_l("proxy-recv");
        let t = self.tok(TokKind::Proxy(p));
        self.add_tok(r, t);
        r
    }

    /// [[Get]] (10.5.8): trap `get( t, key, receiver )`, and the forward.
    fn proxy_get(&mut self, p: u32, key: Key, idx: NodeId, dst: NodeId) {
        let t = self.proxies[p as usize].target;
        let k = self.proxy_key_arg(key, idx);
        let recv = self.proxy_recv(p);
        self.proxy_trap(p, "get", vec![t, k, recv], dst);
        if idx == NO_NODE {
            self.cons(t, Cons::Load { key, dst });
        } else {
            self.cons(
                t,
                Cons::IdxLoad {
                    idx,
                    dst,
                    shape: 0,
                    enumerable: false,
                    kept: false,
                },
            );
        }
    }

    /// The one rule that sees a Proxy token: operation `c` on Proxy `p`.
    pub(crate) fn proxy_apply(&mut self, n: NodeId, c: &Cons, p: u32) {
        match *c {
            Cons::Load { key, dst } => self.proxy_get(p, key, NO_NODE, dst),
            Cons::IdxLoad { idx, dst, .. } => self.proxy_get(p, Key::any(), idx, dst),
            // `p.m( ... )` / `p[k]( ... )`: [[Get]], then Call( f, p, args ).
            Cons::MCall { key, call } => {
                let f = self.node_l("proxy-method");
                self.proxy_get(p, key, NO_NODE, f);
                let t = self.tok(TokKind::Proxy(p));
                self.cons(f, Cons::CallThis { call, this: t });
            }
            Cons::IdxMCall { idx, call, .. } => {
                let f = self.node_l("proxy-method");
                self.proxy_get(p, Key::any(), idx, f);
                let t = self.tok(TokKind::Proxy(p));
                self.cons(f, Cons::CallThis { call, this: t });
            }
            // [[Set]] (10.5.9): trap `set( t, key, V, receiver )`.
            Cons::Store { key, src, site } => {
                let t = self.proxies[p as usize].target;
                let k = self.proxy_key_arg(key, NO_NODE);
                let recv = self.proxy_recv(p);
                let ret = self.node_l("proxy-set-ret");
                self.proxy_trap(p, "set", vec![t, k, src, recv], ret);
                self.cons(t, Cons::Store { key, src, site });
            }
            // [[HasProperty]] (10.5.7): trap `has( t, key )`.
            Cons::HasProp { key, idx } => {
                let t = self.proxies[p as usize].target;
                let k = self.proxy_key_arg(key, idx);
                let ret = self.node_l("proxy-has-ret");
                self.proxy_trap(p, "has", vec![t, k], ret);
                self.cons(t, Cons::HasProp { key, idx });
            }
            // [[Delete]] (10.5.10): trap `deleteProperty( t, key )`.
            Cons::DeleteKey { key, reason } => {
                let t = self.proxies[p as usize].target;
                let k = self.proxy_key_arg(Key::Named(key), NO_NODE);
                let ret = self.node_l("proxy-delete-ret");
                self.proxy_trap(p, "deleteProperty", vec![t, k], ret);
                self.cons(t, Cons::DeleteKey { key, reason });
            }
            // An observation; a throw region: the trap calls run in the
            // shared cell every catch reads (`proxy_trap`).
            Cons::Identity { .. } | Cons::Raise { .. } => {}
            // A test of the value itself: `=== null`, `=== lit`, `!== null`
            // (IsStrictlyEqual 7.2.16, no internal method), ToBoolean (7.1.2:
            // an object is truthy), the `??` and `&&` operands, typeof
            // (13.5.3: "object" or "function", never a primitive type) and
            // ToObject of an object (7.1.18). None runs a trap: the Proxy is
            // an object token there like any other, and the ordinary rule
            // decides.
            Cons::Guard { .. }
            | Cons::Defined { .. }
            | Cons::NotNullish { .. }
            | Cons::NotUndefined { .. }
            | Cons::NotNull { .. }
            | Cons::TypeIs { .. }
            | Cons::Falsy { .. }
            | Cons::Objects { .. } => {
                let t = self.tok(TokKind::Proxy(p));
                self.apply_inner(n, c, Tokens::Slice(&[t]), 0);
            }
            _ => {
                self.proxy_degrade(p, c.kind_name());
                self.apply_inner(n, c, Tokens::Slice(&[TOP]), 0);
            }
        }
    }
}
