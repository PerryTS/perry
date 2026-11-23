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
//! (`proxy_inspect_shown`). PROXY11: SerializeJSONProperty (`JSON.stringify`,
//! the formatter's `%j`) runs the `toJSON` [[Get]], [[OwnPropertyKeys]],
//! [[GetOwnProperty]] and the member [[Get]]s (`proxy_json`); a console
//! argument runs that, ToPrimitive and the unwrapped show (`proxy_inspect`).
//! Every other operation on the token, and
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
    /// Rule PXCALL: the target may be callable, so the Proxy has a
    /// [[Call]] (`Model::proxy_callable`).
    pub callable: bool,
    /// Rule PXCALL: the calls of the Proxy (callee node, rule) made while
    /// its target held no callable value; they run when it gets one.
    pub calls: Vec<(NodeId, Cons)>,
}

/// An operation class of one Proxy. Each is made once: its trap call and
/// its forward rule on the target exist once per class, and the forward of
/// a target that holds the Proxy (a Proxy of itself, a deep proxy) finds
/// the class it is part of instead of making it again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProxyOp {
    /// [[Get]] of `key` (with the key node of a computed read) with
    /// Receiver `recv` (the Proxy, or the Proxy that forwarded to it): its
    /// result node, which the trap returns into and the target read feeds.
    Get(Key, NodeId, Tok),
    /// The `set` trap call for `key` with Receiver `recv`: the node of the
    /// values it is passed.
    SetTrap(Key, Tok),
    /// The throw region of the `set` trap call for (`key`, Receiver): what
    /// the trap throws, which reaches the region of every store that ran it.
    SetThrow(Key, Tok),
    /// The forward of one store (`key`, value node, site, Receiver) to the
    /// target.
    SetFwd(Key, NodeId, SiteId, Tok),
    /// Rule PXDEFINE: the `defineProperty` trap call for `key`: the value
    /// node of the descriptors it is passed.
    DefineTrap(Key),
    /// Rule PXDEFINE: its throw region.
    DefineThrow(Key),
    /// [[HasProperty]] of `key` (with its key node): trap and forward.
    Has(Key, NodeId),
    /// [[GetOwnProperty]] of `key` (with its key node): trap and forward.
    OwnProp(Key, NodeId),
    /// The `deleteProperty` trap call for `key`.
    DelTrap(Atom),
    /// The forward of one delete (`key`, its reason) to the target.
    DelFwd(Atom, u32),
    /// [[OwnPropertyKeys]] and [[GetOwnProperty]] of every key: the
    /// `ownKeys` / `getOwnPropertyDescriptor` traps and the target's own.
    OwnKeys,
    /// [[GetPrototypeOf]]: the `getPrototypeOf` trap (a for-in walk).
    ProtoTrap,
    /// Rule PXCALL: the `ProxyCallee` rule of the called Proxy (the id) on
    /// this Proxy's target: a Proxy is callable when its target is.
    Callee(u32),
    /// One `__lookupGetter__` / `__lookupSetter__` call on the Proxy
    /// (`proxy_annexb_lookup`): its traps and its forward.
    Lookup(CallId),
    /// Rule PXCHAIN: the receivers of the operations that reach the Proxy
    /// on a prototype chain (`Cons::ChainRecv`): its node.
    ChainRecv,
    /// Rule PXCHAIN: [[Get]] reaching the Proxy on a chain, the chain value
    /// node it feeds (`Cons::ChainUnion`): trap and forward.
    ChainGet(NodeId),
    /// Rule PXCHAIN: one per-receiver walk (`Cons::Chain`) reaching the
    /// Proxy: its traps (a lookup) and its forward.
    ChainWalk(Tok, Key, ChainAct),
    /// Rule PXCHAIN: an accessor holder walk (`Cons::HolderStep`, its pass
    /// node) reaching the Proxy: its forward.
    Holders(Key, NodeId),
    /// Rule PXCHAIN: a prototype closure (`Cons::ProtoClosure`, its node)
    /// holding the Proxy: its forward.
    Closure(NodeId),
    /// Rule PXCHAIN: the `set` trap call for `key` reached on a chain: the
    /// node of the values it is passed.
    ChainSetTrap(Key),
    /// Rule PXCHAIN: the throw node of that trap call.
    ChainSetThrow(Key),
    /// Rule PXCHAIN: one receiver's [[Set]] of `key` reaching the Proxy.
    ChainSet(Tok, Key),
    /// Rule PXCHAIN: a receiver's read or call under a computed key that
    /// may be an index (`Cons::PlainKey`) reaching the Proxy.
    Plain(Tok, ChainAct),
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
                    callable: false,
                    calls: Vec::new(),
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
    /// its result into `ret`, what it throws into the shared thrown cell.
    /// `key`: the access's key when 10.5.x passes the trap P (rule PXGET:
    /// the trap runs per key shape), `None` for a trap without one.
    fn proxy_trap(&mut self, p: u32, name: &str, key: Option<Key>, args: Vec<NodeId>, ret: NodeId) {
        let thrown = self.thrown;
        self.proxy_trap_in(p, name, key, args, ret, thrown);
    }

    /// `proxy_trap` with what the trap throws into `region`.
    fn proxy_trap_in(
        &mut self,
        p: u32,
        name: &str,
        key: Option<Key>,
        args: Vec<NodeId>,
        ret: NodeId,
        region: NodeId,
    ) {
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
            region,
        });
        if let Some(k) = key {
            let sh = match k {
                Key::Named(a) => a,
                Key::Any(_) => crate::summary::TRAP_ANY,
            };
            self.sum.trap_shape.insert(call, sh);
        }
        self.why_mint(call, None, u32::MAX);
        self.cons(trap, Cons::Call { call });
    }

    /// The `receiver` argument of a trap: the Receiver `recv` of the
    /// operation (10.5.8, 10.5.9).
    fn proxy_recv(&mut self, recv: Tok) -> NodeId {
        let r = self.node_l("proxy-recv");
        self.add_tok(r, recv);
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
        let recv = self.tok(TokKind::Proxy(p));
        self.proxy_get_r(p, key, idx, recv)
    }

    /// Rule PXRECV: p.[[Get]](P, Receiver) (10.5.8) with Receiver `recv`:
    /// the trap `get( t, key, recv )`, and the forward
    /// target.[[Get]](P, recv) (step 6), the same Receiver: the target's
    /// getters run with `this` = `recv` (`Cons::FwdLoad`), a Proxy target
    /// in turn with `recv`. One class per (key, key node, Receiver).
    fn proxy_get_r(&mut self, p: u32, key: Key, idx: NodeId, recv: Tok) -> NodeId {
        let op = ProxyOp::Get(key, idx, recv);
        if let Some(&g) = self.proxies[p as usize].ops.get(&op) {
            return g;
        }
        let g = self.node_l("proxy-get");
        self.proxies[p as usize].ops.insert(op, g);
        let t = self.proxies[p as usize].target;
        let k = self.proxy_key_arg(key, idx);
        let r = self.proxy_recv(recv);
        self.proxy_trap(p, "get", Some(key), vec![t, k, r], g);
        self.cons(
            t,
            Cons::FwdLoad {
                key,
                idx,
                dst: g,
                recv,
            },
        );
        g
    }

    /// Rule PXRECV: p.[[Set]](P, V, Receiver) (10.5.9) with Receiver
    /// `recv`: trap `set( t, key, V, recv )`, one call per (key, Receiver)
    /// passed every stored value. Step 7 is `? Call( trap, ... )`: what the
    /// trap throws completes this [[Set]], so it reaches the store's own
    /// region (the trap call's region node feeds every store's), never the
    /// program-wide thrown cell. The forward target.[[Set]](P, V, recv)
    /// (step 6) once per store: the target's setters run with `this` =
    /// `recv` (`Cons::FwdStore`), a Proxy target in turn with `recv`.
    fn proxy_set(
        &mut self,
        p: u32,
        key: Key,
        src: NodeId,
        site: SiteId,
        region: NodeId,
        recv: Tok,
    ) {
        let t = self.proxies[p as usize].target;
        let ops = &self.proxies[p as usize].ops;
        let made = ops
            .get(&ProxyOp::SetTrap(key, recv))
            .zip(ops.get(&ProxyOp::SetThrow(key, recv)));
        let (v, throw) = match made.map(|(&v, &throw)| (v, throw)) {
            Some(made) => made,
            None => {
                let v = self.node_l("proxy-set-value");
                let throw = self.node_l("proxy-set-throw");
                self.proxies[p as usize]
                    .ops
                    .insert(ProxyOp::SetTrap(key, recv), v);
                self.proxies[p as usize]
                    .ops
                    .insert(ProxyOp::SetThrow(key, recv), throw);
                let k = self.proxy_key_arg(key, NO_NODE);
                let r = self.proxy_recv(recv);
                let ret = self.node_l("proxy-set-ret");
                self.proxy_trap_in(p, "set", Some(key), vec![t, k, v, r], ret, throw);
                (v, throw)
            }
        };
        self.edge(src, v);
        self.edge(throw, region);
        if !self.proxy_made(p, ProxyOp::SetFwd(key, src, site, recv)) {
            self.cons(
                t,
                Cons::FwdStore {
                    key,
                    src,
                    site,
                    region,
                    recv,
                },
            );
        }
    }

    /// [[GetOwnProperty]] (10.5.5) of `key` (key node `idx`): trap
    /// `getOwnPropertyDescriptor( t, key )`, and the forward. The class
    /// (`ProxyOp::OwnProp`) is the caller's.
    fn proxy_own_prop(&mut self, p: u32, key: Key, idx: NodeId) {
        let t = self.proxies[p as usize].target;
        let k = self.proxy_key_arg(key, idx);
        let ret = self.node_l("proxy-gopd-ret");
        let r = self.reason("Proxy getOwnPropertyDescriptor trap result");
        self.sink_node(ret, r);
        self.proxy_trap(p, "getOwnPropertyDescriptor", Some(key), vec![t, k], ret);
        self.cons(t, Cons::OwnProp { key, idx });
    }

    /// Rule PXDEFINE: p.[[DefineOwnProperty]](key, desc) (10.5.6) with a
    /// data descriptor whose [[Value]] is `value`: trap `defineProperty( t,
    /// key, descObj )` (step 8: descObj = FromPropertyDescriptor(desc),
    /// `builtins::trap_descriptor`), one call per key passed every value,
    /// its throw into `region`; then (step 10) target.[[GetOwnProperty]](key)
    /// and, without a trap (step 7), target.[[DefineOwnProperty]](key, desc):
    /// the same rules on the target (`Cons::OwnProp`, `Cons::DefineOwn`).
    fn proxy_define(&mut self, p: u32, key: Key, value: NodeId, region: NodeId) {
        let ops = &self.proxies[p as usize].ops;
        let made = ops
            .get(&ProxyOp::DefineTrap(key))
            .zip(ops.get(&ProxyOp::DefineThrow(key)));
        let (v, throw) = match made.map(|(&v, &throw)| (v, throw)) {
            Some(made) => made,
            None => {
                let v = self.node_l("proxy-define-value");
                let throw = self.node_l("proxy-define-throw");
                self.proxies[p as usize]
                    .ops
                    .insert(ProxyOp::DefineTrap(key), v);
                self.proxies[p as usize]
                    .ops
                    .insert(ProxyOp::DefineThrow(key), throw);
                let t = self.proxies[p as usize].target;
                let k = self.proxy_key_arg(key, NO_NODE);
                let d = self.node_l("proxy-define-desc");
                let ret = self.node_l("proxy-define-ret");
                self.proxy_trap_in(p, "defineProperty", Some(key), vec![t, k, d], ret, throw);
                let call = (self.calls.len() - 1) as CallId;
                let dt = builtins::trap_descriptor(self, call, v);
                self.add_tok(d, dt);
                self.cons(t, Cons::OwnProp { key, idx: NO_NODE });
                self.cons(
                    t,
                    Cons::DefineOwn {
                        key,
                        value: v,
                        region: throw,
                    },
                );
                (v, throw)
            }
        };
        self.edge(value, v);
        self.edge(throw, region);
    }

    /// A [[Get]] of Proxy `p` whose result goes to `dst`.
    pub(crate) fn proxy_get_into(&mut self, p: u32, key: Key, idx: NodeId, dst: NodeId) {
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
    pub(crate) fn proxy_to_primitive(&mut self, p: u32, reason: u32, region: NodeId) {
        let t = self.tok(TokKind::Proxy(p));
        let Some(throw) = self.implicit_throw(t, Protocol::ToPrimitive, region) else {
            return;
        };
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
            region: throw,
        });
        self.protocol_calls.insert(call);
        self.why_mint(call, Some(t), reason);
        for k in Protocol::ToPrimitive.keys().unwrap_or(&[]) {
            let a = self.atom(k);
            let f = self.proxy_get(p, Key::Named(a), NO_NODE);
            self.cons(f, Cons::Call { call });
        }
    }

    /// SerializeJSONProperty (ECMA-262 25.5.2.2) of Proxy `p` (perry-runtime
    /// json/stringify_proxy.rs `try_stringify`), what `JSON.stringify` and the
    /// formatter's `%j` run on it. `toJSON` is a [[Get]] of the Proxy
    /// (`proxy_get`), a callable result called with `this` = the Proxy and the
    /// key, its result serialized in turn. IsCallable and IsArray read the
    /// target's kind and run nothing. An array's `length` is a [[Get]], then
    /// ToNumber. An object's EnumerableOwnProperties (7.3.23) runs
    /// [[OwnPropertyKeys]] (10.5.11: trap `ownKeys( t )`, its result read by
    /// CreateListFromArrayLike 7.3.19: `length` and the elements) and per
    /// string key [[GetOwnProperty]] (10.5.5: trap
    /// `getOwnPropertyDescriptor( t, key )`, its result read by
    /// ToPropertyDescriptor 6.2.6.5: has and get of its six fields); with no
    /// trap, and for the invariant checks, the same internal methods run on
    /// the target (`Implicit OwnKeys` on it: nothing on an ordinary object, a
    /// Proxy target's own rule). Each member is a [[Get]] of the Proxy (any
    /// key: the get trap and the forward), serialized in turn. Once per Proxy.
    pub(crate) fn proxy_json(&mut self, p: u32, reason: u32, region: NodeId) {
        let t = self.tok(TokKind::Proxy(p));
        let Some(throw) = self.implicit_throw(t, Protocol::Json, region) else {
            return;
        };
        // toJSON( key ) with this = the Proxy.
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
            region: throw,
        });
        self.protocol_calls.insert(call);
        self.why_mint(call, Some(t), reason);
        let a = self.atom("toJSON");
        let f = self.proxy_get(p, Key::Named(a), NO_NODE);
        self.cons(f, Cons::Call { call });
        self.cons(
            ret,
            Cons::Implicit {
                reason,
                proto: Protocol::Json,
                region: throw,
            },
        );
        // An array's `length`, ToNumber'd.
        let a = self.atom("length");
        let len = self.proxy_get(p, Key::Named(a), NO_NODE);
        self.cons(
            len,
            Cons::Implicit {
                reason,
                proto: Protocol::ToPrimitive,
                region: throw,
            },
        );
        self.proxy_own_props(p, reason);
        // Each member, serialized in turn.
        let m = self.proxy_get(p, Key::any(), NO_NODE);
        self.cons(
            m,
            Cons::Implicit {
                reason,
                proto: Protocol::Json,
                region: throw,
            },
        );
    }

    /// [[OwnPropertyKeys]] (10.5.11) and [[GetOwnProperty]] (10.5.5) of
    /// every key of Proxy `p`: the `ownKeys` trap with the list read off its
    /// result (CreateListFromArrayLike 7.3.18), the
    /// `getOwnPropertyDescriptor` trap with the descriptor read off its
    /// result (ToPropertyDescriptor 6.2.6.5), and, without a trap and for
    /// the invariant checks, the target's own (`Implicit OwnKeys` on it).
    /// Once per Proxy.
    fn proxy_own_props(&mut self, p: u32, reason: u32) {
        if self.proxy_made(p, ProxyOp::OwnKeys) {
            return;
        }
        let target = self.proxies[p as usize].target;
        // [[OwnPropertyKeys]]: the trap, the list read off its result.
        let keys = self.node_l("proxy-ownkeys-ret");
        self.proxy_trap(p, "ownKeys", None, vec![target], keys);
        self.proxy_result_reads(keys, &["length"], true, reason);
        // [[GetOwnProperty]] of each key: the trap, the descriptor read.
        let k = self.proxy_key_arg(Key::any(), NO_NODE);
        let desc = self.node_l("proxy-gopd-ret");
        self.proxy_trap(
            p,
            "getOwnPropertyDescriptor",
            Some(Key::any()),
            vec![target, k],
            desc,
        );
        self.proxy_result_reads(
            desc,
            &[
                "enumerable",
                "configurable",
                "value",
                "writable",
                "get",
                "set",
            ],
            false,
            reason,
        );
        // Without a trap, and for the invariant checks: the target's own.
        self.cons(
            target,
            Cons::Implicit {
                reason,
                proto: Protocol::OwnKeys,
                region: self.thrown,
            },
        );
    }

    /// The own keys of Proxy `p` (`Protocol::OwnKeys`: Object.keys,
    /// Object.getOwnPropertyNames / Symbols, Reflect.ownKeys, for-in):
    /// its [[OwnPropertyKeys]] and [[GetOwnProperty]] (`proxy_own_props`;
    /// EnumerableOwnProperties 7.3.23 reads both, Reflect.ownKeys the
    /// first), and for-in's walk (EnumerateObjectProperties 14.7.5.9:
    /// [[GetPrototypeOf]], 10.5.1, the `getPrototypeOf(t)` trap; without
    /// it the target's, inside its own `Implicit OwnKeys`), whose result's
    /// own keys are enumerated in turn. Nothing else runs.
    pub(crate) fn proxy_own_keys(&mut self, p: u32, reason: u32) {
        self.proxy_own_props(p, reason);
        if self.proxy_made(p, ProxyOp::ProtoTrap) {
            return;
        }
        let target = self.proxies[p as usize].target;
        let proto = self.node_l("proxy-getprototypeof-ret");
        self.proxy_trap(p, "getPrototypeOf", None, vec![target], proto);
        self.cons(
            proto,
            Cons::Implicit {
                reason,
                proto: Protocol::OwnKeys,
                region: self.thrown,
            },
        );
    }

    /// `__lookupGetter__` / `__lookupSetter__` (Annex B.2.2.4 / B.2.2.5) of
    /// `key` with `this` = Proxy `p`, the result into `ret`. The loop's
    /// O.[[GetOwnProperty]](key) is 10.5.5: the trap
    /// `getOwnPropertyDescriptor( t, key )`, whose result ToPropertyDescriptor
    /// reads (6.2.6.5) and whose `get` / `set` field is what the call returns
    /// for an accessor descriptor. An absent descriptor goes on with
    /// O.[[GetPrototypeOf]]() (10.5.1): the trap `getPrototypeOf( t )`; its
    /// result (an object or null, a TypeError otherwise) is the next O of the
    /// loop, the same lookup again (`Cons::AnnexBLookup`, which ends at null
    /// and runs a Proxy's traps in turn). Without a trap each forwards to the
    /// target: its own descriptor, then its prototype, which is the lookup on
    /// the target. Trap and forward both run, as for every Proxy operation.
    /// Once per call.
    pub(crate) fn proxy_annexb_lookup(
        &mut self,
        p: u32,
        key: Key,
        call: CallId,
        ret: NodeId,
        setter: bool,
    ) {
        if self.proxy_made(p, ProxyOp::Lookup(call)) {
            return;
        }
        let target = self.proxies[p as usize].target;
        let next = self.proxy_lookup_traps(p, key, ret, setter);
        self.cons(next, Cons::AnnexBLookup { call, setter });
        self.cons(target, Cons::AnnexBLookup { call, setter });
    }

    /// The Annex B loop's two steps at Proxy `p` (`proxy_annexb_lookup`):
    /// the `getOwnPropertyDescriptor` trap, whose `get` / `set` field goes to
    /// `ret`, and the `getPrototypeOf` trap; the node of the objects it
    /// returns (the loop's next O) is the result. The caller puts the next
    /// step on it and on the target (the forward of both traps).
    fn proxy_lookup_traps(&mut self, p: u32, key: Key, ret: NodeId, setter: bool) -> NodeId {
        let target = self.proxies[p as usize].target;
        let reason = self.reason("Annex B __lookupGetter__ / __lookupSetter__ on a Proxy");
        let k = self.proxy_key_arg(key, NO_NODE);
        let desc = self.node_l("proxy-gopd-ret");
        self.proxy_trap(
            p,
            "getOwnPropertyDescriptor",
            Some(key),
            vec![target, k],
            desc,
        );
        self.proxy_result_reads(
            desc,
            &[
                "enumerable",
                "configurable",
                "value",
                "writable",
                "get",
                "set",
            ],
            false,
            reason,
        );
        let field = self.atom(if setter { "set" } else { "get" });
        self.cons(
            desc,
            Cons::Load {
                key: Key::Named(field),
                dst: ret,
            },
        );
        let proto = self.node_l("proxy-getprototypeof-ret");
        self.proxy_trap(p, "getPrototypeOf", None, vec![target], proto);
        let next = self.node_l("proxy-lookup-next");
        self.cons(proto, Cons::Objects { dst: next });
        next
    }

    /// Rule PXCHAIN: the receivers of the chain operations that reach Proxy
    /// `p` (every object whose prototype closure holds `p`, `ChainRecv`):
    /// the `receiver` argument of the `get` and `set` traps run there.
    fn proxy_chain_recv(&mut self, p: u32) -> NodeId {
        if let Some(&r) = self.proxies[p as usize].ops.get(&ProxyOp::ChainRecv) {
            return r;
        }
        let r = self.node_l("proxy-chain-recv");
        self.proxies[p as usize].ops.insert(ProxyOp::ChainRecv, r);
        r
    }

    /// Rule PXCHAIN [[Get]]: OrdinaryGet (10.1.8.1 step 3) of `of` reaching
    /// Proxy `p` on its chain is p.[[Get]](key, Receiver) (10.5.8): the trap
    /// `get( t, key, receiver )` into the chain value `dst`, and the forward
    /// target.[[Get]](key, Receiver): the same chain union on the target
    /// (its own value, then its chain), as if the target were the next link.
    fn proxy_chain_get(&mut self, p: u32, key: Key, dst: NodeId, of: Tok, builtin: bool) {
        if self.proxy_made(p, ProxyOp::ChainGet(dst)) {
            return;
        }
        let t = self.proxies[p as usize].target;
        let k = self.proxy_key_arg(key, NO_NODE);
        let recv = self.proxy_chain_recv(p);
        self.proxy_trap(p, "get", Some(key), vec![t, k, recv], dst);
        self.cons(
            t,
            Cons::ChainUnion {
                key,
                dst,
                of,
                builtin,
            },
        );
    }

    /// Rule PXCHAIN: a per-receiver walk of `origin` (`Cons::Chain`)
    /// reaching Proxy `p`. A lookup (B.2.2.4/5) runs the
    /// `getOwnPropertyDescriptor` and `getPrototypeOf` traps
    /// (`proxy_lookup_traps`) and goes on at the objects the latter returns;
    /// every walk forwards to the target (10.5.5 / 10.5.1 / 10.5.8 / 10.5.9
    /// without a trap: the target's own, then its chain), whose accessors
    /// run with `this` = `origin` (the Receiver the forward passes).
    fn proxy_chain_walk(&mut self, p: u32, origin: Tok, key: Key, act: ChainAct) {
        if self.proxy_made(p, ProxyOp::ChainWalk(origin, key, act)) {
            return;
        }
        if let ChainAct::Lookup { dst, setter } = act {
            let next = self.proxy_lookup_traps(p, key, dst, setter);
            self.cons(next, Cons::Chain { origin, key, act });
        }
        let t = self.proxies[p as usize].target;
        self.cons(t, Cons::Chain { origin, key, act });
    }

    /// Rule PXCHAIN: the index part of a computed read or call of `origin`
    /// (`Cons::PlainKey`) reaching Proxy `p` on its chain: p.[[Get]](key,
    /// Receiver) (10.5.8): the `get` trap with any key (a String), its
    /// result read (`Load`) or called with `this` = `origin` (`Call`), and
    /// the forward: the same rule on the target (its index fields).
    fn proxy_chain_plain(&mut self, p: u32, origin: Tok, act: ChainAct) {
        if self.proxy_made(p, ProxyOp::Plain(origin, act)) {
            return;
        }
        let t = self.proxies[p as usize].target;
        let recv = self.proxy_chain_recv(p);
        self.add_tok(recv, origin);
        let key = Key::any();
        let k = self.proxy_key_arg(key, NO_NODE);
        match act {
            ChainAct::Load(dst, _) => self.proxy_trap(p, "get", Some(key), vec![t, k, recv], dst),
            ChainAct::Call(call) => {
                let g = self.node_l("proxy-chain-get");
                self.proxy_trap(p, "get", Some(key), vec![t, k, recv], g);
                self.bind_via(g, call, origin);
            }
            // `plain_key` makes neither.
            ChainAct::Store(..) | ChainAct::Lookup { .. } => {}
        }
        self.cons(t, Cons::PlainKey { origin, act });
    }

    /// Rule PXCHAIN [[Set]]: OrdinarySet (10.1.9.2 step 2.b) of `key` on
    /// `origin` reaching Proxy `p` on its chain is p.[[Set]](key, V,
    /// Receiver) (10.5.9): the trap `set( t, key, V, receiver )`, one call
    /// per key passed every value (`src`: the receiver's stored values) and
    /// throwing into `region` (the receiver's store regions). The forward
    /// target.[[Set]](key, V, Receiver) runs the setters of the target's
    /// chain with `this` = `origin` (the walk's forward,
    /// `proxy_chain_walk`), a Proxy there (it is in `origin`'s closure,
    /// `Closure`) in turn, and otherwise defines the key on the receiver:
    /// the receiver's own store.
    fn proxy_chain_set(&mut self, p: u32, origin: Tok, key: Key, src: NodeId, region: NodeId) {
        let recv = self.proxy_chain_recv(p);
        self.add_tok(recv, origin);
        if self.proxy_made(p, ProxyOp::ChainSet(origin, key)) {
            return;
        }
        let ops = &self.proxies[p as usize].ops;
        let made = ops
            .get(&ProxyOp::ChainSetTrap(key))
            .zip(ops.get(&ProxyOp::ChainSetThrow(key)));
        let (v, throw) = match made.map(|(&v, &throw)| (v, throw)) {
            Some(made) => made,
            None => {
                let v = self.node_l("proxy-chain-set-value");
                let throw = self.node_l("proxy-chain-set-throw");
                self.proxies[p as usize]
                    .ops
                    .insert(ProxyOp::ChainSetTrap(key), v);
                self.proxies[p as usize]
                    .ops
                    .insert(ProxyOp::ChainSetThrow(key), throw);
                let t = self.proxies[p as usize].target;
                let k = self.proxy_key_arg(key, NO_NODE);
                let ret = self.node_l("proxy-set-ret");
                self.proxy_trap_in(p, "set", Some(key), vec![t, k, v, recv], ret, throw);
                (v, throw)
            }
        };
        self.edge(src, v);
        self.edge(throw, region);
    }

    /// Rule PXCHAIN [[Set]]: the store of `src` under `key` into ordinary
    /// object `t` (no definition: [[DefineOwnProperty]] walks no chain) in
    /// `region`. Once per (`t`, `key`): `Cons::ChainSet` on `t`'s prototype
    /// closure, with one value node and one throw node; each store feeds
    /// them once a Proxy is met there (until then its sources wait, as a
    /// store walk's do).
    /// Rule PXRECV: `origin` is the Receiver (`t` itself, or the Proxy whose
    /// forward stores into `t`).
    pub(crate) fn chain_set_rule(
        &mut self,
        origin: Tok,
        t: Tok,
        key: Key,
        src: NodeId,
        region: NodeId,
    ) {
        if t == TOP {
            return;
        }
        let (v, throw) = match self.chain_set_nodes.get(&(origin, t, key)) {
            Some(&x) => x,
            None => {
                let v = self.node_l("chain-set-value");
                let throw = self.node_l("chain-set-throw");
                self.chain_set_nodes.insert((origin, t, key), (v, throw));
                self.chain_set_pending.insert(v, Vec::new());
                let cp = self.cp(t);
                self.cons(
                    cp,
                    Cons::ChainSet {
                        origin,
                        key,
                        src: v,
                        region: throw,
                    },
                );
                (v, throw)
            }
        };
        match self.chain_set_pending.get_mut(&v) {
            Some(w) => w.push((src, region)),
            None => {
                self.edge(src, v);
                self.edge(throw, region);
            }
        }
    }

    /// Rule PXCHAIN: `t` is a receiver whose chain operations may reach a
    /// Proxy (`Cons::ChainRecv` on its prototype closure, once).
    pub(crate) fn note_chain_recv(&mut self, t: Tok) {
        self.note_chain_recv_at(t, t);
    }

    /// Rule PXRECV: a chain operation of `holder` with Receiver `origin`
    /// (`holder` itself, or the Proxy whose forward reads `holder`): a
    /// Proxy on `holder`'s chain is passed `origin`.
    pub(crate) fn note_chain_recv_at(&mut self, holder: Tok, origin: Tok) {
        if holder == TOP || origin == TOP || !self.chain_recv_done.insert((holder, origin)) {
            return;
        }
        let cp = self.cp(holder);
        self.cons(cp, Cons::ChainRecv { origin });
    }

    /// What the runtime reads off a trap result `r`: HasProperty and [[Get]]
    /// of `keys` (getters run in the load), and with `elems` every element
    /// (CreateListFromArrayLike); a `length` read is ToNumber'd.
    fn proxy_result_reads(&mut self, r: NodeId, keys: &[&str], elems: bool, reason: u32) {
        for k in keys {
            let a = self.atom(k);
            let dst = self.node_l("proxy-result-read");
            self.nodes[dst as usize].sink = true;
            self.cons(
                r,
                Cons::HasProp {
                    key: Key::Named(a),
                    idx: NO_NODE,
                },
            );
            self.cons(
                r,
                Cons::Load {
                    key: Key::Named(a),
                    dst,
                },
            );
            if *k == "length" {
                self.cons(
                    dst,
                    Cons::Implicit {
                        reason,
                        proto: Protocol::ToPrimitive,
                        region: self.thrown,
                    },
                );
            }
        }
        if elems {
            let dst = self.node_l("proxy-result-read");
            self.nodes[dst as usize].sink = true;
            self.cons(
                r,
                Cons::Load {
                    key: Key::any(),
                    dst,
                },
            );
        }
    }

    /// The formatter on a console argument that is Proxy `p` (`Inspect`):
    /// `%j` serializes it (`proxy_json`), `%s` / `%d` / `%i` / `%f` run
    /// ToPrimitive (`proxy_to_primitive`), and the value is shown unwrapped
    /// (`proxy_inspect_shown`).
    pub(crate) fn proxy_inspect(&mut self, p: u32, at: InspectAt, reason: u32, region: NodeId) {
        self.proxy_json(p, reason, region);
        self.proxy_to_primitive(p, reason, region);
        self.proxy_inspect_shown(p, at, reason, region);
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
    /// Rule R2 (format_proxy_value, formatting.rs:940): no limit check; the
    /// hook and the target are at the Proxy's own depth `at`.
    pub(crate) fn proxy_inspect_shown(
        &mut self,
        p: u32,
        at: InspectAt,
        reason: u32,
        region: NodeId,
    ) {
        if at.depth > INSPECT_HARD_CAP {
            return;
        }
        let t = self.tok(TokKind::Proxy(p));
        let Some(throw) = self.implicit_throw(t, Protocol::InspectShown(at), region) else {
            return;
        };
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
            region: throw,
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
                proto: Protocol::InspectShown(at),
                region: throw,
            },
        );
        self.cons(
            target,
            Cons::Implicit {
                reason,
                proto: Protocol::InspectTarget(at),
                region: throw,
            },
        );
    }

    /// Rule PXCALL: the callee node `n` of a call or `new` (rule `c`, or
    /// one of the relays of a method call through a receiver, rule
    /// PXCALLR: `ThisTo`, `CallArgs`, `BuiltinCallee`) holds Proxy `p`.
    /// ProxyCreate (10.5.14 step 7) gives `p` a [[Call]] and [[Construct]]
    /// only when its target is callable (a constructor); otherwise the
    /// call is that of an ordinary object without [[Call]] (7.3.14 Call
    /// step 2, a TypeError): no trap runs, nothing flows. The decision is
    /// the target node's rule (`Cons::ProxyCallee`), re-made as its tokens
    /// grow; until a callable value reaches it, the call waits.
    fn proxy_call(&mut self, n: NodeId, c: Cons, p: u32) {
        if self.proxies[p as usize].callable {
            self.proxy_degrade(p, "call");
            return self.apply_inner(n, &c, Tokens::Slice(&[TOP]), 0);
        }
        // Array.prototype.toString (23.1.3.36 step 3) on a non-callable
        // `join`: the String of Object.prototype.toString, as for any
        // object without [[Call]] (`call_tok`).
        if let Cons::Call { call } | Cons::CallThis { call, .. } = c {
            if self.tostring_join_calls.contains(&call) {
                let ret = self.calls[call as usize].ret;
                self.add_prim(ret, prim::STR);
            }
        }
        self.proxies[p as usize].calls.push((n, c));
        self.proxy_callee_rule(p, p);
    }

    /// Rule PXCALL: the `ProxyCallee` rule of called Proxy `of` on the
    /// target of Proxy `p` (`of` itself, or a Proxy its target holds: a
    /// Proxy is callable when its target is). Once per pair.
    fn proxy_callee_rule(&mut self, of: u32, p: u32) {
        if !self.proxy_made(p, ProxyOp::Callee(of)) {
            let t = self.proxies[p as usize].target;
            self.cons(t, Cons::ProxyCallee { p: of });
        }
    }

    /// Rule PXCALL: Proxy `p`'s target may be callable. The Proxy's
    /// [[Call]] is the `apply` trap or the forward (10.5.12), its
    /// [[Construct]] the `construct` trap or the forward (10.5.13): not
    /// modeled, today's path (the degrade, and each waiting call of ⊤).
    pub(crate) fn proxy_callable(&mut self, p: u32) {
        if self.proxies[p as usize].callable {
            return;
        }
        self.proxies[p as usize].callable = true;
        self.proxy_degrade(p, "call");
        let calls = std::mem::take(&mut self.proxies[p as usize].calls);
        for (n, c) in calls {
            self.apply_inner(n, &c, Tokens::Slice(&[TOP]), 0);
        }
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
            // [[Set]] (10.5.9) with the Proxy as the Receiver (`proxy_set`).
            Cons::Store {
                key,
                src,
                site,
                region,
            } => {
                let recv = self.tok(TokKind::Proxy(p));
                self.proxy_set(p, key, src, site, region, recv);
                // Rule PXDEFINE: the forward's OrdinarySet ends at the
                // Receiver, this Proxy (10.1.9.2 OrdinarySetWithOwnDescriptor
                // step 2.c: Receiver.[[GetOwnProperty]](P), 10.5.5; steps
                // 2.d / 2.e: Receiver.[[DefineOwnProperty]](P, desc), 10.5.6).
                // A setter on the target's chain ends it there instead: the
                // model keeps both.
                if !self.proxy_made(p, ProxyOp::OwnProp(key, NO_NODE)) {
                    self.proxy_own_prop(p, key, NO_NODE);
                }
                self.proxy_define(p, key, src, region);
            }
            Cons::DefineOwn { key, value, region } => self.proxy_define(p, key, value, region),
            // Rule PXRECV: the Proxy is the target of a Proxy whose forward
            // passes the original Receiver `recv`.
            Cons::FwdLoad {
                key,
                idx,
                dst,
                recv,
            } => {
                let g = self.proxy_get_r(p, key, idx, recv);
                if g != dst {
                    self.edge(g, dst);
                }
            }
            Cons::FwdStore {
                key,
                src,
                site,
                region,
                recv,
            } => self.proxy_set(p, key, src, site, region, recv),
            // [[HasProperty]] (10.5.7): trap `has( t, key )`.
            Cons::HasProp { key, idx } => {
                if self.proxy_made(p, ProxyOp::Has(key, idx)) {
                    return;
                }
                let t = self.proxies[p as usize].target;
                let k = self.proxy_key_arg(key, idx);
                let ret = self.node_l("proxy-has-ret");
                self.proxy_trap(p, "has", Some(key), vec![t, k], ret);
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
                self.proxy_own_prop(p, key, idx);
            }
            // [[Delete]] (10.5.10): trap `deleteProperty( t, key )`.
            Cons::DeleteKey { key, reason } => {
                let t = self.proxies[p as usize].target;
                if !self.proxy_made(p, ProxyOp::DelTrap(key)) {
                    let k = self.proxy_key_arg(Key::Named(key), NO_NODE);
                    let ret = self.node_l("proxy-delete-ret");
                    self.proxy_trap(p, "deleteProperty", Some(Key::Named(key)), vec![t, k], ret);
                }
                if !self.proxy_made(p, ProxyOp::DelFwd(key, reason)) {
                    self.cons(t, Cons::DeleteKey { key, reason });
                }
            }
            // Rule PXCHAIN: the Proxy as a link of a prototype chain. Every
            // operation that reaches it there is its own internal method
            // with the original receiver (10.1.8.1 step 3, 10.1.7.1 step 3,
            // 10.1.9.2 step 2.b, B.2.2.4): `HasProp` (above, on the
            // receiver's closure), `proxy_chain_get`, `proxy_chain_walk`,
            // `proxy_chain_set`; the forward continues the walk at the target.
            Cons::ChainUnion {
                key,
                dst,
                of,
                builtin,
            } => self.proxy_chain_get(p, key, dst, of, builtin),
            Cons::Chain { origin, key, act } => self.proxy_chain_walk(p, origin, key, act),
            Cons::PlainKey { origin, act } => self.proxy_chain_plain(p, origin, act),
            Cons::ChainSet {
                origin,
                key,
                src,
                region,
            } => {
                if let Some(w) = self.chain_set_pending.remove(&src) {
                    for (s, r) in w {
                        self.edge(s, src);
                        self.edge(region, r);
                    }
                }
                self.proxy_chain_set(p, origin, key, src, region);
            }
            Cons::ChainRecv { origin } => {
                let r = self.proxy_chain_recv(p);
                self.add_tok(r, origin);
            }
            Cons::HolderStep { key, pass, stop } => {
                if !self.proxy_made(p, ProxyOp::Holders(key, pass)) {
                    let t = self.proxies[p as usize].target;
                    self.cons(t, Cons::HolderStep { key, pass, stop });
                }
            }
            // The closure holds the Proxy (the chain operations above find
            // it there) and what its forwards walk: the target and its own
            // closure.
            Cons::ProtoClosure { dst } => {
                let pt = self.tok(TokKind::Proxy(p));
                self.add_tok(dst, pt);
                if !self.proxy_made(p, ProxyOp::Closure(dst)) {
                    let t = self.proxies[p as usize].target;
                    self.cons(t, Cons::ProtoClosure { dst });
                }
            }
            // Rules on a prototype closure that read the builtin prototypes
            // (`ProtoMethods`), the accessors of its holders (`AnyAccessors`)
            // or ⊤ (`StoreTopProto`): a Proxy is none of them and has no own
            // accessor; its target and the target's chain are in the closure
            // (`Closure`) and answer there.
            Cons::ProtoMethods { .. } | Cons::AnyAccessors { .. } | Cons::StoreTopProto { .. } => {}
            // An observation; a throw region: the trap calls run in the
            // shared cell every catch reads (`proxy_trap`).
            Cons::Identity { .. } | Cons::Raise { .. } | Cons::HandRaise { .. } => {}
            // Rule PXCALL: a call or `new` of the Proxy (`proxy_call`).
            Cons::Call { .. }
            | Cons::CallThis { .. }
            | Cons::New { .. }
            // Rule PXCALLR: the relays of a method call that found the
            // Proxy through a receiver (`bind_via`): the `this` it binds
            // (`ThisTo`, the thisArg of the [[Call]]), its arguments and
            // result (`CallArgs`, the call's relay node) and its run per
            // (call, receiver) for a builtin namespace value
            // (`BuiltinCallee`). Each is a part of the Proxy's [[Call]]:
            // the same decision as the call (`proxy_call`).
            | Cons::ThisTo { .. }
            | Cons::CallArgs { .. }
            | Cons::BuiltinCallee => self.proxy_call(n, *c, p),
            // Rule PXCALL: Proxy `p` in the target of called Proxy `of`:
            // `of` is callable when `p` is, so the rule moves to `p`'s target.
            Cons::ProxyCallee { p: of } => self.proxy_callee_rule(of, p),
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
 | Cons::RecordEntry { .. }
            | Cons::TypeIs { .. }
            | Cons::Falsy { .. }
            | Cons::Objects { .. }
            // Rule G1/G2 (`x && x.key`, `x.key === b`): the test reads
            // `x.key` = p.[[Get]](key) (10.5.8, `proxy_get`: the get trap
            // and the forward), then ToBoolean (7.1.2) or IsStrictlyEqual
            // (7.2.16) of the result, which run no trap. The ordinary rule
            // is exactly that: a Load of `key` on a node holding the Proxy
            // token (it reaches the Load arm above) and the Guard on it.
            | Cons::FieldTruthy { .. }
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
            // Rule PXCASE (K7 on a Proxy): `name.toLowerCase()` on a node that
            // holds Strings and the Proxy. The case map is the result rule of
            // the String receivers (22.1.3.28 / .29); for the Proxy the call
            // is p.[[Get]]("toLowerCase") (the Load arm: the get trap) and a
            // call of what it returns. The rule runs no internal method of
            // the Proxy, and an object receiver gives any String in the
            // ordinary rule (`case_map_strs`), as for any other object.
            | Cons::CaseMap { .. }
            // The KEY of a computed store `o[proxy] = v` (the rule sits on
            // the key node): ToPropertyKey (7.1.19) is ToPrimitive with hint
            // String, its own rule (`Cons::Implicit` on every computed key,
            // `proxy_to_primitive`: the [[Get]]s of `@@toPrimitive`,
            // `valueOf`, `toString`). The store then uses that primitive; the
            // ordinary rule reads no internal method of an object key (the
            // any-key store, or the ToPrimitive result key: `implicit`).
            | Cons::StoreIndexed { .. }
            // Rule PXKEY: the rules a computed read puts on its KEY node
            // (`o[proxy]`, `o[proxy](...)`): the past-the-end `undefined`
            // (`ElemUndef`, also on the length node, which holds Numbers
            // only), the read under the key's names (`IdxKey`,
            // `IdxKeyCall`) and the own read (`IdxOwnKey`). Each reads the
            // key's value only: ToPropertyKey is ToPrimitive, the key's own
            // rule (`Cons::Implicit`, `proxy_to_primitive`), and the
            // receiver's [[Get]] runs no internal method of the key. The
            // ordinary rule treats an object key as one that may spell any
            // name and is no index, exactly as for any other object.
            | Cons::ElemUndef { .. }
            | Cons::IdxKey { .. }
            | Cons::IdxKeyCall { .. }
            | Cons::IdxOwnKey { .. } => {
                let t = self.tok(TokKind::Proxy(p));
                self.apply_inner(n, c, Tokens::Slice(&[t]), 0);
            }
            // Rule PXVAL: rules that read the Proxy as a value and run no
            // internal method of it. `LenIndex`, `LenPermuted`, `IndexHole`,
            // `IndexHoleIn` and `IndexKept` are rules of an Array exotic
            // object (10.4.2), which a Proxy is not (`is_arr`): the index
            // store into it is its [[Set]] (10.5.9), the Store arm.
            // `ArrayLenArg`: `Array(p)` holds the Proxy as its one element
            // (23.1.1.1 step 4: an argument that is no Number).
            // `NotObjectProto` (7.2.16 IsStrictlyEqual), `ObjTagProbe` (a
            // value a [[Get]] returned: an object there widens ToString, as
            // any object does), `TopIfAny`, `TopIfTop` and `ForInMeet` test
            // the value only. `ThisInst`: the run binds `this` = the Proxy;
            // the body's reads of `this` reach the Load arm (the get trap).
            Cons::LenIndex { .. }
            | Cons::LenPermuted { .. }
            | Cons::IndexHole
            | Cons::IndexHoleIn { .. }
            | Cons::IndexKept
            | Cons::ArrayLenArg { .. }
            | Cons::NotObjectProto { .. }
            | Cons::NotError { .. }
            | Cons::ObjTagProbe { .. }
            | Cons::TopIfAny { .. }
            | Cons::TopIfTop { .. }
            | Cons::ForInMeet { .. }
            | Cons::ThisInst { .. }
            // `Object.create( p )` (20.1.2.2, OrdinaryObjectCreate 10.1.12):
            // `p` becomes the new object's [[Prototype]]; no internal method
            // of `p` runs. The operations that later reach it on the chain
            // run its traps (`proxy_chain_get`, `proxy_chain_has`, ...).
            | Cons::CreateProto { .. }
            // A prototype given to `Object.setPrototypeOf`, `__proto__` or
            // `extends` (`proto_objects`, `ProtoGiven`, `ParentGiven`): a
            // value test; the [[SetPrototypeOf]] of the receiver runs no
            // internal method of the new prototype (10.1.2.1).
            | Cons::ProtoObjects { .. }
            | Cons::ProtoGiven { .. }
            | Cons::ParentGiven { .. } => {
                let t = self.tok(TokKind::Proxy(p));
                self.apply_inner(n, c, Tokens::Slice(&[t]), 0);
            }
            // Rule PXSTR: the constant folds of String and RegExp methods
            // (`StrIndex` KEY2a, `StrSubstring` KEY2b, `StrReplace` M6,
            // `ClassReplace` M9, `Captures` RT) read the operand's String
            // constants only; an object operand gives any Number or String,
            // as every object does. The operand's ToString and
            // ToIntegerOrInfinity (7.1.17, 7.1.5: ToPrimitive 7.1.1) are its
            // own rule (`coerce_operands` / `to_prim`: `Cons::Implicit`
            // ToPrimitive, `proxy_to_primitive`), and so are replace's
            // GetMethod(searchValue, @@replace) (22.1.3.19 step 2,
            // `sym_call`: the Load arm, the get trap) and a callable
            // replaceValue (step 6, `replace_value`).
            Cons::StrIndex { .. }
            | Cons::StrSubstring { .. }
            | Cons::StrReplace { .. }
            | Cons::ClassReplace { .. }
            | Cons::Captures { .. } => {
                let t = self.tok(TokKind::Proxy(p));
                self.apply_inner(n, c, Tokens::Slice(&[t]), 0);
            }
            // ToPrimitive (7.1.1), and ToPropertyKey's (7.1.19, hint String).
            Cons::Implicit {
                reason,
                proto: Protocol::ToPrimitive,
                region,
            } => self.proxy_to_primitive(p, reason, region),
            // The formatter showing the Proxy (`proxy_inspect_shown`).
            Cons::Implicit {
                reason,
                proto: Protocol::InspectShown(at),
                region,
            } => self.proxy_inspect_shown(p, at, reason, region),
            // SerializeJSONProperty (`proxy_json`) and the formatter on a
            // console argument (`proxy_inspect`).
            Cons::Implicit {
                reason,
                proto: Protocol::Json,
                region,
            } => self.proxy_json(p, reason, region),
            Cons::Implicit {
                reason,
                proto: Protocol::Inspect(at),
                region,
            } => self.proxy_inspect(p, at, reason, region),
            // The own keys (`proxy_own_keys`).
            Cons::Implicit {
                reason,
                proto: Protocol::OwnKeys,
                ..
            } => self.proxy_own_keys(p, reason),
            // A for-in body over the Proxy (14.7.5.9
            // EnumerateObjectProperties): its keys are whatever the traps
            // return, so the body runs once with every String key
            // (`for_in_every_key`); its `p[k]` reads are [[Get]]s (the Load
            // arm), the enumeration itself `proxy_own_keys`.
            Cons::ForInKeys { ft, call } => self.for_in_every_key(call, ft),
            // Rule PK on a Proxy (Object.getOwnPropertyNames): its own
            // keys are what its ownKeys trap (or the target's own) returns
            // (10.5.11 [[OwnPropertyKeys]]), kept to Strings (20.1.2.10):
            // any String. The traps run in the operand's own
            // `Implicit OwnKeys` (`proxy_own_keys`).
            Cons::OwnNames { dst } => {
                self.add_prim(dst, prim::STR);
            }
            // Another protocol touch: its hold names the protocol and the
            // touch's own reason.
            Cons::Implicit { reason, proto, .. } => {
                let cause = format!("implicit {proto:?}: {}", self.reasons[reason as usize]);
                self.proxy_degrade(p, &cause);
                self.apply_inner(n, c, Tokens::Slice(&[TOP]), 0);
            }
            // A hard operation: its hold names the kind and the reason.
            Cons::Hard { kind, reason } => {
                let cause = format!("hard {}: {}", kind.code(), self.reasons[reason as usize]);
                self.proxy_degrade(p, &cause);
                self.apply_inner(n, c, Tokens::Slice(&[TOP]), 0);
            }
            _ => {
                self.proxy_degrade(p, c.kind_name());
                self.apply_inner(n, c, Tokens::Slice(&[TOP]), 0);
            }
        }
    }
}
