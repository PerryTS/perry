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
//! an object there like any other. PROXY5: ToPrimitive (and so
//! ToPropertyKey) reads and calls `@@toPrimitive`, `valueOf` and `toString`
//! through [[Get]] with `this` = the Proxy (`proxy_to_primitive`). PROXY7:
//! the runtime's formatter showing the Proxy runs no trap
//! (`proxy_inspect_shown`). Every other operation on the token, and
//! an escape of it, takes the path the solver had before (`degrade`): the
//! target is hard E9 and goes to ⊤, the handler escapes, and the operation
//! sees ⊤. The hold's reason names that first operation.

use super::*;

/// The HIR form `new Proxy( t, h )` as a `Cons::NewNamed` call: a solver
/// spelling (`#`), never a program name.
pub(crate) const PROXY_NEW: &str = "#proxy-new";

#[derive(Clone, Debug)]
pub struct ProxyRec {
    pub target: NodeId,
    pub handler: NodeId,
    pub degraded: bool,
    /// The Proxy's operations, one entry per class (`ProxyOp`): what the
    /// first use made (its trap call, its forward to the target), which
    /// every later use of the class joins.
    pub ops: crate::pts::FxHashMap<ProxyOp, NodeId>,
}

/// An operation class of one Proxy. Each is made once: its trap call and
/// its forward rule on the target exist once per class, and the forward of
/// a target that holds the Proxy (a Proxy of itself, a deep proxy) finds
/// the class it is part of instead of making it again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProxyOp {
    /// [[Get]] of `key` (with the key node of a computed read): its
    /// result node, which the trap returns into and the target read feeds.
    Get(Key, NodeId),
    /// The `set` trap call for `key`: the node of the values it is passed.
    SetTrap(Key),
    /// The forward of one store (`key`, value node, site) to the target.
    SetFwd(Key, NodeId, SiteId),
    /// [[HasProperty]] of `key` (with its key node): trap and forward.
    Has(Key, NodeId),
    /// [[GetOwnProperty]] of `key` (with its key node): trap and forward.
    OwnProp(Key, NodeId),
    /// The `deleteProperty` trap call for `key`.
    DelTrap(Atom),
    /// The forward of one delete (`key`, its reason) to the target.
    DelFwd(Atom, u32),
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
                    ops: Default::default(),
                });
                self.proxy_index.insert(key, p);
                p
            }
        };
        let rec = &self.proxies[p as usize];
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
        let (target, handler) = (
            self.proxies[p as usize].target,
            self.proxies[p as usize].handler,
        );
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

    /// Whether Proxy `p` has made the class `op`, and enter it (`NO_NODE`
    /// for a class without a node of its own).
    fn proxy_made(&mut self, p: u32, op: ProxyOp) -> bool {
        self.proxies[p as usize].ops.insert(op, NO_NODE).is_some()
    }

    /// [[Get]] (10.5.8): trap `get( t, key, receiver )`, and the forward.
    /// The result node of the class (`ProxyOp::Get`): made once, with one
    /// trap call returning into it and one forward read of the target into
    /// it. The class is entered before the forward runs: a target holding
    /// `p` reads into the same node.
    fn proxy_get(&mut self, p: u32, key: Key, idx: NodeId) -> NodeId {
        let op = ProxyOp::Get(key, idx);
        if let Some(&g) = self.proxies[p as usize].ops.get(&op) {
            return g;
        }
        let g = self.node_l("proxy-get");
        self.proxies[p as usize].ops.insert(op, g);
        let t = self.proxies[p as usize].target;
        let k = self.proxy_key_arg(key, idx);
        let recv = self.proxy_recv(p);
        self.proxy_trap(p, "get", vec![t, k, recv], g);
        if idx == NO_NODE {
            self.cons(t, Cons::Load { key, dst: g });
        } else {
            self.cons(
                t,
                Cons::IdxLoad {
                    idx,
                    dst: g,
                    shape: 0,
                    enumerable: false,
                    kept: false,
                },
            );
        }
        g
    }

    /// A [[Get]] of Proxy `p` whose result goes to `dst`.
    fn proxy_get_into(&mut self, p: u32, key: Key, idx: NodeId, dst: NodeId) {
        let g = self.proxy_get(p, key, idx);
        if g != dst {
            self.edge(g, dst);
        }
    }

    /// ToPrimitive (7.1.1) of Proxy `p`, and through it ToString, ToNumber,
    /// ToNumeric and ToPropertyKey (7.1.19: hint String). `@@toPrimitive`
    /// (GetMethod 7.3.11), then OrdinaryToPrimitive's (7.1.1.1) `valueOf`
    /// and `toString`, each a [[Get]] of the Proxy (`proxy_get`: the get
    /// trap and the forward to the target). A callable result is called with
    /// `this` = the Proxy and the hint; a non-callable one is not called (a
    /// TypeError, or the next method) and runs nothing. What the calls
    /// return goes back to the runtime (an object result: TypeError, 7.1.1
    /// step 1.b.vi, 7.1.1.1 step 6), as for any object (`implicit`). A
    /// superset of the one order each hint takes. Once per Proxy.
    pub(crate) fn proxy_to_primitive(&mut self, p: u32, reason: u32) {
        let t = self.tok(TokKind::Proxy(p));
        if !self.implicit_done.insert((t, Protocol::ToPrimitive)) {
            return;
        }
        let ret = self.node_l("implicit-ret");
        self.to_prim_ret.insert(t, ret);
        self.to_prim_ret_nodes.insert(ret);
        self.nodes[ret as usize].sink = true;
        let arg = self.node_l("implicit-arg");
        self.add_prim(arg, prim::ALL);
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            first_arg_fixed: false,
            this: ThisArg::Tok(t),
            args: vec![arg; 4],
            spread: vec![],
            ret,
            site: 0,
            region: self.thrown,
        });
        self.protocol_calls.insert(call);
        self.why_mint(call, Some(t), reason);
        for k in Protocol::ToPrimitive.keys().unwrap_or(&[]) {
            let a = self.atom(k);
            let f = self.proxy_get(p, Key::Named(a), NO_NODE);
            self.cons(f, Cons::Call { call });
        }
    }

    /// The runtime's formatter showing Proxy `p` (`InspectShown`: a console
    /// argument or a value it shows; perry-runtime builtins/formatting.rs
    /// `format_proxy_value`, Node lib/internal/util/inspect.js `formatValue`).
    /// The Proxy is unwrapped first (`getProxyDetails`): no internal method
    /// of the Proxy runs, so no trap. The `[util.inspect.custom]` hook is an
    /// ordinary [[Get]] of the TARGET, called with `this` = the Proxy and
    /// primitive arguments; what it returns is shown in turn. Without one the
    /// target's own values are shown (`Protocol::InspectTarget`). Once per
    /// Proxy.
    pub(crate) fn proxy_inspect_shown(&mut self, p: u32, reason: u32) {
        let t = self.tok(TokKind::Proxy(p));
        if !self.implicit_done.insert((t, Protocol::InspectShown)) {
            return;
        }
        let target = self.proxies[p as usize].target;
        let ret = self.node_l("implicit-ret");
        self.nodes[ret as usize].sink = true;
        let arg = self.node_l("implicit-arg");
        self.add_prim(arg, prim::ALL);
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            first_arg_fixed: false,
            this: ThisArg::Tok(t),
            args: vec![arg; 4],
            spread: vec![],
            ret,
            site: 0,
            region: self.thrown,
        });
        self.protocol_calls.insert(call);
        self.why_mint(call, Some(t), reason);
        let hook = self.node_l("proxy-method");
        let a = self.atom(crate::model::INSPECT_CUSTOM_METHOD);
        self.cons(
            target,
            Cons::Load {
                key: Key::Named(a),
                dst: hook,
            },
        );
        self.cons(hook, Cons::Call { call });
        self.cons(
            ret,
            Cons::Implicit {
                reason,
                proto: Protocol::InspectShown,
            },
        );
        self.cons(
            target,
            Cons::Implicit {
                reason,
                proto: Protocol::InspectTarget,
            },
        );
    }

    /// The one rule that sees a Proxy token: operation `c` on Proxy `p`.
    pub(crate) fn proxy_apply(&mut self, n: NodeId, c: &Cons, p: u32) {
        match *c {
            Cons::Load { key, dst } => self.proxy_get_into(p, key, NO_NODE, dst),
            Cons::IdxLoad { idx, dst, .. } => self.proxy_get_into(p, Key::any(), idx, dst),
            // `p.m( ... )` / `p[k]( ... )`: [[Get]], then Call( f, p, args ).
            Cons::MCall { key, call } => {
                let f = self.proxy_get(p, key, NO_NODE);
                let t = self.tok(TokKind::Proxy(p));
                self.cons(f, Cons::CallThis { call, this: t });
            }
            Cons::IdxMCall { idx, call, .. } => {
                let f = self.proxy_get(p, Key::any(), idx);
                let t = self.tok(TokKind::Proxy(p));
                self.cons(f, Cons::CallThis { call, this: t });
            }
            // [[Set]] (10.5.9): trap `set( t, key, V, receiver )`.
            // One trap call per key, passed every stored value; the forward
            // once per store.
            Cons::Store { key, src, site } => {
                let t = self.proxies[p as usize].target;
                let v = match self.proxies[p as usize].ops.get(&ProxyOp::SetTrap(key)) {
                    Some(&v) => v,
                    None => {
                        let v = self.node_l("proxy-set-value");
                        self.proxies[p as usize].ops.insert(ProxyOp::SetTrap(key), v);
                        let k = self.proxy_key_arg(key, NO_NODE);
                        let recv = self.proxy_recv(p);
                        let ret = self.node_l("proxy-set-ret");
                        self.proxy_trap(p, "set", vec![t, k, v, recv], ret);
                        v
                    }
                };
                self.edge(src, v);
                if !self.proxy_made(p, ProxyOp::SetFwd(key, src, site)) {
                    self.cons(t, Cons::Store { key, src, site });
                }
            }
            // [[HasProperty]] (10.5.7): trap `has( t, key )`.
            Cons::HasProp { key, idx } => {
                if self.proxy_made(p, ProxyOp::Has(key, idx)) {
                    return;
                }
                let t = self.proxies[p as usize].target;
                let k = self.proxy_key_arg(key, idx);
                let ret = self.node_l("proxy-has-ret");
                self.proxy_trap(p, "has", vec![t, k], ret);
                self.cons(t, Cons::HasProp { key, idx });
            }
            // [[GetOwnProperty]] (10.5.5): trap `getOwnPropertyDescriptor( t,
            // key )`. ToPropertyDescriptor reads the returned object's fields
            // by HasProperty / Get (6.2.6.5): program code on a value this
            // model does not follow, so the result is sunk.
            Cons::OwnProp { key, idx } => {
                if self.proxy_made(p, ProxyOp::OwnProp(key, idx)) {
                    return;
                }
                let t = self.proxies[p as usize].target;
                let k = self.proxy_key_arg(key, idx);
                let ret = self.node_l("proxy-gopd-ret");
                let r = self.reason("Proxy getOwnPropertyDescriptor trap result");
                self.sink_node(ret, r);
                self.proxy_trap(p, "getOwnPropertyDescriptor", vec![t, k], ret);
                self.cons(t, Cons::OwnProp { key, idx });
            }
            // [[Delete]] (10.5.10): trap `deleteProperty( t, key )`.
            Cons::DeleteKey { key, reason } => {
                let t = self.proxies[p as usize].target;
                if !self.proxy_made(p, ProxyOp::DelTrap(key)) {
                    let k = self.proxy_key_arg(Key::Named(key), NO_NODE);
                    let ret = self.node_l("proxy-delete-ret");
                    self.proxy_trap(p, "deleteProperty", vec![t, k], ret);
                }
                if !self.proxy_made(p, ProxyOp::DelFwd(key, reason)) {
                    self.cons(t, Cons::DeleteKey { key, reason });
                }
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
            | Cons::Objects { .. }
            // The interval of an arithmetic or string operand (`range`):
            // an object operand's is ⊤; its ToPrimitive is its own rule
            // (`Cons::Implicit`, below).
            | Cons::Range { .. }
            // The type of a `+` or arithmetic result: an object operand
            // gives any primitive its ToPrimitive may return, as for any
            // object; ToPrimitive itself is `Cons::Implicit`.
            | Cons::Plus { .. }
            | Cons::Arith { .. }
            // Rule CONSOLE-PCT: specifiers are parsed only in a String
            // first argument (`may_hold_percent` reads the node's strings);
            // a Proxy is an object, never one, and runs no trap there.
            | Cons::ConsoleFormat { .. }
            // The KEY of a computed store `o[proxy] = v` (the rule sits on
            // the key node): ToPropertyKey (7.1.19) is ToPrimitive with hint
            // String, its own rule (`Cons::Implicit` on every computed key,
            // `proxy_to_primitive`: the [[Get]]s of `@@toPrimitive`,
            // `valueOf`, `toString`). The store then uses that primitive; the
            // ordinary rule reads no internal method of an object key (the
            // any-key store, or the ToPrimitive result key: `implicit`).
            | Cons::StoreIndexed { .. } => {
                let t = self.tok(TokKind::Proxy(p));
                self.apply_inner(n, c, Tokens::Slice(&[t]), 0);
            }
            // ToPrimitive (7.1.1), and ToPropertyKey's (7.1.19, hint String).
            Cons::Implicit {
                reason,
                proto: Protocol::ToPrimitive,
            } => self.proxy_to_primitive(p, reason),
            // The formatter showing the Proxy (`proxy_inspect_shown`).
            Cons::Implicit {
                reason,
                proto: Protocol::InspectShown,
            } => self.proxy_inspect_shown(p, reason),
            _ => {
                self.proxy_degrade(p, c.kind_name());
                self.apply_inner(n, c, Tokens::Slice(&[TOP]), 0);
            }
        }
    }
}
