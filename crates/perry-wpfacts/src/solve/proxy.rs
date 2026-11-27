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
//! PXESC: an escape of the token (unknown code, a ⊤ sink, a reflecting
//! runtime helper) runs every trap with unknown operands and escapes the
//! target (`proxy_escape`); a rule with no Proxy model (each named in
//! `proxy_apply`) is that escape with the rule in its reason, its result
//! unknown. Nothing holds the target as E9.

use super::*;

/// The HIR form `new Proxy( t, h )` as a `Cons::NewNamed` call: a solver
/// spelling (`#`), never a program name.
pub(crate) const PROXY_NEW: &str = "#proxy-new";

#[derive(Clone, Debug)]
pub struct ProxyRec {
    pub target: NodeId,
    pub handler: NodeId,
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
    /// Rule PXDEFINE: the `get` (false) / `set` (true) field of the
    /// descriptor object that trap call is passed.
    DefineAcc(Key, bool),
    /// Rule PXDEFP: the forward of one definition's value (`key`, value
    /// node, site) to the target: target.[[DefineOwnProperty]].
    DefineFwd(Key, NodeId, SiteId),
    /// Rule PXDEFP: the forward of a definition's key class
    /// (`Cons::DefineKey`: key, data) to the target.
    DefineKey(Atom, bool),
    /// Rule PXDEFP: the forward of one accessor a definition installs
    /// (key, function, setter) to the target.
    AccFwd(Atom, Tok, bool),
    /// Rule PXDEFP: the forward of a hard operation to the target.
    HardFwd(Hard),
    /// Rule PXAPPLY: the [[Call]] of one call site: its `this` node.
    Apply(CallId),
    /// Rule PXAPPLY: the [[Construct]] of one call site with its newTarget
    /// node (`NO_NODE`: `new`, the Proxy itself; else Reflect.construct's).
    Construct(CallId, NodeId),
    /// Rule PXNEWT: GetPrototypeFromConstructor(p) (10.1.14): the objects
    /// p.[[Get]]("prototype") returns, or the realm's default prototype.
    NtProto,
    /// Rule PXAPPLY: the receivers of the `ThisTo` relays.
    ThisTo,
    /// [[HasProperty]] of `key` (with its key node): trap and forward.
    Has(Key, NodeId),
    /// [[GetOwnProperty]] of `key` (with its key node): trap and forward.
    OwnProp(Key, NodeId),
    /// The `deleteProperty` trap call for `key`.
    DelTrap(Atom),
    /// Rule PXPROTO: the `setPrototypeOf` trap call and the forward of one
    /// prototype value node.
    SetProto(NodeId),
    /// [[OwnPropertyKeys]] alone: the `ownKeys` trap and the target's own.
    KeysTrap,
    /// The forward of one delete (`key`, its reason) to the target.
    DelFwd(Atom, u32),
    /// [[OwnPropertyKeys]] and [[GetOwnProperty]] of every key: the
    /// `ownKeys` / `getOwnPropertyDescriptor` traps and the target's own.
    OwnKeys,
    /// [[GetPrototypeOf]] (10.5.1): the `getPrototypeOf` trap and the
    /// forward, its result node (`proxy_proto_trap`).
    ProtoTrap,
    /// for-in's walk on from the Proxy's prototype (`proxy_own_keys`).
    ProtoWalk,
    /// Rule PXINV: [[IsExtensible]] (10.5.3): the `isExtensible` trap and
    /// the forward.
    IsExt,
    /// Rule PXINV: the invariant checks of one operation class on the
    /// target (`proxy_inv`): target.[[GetOwnProperty]] of (key, key node)
    /// and IsExtensible(target).
    Inv(Key, NodeId, bool),
    /// Rule PXINV: the `deleteProperty` trap call and the forward of a
    /// delete under the computed key value `idx` (`Cons::DeleteIdx`).
    DelIdx(NodeId),
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
    /// Rule PXESC: the Proxy reached unknown code (`proxy_escape`).
    Escape,
    /// Rule PXOWN: the node of the `getOwnPropertyDescriptor` trap's
    /// results for any key (`proxy_own_props`).
    OwnDesc,
    /// Rule PXOWN: one copy of the own properties (CopyDataProperties)
    /// into the targets node at a site: its stores.
    Copy(NodeId, SiteId),
    /// Rule PXOWN: Array.from's array-like read of the Proxy into one node.
    FromLike(NodeId),
    /// Rule PXOWN: the descriptor values one getOwnPropertyDescriptor(s)
    /// node takes: the trap result's fields and the forward.
    DescValues(NodeId),
    /// Rule PXOWN: the getters (false) / setters (true) of those
    /// descriptors one node takes.
    DescAcc(bool, NodeId),
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

    /// Rule PXESC: Proxy `p` reaches unknown code (an escape, a ⊤ sink, a
    /// reflecting runtime helper): that code may run every internal method
    /// of it (ECMA-262 10.5.1-10.5.13) with operands of its own. Each trap
    /// runs as the operation calls it: `this` = the handler, the target its
    /// first argument, the key any String or Symbol (ToPropertyKey's), each
    /// other operand (a value, a descriptor, a Receiver, a thisArgument, an
    /// argument list, a newTarget) unknown, and its result goes back to that
    /// code. Each forward is the same internal method of the target with
    /// those operands, which is the target's own escape: the target
    /// escapes (a Proxy target runs this rule in turn). Once per Proxy.
    pub(crate) fn proxy_escape(&mut self, p: u32, reason: u32) {
        if self.proxy_made(p, ProxyOp::Escape) {
            return;
        }
        let t = self.proxies[p as usize].target;
        let unk = self.node_l("proxy-escape-operand");
        self.add_top(unk, |m| {
            format!(
                "operand of an escaped Proxy's trap: {}",
                m.reasons[reason as usize]
            )
        });
        self.add_prim(unk, prim::ALL);
        let k = self.proxy_key_arg(Key::any(), NO_NODE);
        let ret = self.node_l("proxy-escape-ret");
        self.sink_node(ret, reason);
        let any = Some(Key::any());
        let traps: [(&str, Option<Key>, Vec<NodeId>); 13] = [
            ("getPrototypeOf", None, vec![t]),
            ("setPrototypeOf", None, vec![t, unk]),
            ("isExtensible", None, vec![t]),
            ("preventExtensions", None, vec![t]),
            ("getOwnPropertyDescriptor", any, vec![t, k]),
            ("defineProperty", any, vec![t, k, unk]),
            ("has", any, vec![t, k]),
            ("get", any, vec![t, k, unk]),
            ("set", any, vec![t, k, unk, unk]),
            ("deleteProperty", any, vec![t, k]),
            ("ownKeys", None, vec![t]),
            ("apply", None, vec![t, unk, unk]),
            ("construct", None, vec![t, unk, unk]),
        ];
        for (name, key, args) in traps {
            self.proxy_trap(p, name, key, args, ret);
        }
        self.sink_node(t, reason);
    }

    /// The key argument a trap receives: ToPropertyKey's result (a String
    /// or a Symbol). Named: that string (or the well-known symbol); else any
    /// string or symbol. A computed key's value is never passed itself: a
    /// Number index reaches the trap as its string (`p[2]` passes `"2"`).
    fn proxy_key_arg(&mut self, key: Key, idx: NodeId) -> NodeId {
        let k = self.node_l("proxy-key");
        if idx != NO_NODE {
            self.add_prim(k, prim::STR | prim::SYMBOL);
            return k;
        }
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

    /// Rule PXINV: the internal methods an operation's invariant checks
    /// (ECMA-262 10.5.x, after the trap returns) call on the target:
    /// target.[[GetOwnProperty]](P) (`own`: the key and its key node) and
    /// IsExtensible(target) (`ext`). On an ordinary target they run no code;
    /// a Proxy target runs its `getOwnPropertyDescriptor` / `isExtensible`
    /// trap and its own checks (`Cons::OwnProp`, `Cons::Meta`). Once per
    /// (Proxy, checks).
    fn proxy_inv(&mut self, p: u32, own: Option<(Key, NodeId)>, ext: bool) {
        if let Some((key, idx)) = own {
            if !self.proxy_made(p, ProxyOp::Inv(key, idx, false)) {
                let t = self.proxies[p as usize].target;
                self.cons(t, Cons::OwnProp { key, idx });
            }
        }
        if ext && !self.proxy_made(p, ProxyOp::Inv(Key::any(), NO_NODE, true)) {
            let t = self.proxies[p as usize].target;
            let reason = self.reason("Proxy invariant check: IsExtensible(target)");
            self.cons(
                t,
                Cons::Meta {
                    op: MetaOp::IsExtensible,
                    reason,
                },
            );
        }
    }

    /// Rule PXINV: p.[[IsExtensible]]() (10.5.3): trap `isExtensible( t )`;
    /// step 8 IsExtensible(target), which is also the forward (step 6).
    fn proxy_is_extensible(&mut self, p: u32) {
        if self.proxy_made(p, ProxyOp::IsExt) {
            return;
        }
        let t = self.proxies[p as usize].target;
        let ret = self.node_l("proxy-isextensible-ret");
        self.proxy_trap(p, "isExtensible", None, vec![t], ret);
        self.proxy_inv(p, None, true);
    }

    /// p.[[GetPrototypeOf]]() (10.5.1): trap `getPrototypeOf( t )`, whose
    /// result is an Object or null (step 7: a TypeError otherwise); the
    /// forward target.[[GetPrototypeOf]]() (step 6), also step 11's check
    /// on a non-extensible target, and IsExtensible(target) (step 8). The
    /// result node, made once.
    fn proxy_proto_trap(&mut self, p: u32) -> NodeId {
        if let Some(&n) = self.proxies[p as usize].ops.get(&ProxyOp::ProtoTrap) {
            return n;
        }
        let pn = self.node_l("proxy-getprototypeof");
        self.proxies[p as usize].ops.insert(ProxyOp::ProtoTrap, pn);
        let t = self.proxies[p as usize].target;
        let raw = self.node_l("proxy-getprototypeof-ret");
        self.proxy_trap(p, "getPrototypeOf", None, vec![t], raw);
        self.cons(raw, Cons::Objects { dst: pn });
        self.cons(
            raw,
            Cons::TypeIs {
                mask: prim::NULL,
                dst: pn,
            },
        );
        self.proxy_inv(p, None, true);
        self.cons(t, Cons::ProtoOf { dst: pn });
        pn
    }

    /// [[Get]] (10.5.8): trap `get( t, key, receiver )`, and the forward.
    /// The result node of the class (`ProxyOp::Get`): made once, with one
    /// trap call returning into it and one forward read of the target into
    /// it. The class is entered before the forward runs: a target holding
    /// `p` reads into the same node.
    pub(crate) fn proxy_get(&mut self, p: u32, key: Key, idx: NodeId) -> NodeId {
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
        self.proxy_inv(p, Some((key, idx)), false);
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
                self.proxy_inv(p, Some((key, NO_NODE)), false);
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
    /// (`ProxyOp::OwnProp`) is the caller's. The trap result is read only
    /// by ToPropertyDescriptor (step 9, 6.2.6.5: HasProperty and Get of its
    /// six fields, getters run) and handed to no code; a result that is
    /// neither an Object nor `undefined` throws a realm TypeError (step 7).
    /// What a descriptor hands out (`value`, `get`, `set`) is
    /// `Cons::OwnDescValues` / `Cons::OwnDescAcc`'s.
    fn proxy_own_prop(&mut self, p: u32, key: Key, idx: NodeId) {
        let t = self.proxies[p as usize].target;
        let k = self.proxy_key_arg(key, idx);
        let ret = self.node_l("proxy-gopd-ret");
        let r = self.reason("Proxy getOwnPropertyDescriptor trap result");
        self.proxy_trap(p, "getOwnPropertyDescriptor", Some(key), vec![t, k], ret);
        self.proxy_result_reads(
            ret,
            &[
                "enumerable",
                "configurable",
                "value",
                "writable",
                "get",
                "set",
            ],
            false,
            r,
        );
        self.cons(t, Cons::OwnProp { key, idx });
        self.proxy_inv(p, None, true);
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
                for (name, setter) in [("get", false), ("set", true)] {
                    let a = self.atom(name);
                    let f = self.field(dt, a);
                    self.proxies[p as usize]
                        .ops
                        .insert(ProxyOp::DefineAcc(key, setter), f);
                }
                self.cons(t, Cons::OwnProp { key, idx: NO_NODE });
                self.proxy_inv(p, None, true);
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
        if value != NO_NODE {
            self.edge(value, v);
        }
        self.edge(throw, region);
    }

    /// Rule PXDEFP: `Object.defineProperty( p, key, desc )`,
    /// `Reflect.defineProperty`, `Object.defineProperties` (20.1.2.4,
    /// 28.1.3, 20.1.2.3 -> DefinePropertyOrThrow 7.3.8 / ObjectDefine-
    /// Properties): ToPropertyDescriptor ran at the call (the descriptor's
    /// [[Get]]s, `Gen::descriptor_reads`); the definition is
    /// p.[[DefineOwnProperty]](key, Desc) (10.5.6), never a [[Set]]: no
    /// `set` trap runs. The trap is passed FromPropertyDescriptor(Desc), a
    /// new object (`proxy_define`): its `value` is the defined value. The
    /// forward (step 7) is target.[[DefineOwnProperty]](key, Desc): the
    /// same definition on the target (its value at the define site, which
    /// runs no setter, `Model::define_site`).
    fn proxy_define_value(&mut self, p: u32, key: Key, src: NodeId, site: SiteId, region: NodeId) {
        self.proxy_define(p, key, src, region);
        if !self.proxy_made(p, ProxyOp::DefineFwd(key, src, site)) {
            let t = self.proxies[p as usize].target;
            self.cons(
                t,
                Cons::Store {
                    key,
                    src,
                    site,
                    region,
                },
            );
        }
    }

    /// Rule PXDEFP: a definition installs accessor `f` under `key` (`*`: a
    /// computed key) on Proxy `p`: the trap's descriptor object has `f` as
    /// its `get` / `set` and no `value` (FromPropertyDescriptor 6.2.6.4:
    /// an accessor descriptor; the field reads `undefined`), and the
    /// forward installs it on the target.
    fn proxy_define_accessor(&mut self, p: u32, key: Atom, f: Tok, setter: bool, from: NodeId) {
        let star = self.atom(crate::STAR_KEY);
        let k = match key == star {
            true => Key::any(),
            false => Key::Named(key),
        };
        let thrown = self.thrown;
        self.proxy_define(p, k, NO_NODE, thrown);
        let v = self.proxies[p as usize].ops[&ProxyOp::DefineTrap(k)];
        self.add_prim(v, prim::UNDEF);
        let fd = self.proxies[p as usize].ops[&ProxyOp::DefineAcc(k, setter)];
        match f == TOP {
            true => self.add_top(fd, |_| "accessor of unknown function value".into()),
            false => self.add_tok(fd, f),
        }
        if !self.proxy_made(p, ProxyOp::AccFwd(key, f, setter)) {
            let t = self.proxies[p as usize].target;
            self.cons(
                t,
                Cons::AccessorInstall {
                    key,
                    f,
                    setter,
                    from,
                },
            );
        }
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
        self.proxy_keys_trap(p, reason);
        // [[GetOwnProperty]] of each key: the trap, the descriptor read.
        let k = self.proxy_key_arg(Key::any(), NO_NODE);
        let desc = self.node_l("proxy-gopd-ret");
        self.proxies[p as usize].ops.insert(ProxyOp::OwnDesc, desc);
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
    }

    /// [[OwnPropertyKeys]] (10.5.11) of Proxy `p`: the `ownKeys` trap with
    /// the list read off its result (CreateListFromArrayLike 7.3.18), and,
    /// without a trap and for the invariant checks (steps 9-ff:
    /// target.[[OwnPropertyKeys]], target.[[GetOwnProperty]] of each key),
    /// the target's own (`Implicit OwnKeys` on it). Once per Proxy.
    fn proxy_keys_trap(&mut self, p: u32, reason: u32) {
        if self.proxy_made(p, ProxyOp::KeysTrap) {
            return;
        }
        let target = self.proxies[p as usize].target;
        let keys = self.node_l("proxy-ownkeys-ret");
        self.proxy_trap(p, "ownKeys", None, vec![target], keys);
        self.proxy_result_reads(keys, &["length"], true, reason);
        self.proxy_inv(p, None, true);
        self.cons(
            target,
            Cons::Implicit {
                reason,
                proto: Protocol::OwnKeys,
                region: self.thrown,
            },
        );
    }

    /// Rule PXINT: [[PreventExtensions]] and SetIntegrityLevel (7.3.15) of
    /// Proxy `p` at level `kind` (`Object.preventExtensions`,
    /// `Reflect.preventExtensions`, `Object.seal`, `Object.freeze`):
    /// O.[[PreventExtensions]]() (10.5.4), trap `preventExtensions( t )`;
    /// for seal and freeze then O.[[OwnPropertyKeys]]() (10.5.11,
    /// `proxy_keys_trap`), for freeze O.[[GetOwnProperty]](k) of each key
    /// (10.5.5, `proxy_own_props`), and DefinePropertyOrThrow(O, k, desc)
    /// of each key (10.5.6): trap `defineProperty( t, k, desc )`, desc =
    /// FromPropertyDescriptor (6.2.6.4) of `{ [[Configurable]]: false }`
    /// (freeze: and `[[Writable]]: false` for a data property), a new
    /// object without `value`, `get` or `set`. Without a trap each step
    /// forwards to the target: the same level on it (`Cons::Hard` `kind`),
    /// its own rule. Once per (Proxy, level).
    fn proxy_integrity(&mut self, p: u32, kind: Hard, reason: u32) {
        if self.proxy_made(p, ProxyOp::HardFwd(kind)) {
            return;
        }
        let t = self.proxies[p as usize].target;
        let ret = self.node_l("proxy-preventextensions-ret");
        self.proxy_trap(p, "preventExtensions", None, vec![t], ret);
        // Step 8: IsExtensible(target) when the trap answered true.
        self.proxy_inv(p, None, true);
        if kind != Hard::E8PreventExtensions {
            match kind == Hard::E8Freeze {
                true => self.proxy_own_props(p, reason),
                false => self.proxy_keys_trap(p, reason),
            }
            let k = self.proxy_key_arg(Key::any(), NO_NODE);
            let d = self.node_l("proxy-integrity-desc");
            let dret = self.node_l("proxy-integrity-define-ret");
            self.proxy_trap(p, "defineProperty", Some(Key::any()), vec![t, k, d], dret);
            let call = (self.calls.len() - 1) as CallId;
            let v = self.node_l("proxy-integrity-desc-value");
            self.add_prim(v, prim::UNDEF);
            let dt = builtins::trap_descriptor(self, call, v);
            self.add_tok(d, dt);
        }
        self.cons(t, Cons::Hard { kind, reason });
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
        if self.proxy_made(p, ProxyOp::ProtoWalk) {
            return;
        }
        let proto = self.proxy_proto_trap(p);
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

    /// Rule PXSUPER: `super.key` whose home object's prototype is Proxy `p`
    /// (13.3.7.3 MakeSuperPropertyReference: the base is
    /// HomeObject.[[GetPrototypeOf]](), the Receiver `this` = `origin`):
    /// GetValue is p.[[Get]](key, this) (10.5.8: the get trap with receiver
    /// `this`, the forward to the target with the same receiver), a call
    /// calls that value with `this`, PutValue is p.[[Set]](key, V, this)
    /// (`proxy_chain_set`), a lookup the chain walk.
    pub(crate) fn proxy_super(&mut self, p: u32, origin: Tok, key: Key, act: ChainAct) {
        match act {
            ChainAct::Load(dst, _) => {
                let g = self.proxy_get_r(p, key, NO_NODE, origin);
                self.edge(g, dst);
            }
            ChainAct::Call(call) => {
                let g = self.proxy_get_r(p, key, NO_NODE, origin);
                self.bind_via(g, call, origin);
            }
            ChainAct::Store(src, region) => self.proxy_chain_set(p, origin, key, src, region),
            ChainAct::Lookup { .. } => self.proxy_chain_walk(p, origin, key, act),
        }
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
        self.shared_scope(|m| m.proxy_json(p, reason, region));
        self.shared_scope(|m| m.proxy_to_primitive(p, reason, region));
        self.shared_scope(|m| m.proxy_inspect_shown(p, at, reason, region));
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
    pub(crate) fn proxy_call(&mut self, n: NodeId, c: Cons, p: u32) {
        if self.proxies[p as usize].callable {
            return self.proxy_call_run(c, p);
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

    /// Rule PXAPPLY: one call rule `c` of callable Proxy `p`: [[Call]]
    /// (`proxy_invoke`) with the call's `this` (`Call`: the call site's;
    /// `CallThis`: the receiver; `CallArgs`: the receivers its relay's
    /// `ThisTo` gave, `proxy_this_to`), or [[Construct]]
    /// (`proxy_construct`). `BuiltinCallee` runs a builtin namespace value
    /// per receiver: a Proxy is none, its call is the `CallArgs` rule.
    fn proxy_call_run(&mut self, c: Cons, p: u32) {
        match c {
            Cons::Call { call } => {
                let this = self.calls[call as usize].this.clone();
                self.proxy_invoke(p, call, this);
            }
            Cons::CallThis { call, this } => self.proxy_invoke(p, call, ThisArg::Tok(this)),
            Cons::CallArgs { call } => {
                let th = self.proxy_this_to(p);
                self.proxy_invoke(p, call, ThisArg::Node(th));
            }
            Cons::ThisTo { origin } => {
                let th = self.proxy_this_to(p);
                self.add_tok(th, origin);
            }
            Cons::New { call } | Cons::NewProgram { call } => {
                self.proxy_construct(p, call, NO_NODE)
            }
            // Reflect.construct( p, args, newTarget ) (28.1.2):
            // p.[[Construct]]( args, newTarget ).
            Cons::ReflectNew { call, new_target } => self.proxy_construct(p, call, new_target),
            // Reflect.construct( T, args, p ): the Proxy is the newTarget
            // (IsConstructor(p): its target is a constructor).
            Cons::ReflectNewTarget { call, target } => {
                let pt = self.tok(TokKind::Proxy(p));
                self.reflect_construct(call, target, pt);
            }
            _ => {}
        }
    }

    /// Rule PXAPPLY: the receivers the `ThisTo` relays of Proxy `p` bind
    /// (the `this` of its calls through a receiver's lookup).
    fn proxy_this_to(&mut self, p: u32) -> NodeId {
        if let Some(&th) = self.proxies[p as usize].ops.get(&ProxyOp::ThisTo) {
            return th;
        }
        let th = self.node_l("proxy-this-to");
        self.proxies[p as usize].ops.insert(ProxyOp::ThisTo, th);
        th
    }

    /// Rule PXAPPLY: CreateArrayFromList(argumentsList) (7.3.16) of call
    /// `call`: a new Array per call site whose elements are the arguments
    /// and the elements of its spread arguments.
    fn proxy_args_array(&mut self, call: CallId) -> NodeId {
        let c = self.calls[call as usize].clone();
        let arr = self.node_l("proxy-args");
        let at = builtins::trap_args_array(self, call);
        self.add_tok(arr, at);
        let fa = self.field_any(at);
        for &a in &c.args {
            self.edge(a, fa);
        }
        for &s in &c.spread {
            let key = self.elem_key();
            self.cons(s, Cons::Load { key, dst: fa });
        }
        arr
    }

    /// A call site the solver makes for a forward of `call`: its
    /// arguments, `this` and result `ret`.
    fn proxy_fwd_call(&mut self, call: CallId, this: ThisArg, ret: NodeId) -> CallId {
        let c = self.calls[call as usize].clone();
        let call2 = self.calls.len() as CallId;
        self.calls.push(CallSite {
            first_arg_fixed: c.first_arg_fixed,
            this,
            args: c.args,
            spread: c.spread,
            ret,
            site: c.site,
            region: c.region,
        });
        self.why_mint(call2, None, u32::MAX);
        call2
    }

    /// Rule PXAPPLY: p.[[Call]](thisArgument, argumentsList) (10.5.12) of
    /// call `call`: trap `apply( target, thisArg, argArray )` with `this` =
    /// the handler (step 7, argArray = CreateArrayFromList, a new Array),
    /// its result the call's, its throw the call's region; without a trap
    /// (step 6) Call(target, thisArgument, argumentsList), the forward: a
    /// call of the target with the same `this`, arguments and result.
    /// One class per call (`ProxyOp::Apply`: its `this` node, which every
    /// `this` the call binds joins).
    fn proxy_invoke(&mut self, p: u32, call: CallId, this: ThisArg) {
        let th = match self.proxies[p as usize].ops.get(&ProxyOp::Apply(call)) {
            Some(&th) => th,
            None => {
                let th = self.node_l("proxy-apply-this");
                self.proxies[p as usize].ops.insert(ProxyOp::Apply(call), th);
                let t = self.proxies[p as usize].target;
                let (ret, region) = (self.calls[call as usize].ret, self.calls[call as usize].region);
                let arr = self.proxy_args_array(call);
                self.proxy_trap_in(p, "apply", None, vec![t, th, arr], ret, region);
                let call2 = self.proxy_fwd_call(call, ThisArg::Node(th), ret);
                self.cons(t, Cons::Call { call: call2 });
                th
            }
        };
        match this {
            ThisArg::Node(n) => self.edge(n, th),
            ThisArg::Tok(x) => self.add_tok(th, x),
            ThisArg::Undef => self.add_prim(th, prim::UNDEF),
        }
    }

    /// Rule PXAPPLY / PXNEWT: p.[[Construct]](argumentsList, newTarget)
    /// (10.5.13) of call `call`: trap `construct( target, argArray,
    /// newTarget )` with `this` = the handler, only an Object result the
    /// call's (step 11); without a trap (step 6) Construct(target,
    /// argumentsList, newTarget), the forward: `Cons::ReflectNew` on the
    /// target with the same newTarget, so a Proxy target runs its own
    /// [[Construct]] with it and an ordinary target makes its object with
    /// newTarget's prototype (`reflect_construct`, `proxy_nt_inst`); the
    /// forward's result is the call's as made. `given`: the newTarget node
    /// of `Reflect.construct( p, args, newTarget )`, whose omitted
    /// argument (undefined) is p itself (28.1.2 step 2); `NO_NODE` for
    /// `new p(...)`, whose newTarget is p. Once per (call, given).
    fn proxy_construct(&mut self, p: u32, call: CallId, given: NodeId) {
        if self.proxy_made(p, ProxyOp::Construct(call, given)) {
            return;
        }
        let t = self.proxies[p as usize].target;
        let (ret, region) = (self.calls[call as usize].ret, self.calls[call as usize].region);
        let arr = self.proxy_args_array(call);
        let pt = self.tok(TokKind::Proxy(p));
        let nt = self.node_l("proxy-new-target");
        if given == NO_NODE {
            self.add_tok(nt, pt);
        } else {
            self.cons(given, Cons::Objects { dst: nt });
            let omitted = self.node_l("proxy-new-target-omitted");
            self.cons(
                given,
                Cons::TypeIs {
                    mask: prim::UNDEF,
                    dst: omitted,
                },
            );
            let own = self.node_l("proxy-new-target-self");
            self.add_tok(own, pt);
            self.cons(
                omitted,
                Cons::TopIfAny {
                    dst: nt,
                    from: own,
                    reason: None,
                },
            );
        }
        let tr = self.node_l("proxy-construct-ret");
        self.proxy_trap_in(p, "construct", None, vec![t, arr, nt], tr, region);
        self.cons(tr, Cons::Objects { dst: ret });
        let fr = self.node_l("proxy-construct-fwd");
        let call2 = self.proxy_fwd_call(call, ThisArg::Undef, fr);
        self.cons(
            t,
            Cons::ReflectNew {
                call: call2,
                new_target: nt,
            },
        );
        self.edge(fr, ret);
    }

    /// Rule PXNEWT: GetPrototypeFromConstructor(p, intrinsicDefaultProto)
    /// (10.1.14) for Proxy newTarget `p`: the Objects p.[[Get]]("prototype")
    /// returns (its `get` trap and the forward); a non-Object gives the
    /// realm's intrinsic default (step 4: `%Object.prototype%` for a class
    /// or function, `ObjectProto`, which stands for every builtin
    /// prototype). The node of those prototypes; once per Proxy.
    pub(crate) fn proxy_nt_proto(&mut self, p: u32) -> NodeId {
        if let Some(&n) = self.proxies[p as usize].ops.get(&ProxyOp::NtProto) {
            return n;
        }
        let out = self.node_l("proxy-new-target-proto");
        self.proxies[p as usize].ops.insert(ProxyOp::NtProto, out);
        let pa = self.atom("prototype");
        let pr = self.proxy_get(p, Key::Named(pa), NO_NODE);
        self.cons(pr, Cons::Objects { dst: out });
        let prim_pr = self.node_l("proxy-new-target-proto-not-object");
        self.cons(
            pr,
            Cons::TypeIs {
                mask: prim::ALL,
                dst: prim_pr,
            },
        );
        let realm = self.node_l("proxy-new-target-realm-default-proto");
        let op = self.tok(TokKind::ObjectProto);
        self.add_tok(realm, op);
        self.cons(
            prim_pr,
            Cons::TopIfAny {
                dst: out,
                from: realm,
                reason: None,
            },
        );
        out
    }

    /// Rule PXNEWT: OrdinaryCreateFromConstructor(newTarget) of
    /// `Reflect.construct( target, args, p )` (or a Proxy's forward) with
    /// Proxy newTarget `p` and a class or function `target`: an object of
    /// target's layout per (call, Proxy, target), a birth of its own,
    /// whose prototype is `proxy_nt_proto(p)` alone (`proxy_nt_toks`).
    /// `None`: another unit's layout (one token for all its instances),
    /// which cannot carry a prototype of its own.
    pub(crate) fn proxy_nt_inst(&mut self, call: CallId, p: u32, target: Tok) -> Option<Tok> {
        if let Some(&i) = self.proxy_nt_inst.get(&(call, p, target)) {
            return Some(i);
        }
        let tm = match self.toks[target as usize] {
            TokKind::ClassObj(tm) => tm,
            TokKind::Func(f, _) => self.fn_inst_template(f),
            _ => return None,
        };
        if self.foreign.contains_key(&tm) {
            return None;
        }
        let birth = self.node_l("proxy-new-target-birth");
        let ctx = self.proxy_fwd_call(call, ThisArg::Undef, birth);
        let inst = self.site_tok(tm, ctx);
        self.proxy_nt_toks.insert(inst);
        self.proxy_nt_inst.insert((call, p, target), inst);
        let pr = self.proxy_nt_proto(p);
        let protos = self.proto_node(inst);
        self.edge(pr, protos);
        Some(inst)
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
    /// [[Construct]] the `construct` trap or the forward (10.5.13): each
    /// waiting call runs (`proxy_call_run`).
    pub(crate) fn proxy_callable(&mut self, p: u32) {
        if self.proxies[p as usize].callable {
            return;
        }
        self.proxies[p as usize].callable = true;
        let calls = std::mem::take(&mut self.proxies[p as usize].calls);
        for (_, c) in calls {
            self.proxy_call_run(c, p);
        }
    }

    /// The one rule that sees a Proxy token: operation `c` on Proxy `p`.
    /// [[Set]] (10.5.9) of `key` with the Proxy as the Receiver
    /// (`proxy_set`).
    pub(crate) fn proxy_store(&mut self, p: u32, key: Key, src: NodeId, site: SiteId, region: NodeId) {
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

    pub(crate) fn proxy_apply(&mut self, n: NodeId, c: &Cons, p: u32) {
        match *c {
            // Holds the global object only, never a Proxy.
            Cons::InterpRuns { .. } => {}
            Cons::Load { key, dst } => self.proxy_get_into(p, key, NO_NODE, dst),
            // Not a Map: what an element read of the Proxy reads.
            Cons::MapEntryAt { dst, .. } => {
                let ek = self.elem_key();
                self.proxy_get_into(p, ek, NO_NODE, dst)
            }
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
            // A native's receiver rule (`Model::native_call` per receiver
            // token) for a native whose model holds for a Proxy `this`
            // (`Model::proxy_this_model`: `Function.prototype.{call, apply,
            // toString}`): the Proxy is that receiver.
            Cons::NativeThis { call, f }
                if builtins::native_model(self, f)
                    .is_some_and(|(fam, _, name)| Self::proxy_this_model(fam, name)) =>
            {
                let t = self.tok(TokKind::Proxy(p));
                self.native_this(call, f, Tokens::Slice(&[t]), 0);
            }
            // Rule PXDEFP: the value of a definition (a define site).
            Cons::Store {
                key,
                src,
                site,
                region,
            } if self.define_site(site) => self.proxy_define_value(p, key, src, site, region),
            // Rule PXDEFP: the definition's key class, the target's.
            Cons::DefineKey { key, data, reason } => {
                if !self.proxy_made(p, ProxyOp::DefineKey(key, data)) {
                    let t = self.proxies[p as usize].target;
                    self.cons(t, Cons::DefineKey { key, data, reason });
                }
            }
            // Rule PXDEFP: an accessor the definition installs.
            Cons::AccessorInstall {
                key,
                f,
                setter,
                from,
            } => self.proxy_define_accessor(p, key, f, setter, from),
            // Rule PXDEFP: a definition under a computed key (E5 on an
            // ordinary object): the `defineProperty` trap under any key, and
            // the forward, the target's E5. Its value and accessors are the
            // definition's own rules (the arms above).
            Cons::Hard {
                kind: Hard::E5DefineProperty,
                reason,
            } => {
                let thrown = self.thrown;
                self.proxy_define(p, Key::any(), NO_NODE, thrown);
                let v = self.proxies[p as usize].ops[&ProxyOp::DefineTrap(Key::any())];
                self.add_prim(v, prim::UNDEF);
                if !self.proxy_made(p, ProxyOp::HardFwd(Hard::E5DefineProperty)) {
                    let t = self.proxies[p as usize].target;
                    self.cons(
                        t,
                        Cons::Hard {
                            kind: Hard::E5DefineProperty,
                            reason,
                        },
                    );
                }
            }
            // Rule PXPROTO: [[SetPrototypeOf]] (10.5.2) with V = what `src`
            // holds (`Object.setPrototypeOf`, `Reflect.setPrototypeOf`,
            // the `__proto__` setter): trap `setPrototypeOf( t, V )`; the
            // forward t.[[SetPrototypeOf]](V), the same rule on the target.
            // The operation's E6 mark forwards with it (below).
            Cons::ProtoFrom { src } => {
                if !self.proxy_made(p, ProxyOp::SetProto(src)) {
                    let t = self.proxies[p as usize].target;
                    let ret = self.node_l("proxy-setprototypeof-ret");
                    self.proxy_trap(p, "setPrototypeOf", None, vec![t, src], ret);
                    // Steps 9-ff: IsExtensible(target), then
                    // target.[[GetPrototypeOf]]() compared with V.
                    self.proxy_inv(p, None, true);
                    let r = self.reason("Proxy invariant check: target.[[GetPrototypeOf]]");
                    self.cons(
                        t,
                        Cons::Meta {
                            op: MetaOp::ProtoOf,
                            reason: r,
                        },
                    );
                    self.cons(t, Cons::ProtoFrom { src });
                }
            }
            Cons::Hard {
                kind: Hard::E6SetPrototypeOf,
                reason,
            } => {
                if !self.proxy_made(p, ProxyOp::HardFwd(Hard::E6SetPrototypeOf)) {
                    let t = self.proxies[p as usize].target;
                    self.cons(
                        t,
                        Cons::Hard {
                            kind: Hard::E6SetPrototypeOf,
                            reason,
                        },
                    );
                }
            }
            // Rule PXDEL: a delete under a computed key (E7 on an ordinary
            // object): the forward, the target's E7. Its [[Delete]] (10.5.10)
            // trap call is the `Cons::DeleteIdx` beside it, which carries the
            // key value.
            Cons::Hard {
                kind: Hard::E7Delete,
                reason,
            } => {
                if !self.proxy_made(p, ProxyOp::HardFwd(Hard::E7Delete)) {
                    let t = self.proxies[p as usize].target;
                    self.cons(
                        t,
                        Cons::Hard {
                            kind: Hard::E7Delete,
                            reason,
                        },
                    );
                }
            }
            // ENUM-DEL: a for-in over a Proxy yields what its `ownKeys` and
            // `getOwnPropertyDescriptor` traps give (10.5.11, 10.5.5): any
            // key, the delete under any key above (PXDEL).
            Cons::DeleteEnum { reason } => self.proxy_apply(
                n,
                &Cons::Hard {
                    kind: Hard::E7Delete,
                    reason,
                },
                p,
            ),
            // Rule PXINV: [[Delete]] (10.5.10) under the key value `idx`:
            // trap `deleteProperty( t, key )` with the program's key, the
            // checks target.[[GetOwnProperty]](key) and IsExtensible(target),
            // and the forward of the key to the target.
            Cons::DeleteIdx { idx, reason } => {
                if !self.proxy_made(p, ProxyOp::DelIdx(idx)) {
                    let t = self.proxies[p as usize].target;
                    let ret = self.node_l("proxy-delete-ret");
                    self.proxy_trap(p, "deleteProperty", Some(Key::any()), vec![t, idx], ret);
                    self.proxy_inv(p, Some((Key::any(), idx)), true);
                    self.cons(t, Cons::DeleteIdx { idx, reason });
                }
            }
            // Rule PXINV: the internal methods an invariant check or a test
            // (`Object.isExtensible`) runs on the Proxy.
            Cons::Meta { op, reason } => match op {
                MetaOp::IsExtensible => self.proxy_is_extensible(p),
                MetaOp::ProtoOf => {
                    self.proxy_proto_trap(p);
                }
                MetaOp::OwnKeys => self.proxy_keys_trap(p, reason),
            },
            // [[GetPrototypeOf]] (10.5.1): `Object.getPrototypeOf`,
            // `Reflect.getPrototypeOf`, the prototype a `super` home reads.
            Cons::ProtoOf { dst } => {
                let pn = self.proxy_proto_trap(p);
                self.edge(pn, dst);
            }
            // Rule PXINT: an integrity level (`proxy_integrity`).
            Cons::Hard {
                kind: kind @ (Hard::E8Freeze | Hard::E8Seal | Hard::E8PreventExtensions),
                reason,
            } => self.proxy_integrity(p, kind, reason),
            // [[Set]] (10.5.9) with the Proxy as the Receiver (`proxy_set`).
            Cons::Store {
                key,
                src,
                site,
                region,
            } => self.proxy_store(p, key, src, site, region),
            Cons::DefineOwn { key, value, region } => self.proxy_define(p, key, value, region),
            // Rule PXASSIGN: `Object.assign( px, s )`: the copy's Sets of
            // `s`'s keys on the Proxy (`spread_into`, `store_into`). Other
            // copies (a literal's spread, a rest object) copy into a fresh
            // object, never into a Proxy.
            // JSON-RT: a Proxy's serialization runs its traps: content the
            // model does not hold (`json_rt_apply`).
            Cons::JsonCopy { dst, site, arr, at } => {
                let t = self.tok(TokKind::Proxy(p));
                self.json_rt_apply(dst, site, arr, at, crate::delta::Tokens::Slice(&[t]), 0);
            }
            Cons::SpreadFrom { src, site, from } => {
                let t = self.tok(TokKind::Proxy(p));
                self.spread_into(src, t, site, from);
            }
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
                self.proxy_inv(p, Some((key, idx)), true);
                self.cons(t, Cons::HasProp { key, idx });
            }
            // [[GetOwnProperty]] (10.5.5): trap `getOwnPropertyDescriptor( t,
            // key )`. ToPropertyDescriptor reads the returned object's fields
            // by HasProperty / Get (6.2.6.5, `proxy_own_prop`).
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
                    self.proxy_inv(p, Some((Key::Named(key), NO_NODE)), true);
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
            Cons::Identity { .. } | Cons::Detached | Cons::Raise { .. } | Cons::HandRaise { .. } => {}
            // Rule PXCALL: a call or `new` of the Proxy (`proxy_call`).
            Cons::Call { .. }
            | Cons::CallThis { .. }
            | Cons::New { .. }
            | Cons::NewProgram { .. }
            | Cons::ReflectNew { .. }
            | Cons::ReflectNewTarget { .. }
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
 | Cons::ArrayRecordEntry { .. }
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
            // `ProtoValues` on a prototype node that holds the Proxy: the
            // Proxy itself is that [[Prototype]] value (what a
            // [[GetPrototypeOf]] of the object holding it returns, a
            // `super` home object's included); no internal method of the
            // Proxy runs.
            | Cons::ProtoValues { .. }
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
            | Cons::ParentGiven { .. }
            // Rule PXDEFP: a definition under a computed key
            // (`define_computed_key`) decides per receiver; a Proxy there
            // takes the computed-key path, whose E5, value and accessors are
            // its arms above. `DefineKeyed` on a key node that holds the
            // Proxy: an object key is no constant (ToPropertyKey is its own
            // rule), the computed-key definition.
            | Cons::DefineAnyKey { .. }
            | Cons::DefineKeyed { .. }
            | Cons::DeleteKeyed { .. }
            // ENUM-DEL: a Proxy in the watch of a global object's unknown-key
            // writes is a value there (`Cons::DeleteEnumWhen`); no internal
            // method of the Proxy runs.
            | Cons::DeleteEnumWhen { .. }
            // Rule PXSUPER: the Proxy as the home object's prototype of a
            // `super.key` (its [[GetPrototypeOf]] result: the lookup starts
            // at it, `super_lookup`), or as the `this` of one (the
            // Receiver: the lookup itself is on the home prototype).
            | Cons::SuperFrom { .. }
            | Cons::Super { .. } => {
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
            | Cons::Captures { .. }
            // A run's argument and result relays (DECISIONS 44 (b)): the
            // Proxy is a value that flows to the call's result; only the
            // markers there map families.
            | Cons::ArgInto { .. }
            | Cons::RetInto { .. }
            // Rule PXASSIGN: the Proxy as a value of a copied unnamed key
            // (`fa_s` of a copy source): the Set of it on the target is the
            // ordinary rule; no internal method of the Proxy runs.
            | Cons::UnnamedSet { .. } => {
                let t = self.tok(TokKind::Proxy(p));
                self.apply_inner(n, c, Tokens::Slice(&[t]), 0);
            }
            // ToPrimitive (7.1.1), and ToPropertyKey's (7.1.19, hint String).
            Cons::Implicit {
                reason,
                proto: Protocol::ToPrimitive,
                region,
            } => self.shared_scope(|m| m.proxy_to_primitive(p, reason, region)),
            // The formatter showing the Proxy (`proxy_inspect_shown`).
            Cons::Implicit {
                reason,
                proto: Protocol::InspectShown(at),
                region,
            } => self.shared_scope(|m| m.proxy_inspect_shown(p, at, reason, region)),
            // SerializeJSONProperty (`proxy_json`) and the formatter on a
            // console argument (`proxy_inspect`).
            Cons::Implicit {
                reason,
                proto: Protocol::Json,
                region,
            } => self.shared_scope(|m| m.proxy_json(p, reason, region)),
            Cons::Implicit {
                reason,
                proto: Protocol::Inspect(at),
                region,
            } => self.shared_scope(|m| m.proxy_inspect(p, at, reason, region)),
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
            Cons::ForInKeys { ft, call, .. } => self.for_in_every_key(call, ft),
            // Rule PXENUM: for-in over an object whose chain holds the Proxy
            // (14.7.5.9 EnumerateObjectProperties): the walk reaching it
            // runs its [[OwnPropertyKeys]], [[GetOwnProperty]] and
            // [[GetPrototypeOf]] (`proxy_own_keys`: the ownKeys,
            // getOwnPropertyDescriptor and getPrototypeOf traps and the walk
            // on), and the body's `of[k]` read reaching it is OrdinaryGet's
            // step 3, p.[[Get]](k, of) (`proxy_chain_get`: the get trap with
            // receiver `of`, the forward the target's own and chain).
            Cons::EnumChain { key, dst, of, .. } => {
                let r = self.reason("for-in: a Proxy on the chain of the enumerated object");
                self.proxy_own_keys(p, r);
                let recv = self.proxy_chain_recv(p);
                self.add_tok(recv, of);
                self.proxy_chain_get(p, key, dst, of, false);
            }
            // Rule PK on a Proxy (Object.getOwnPropertyNames): its own
            // keys are what its ownKeys trap (or the target's own) returns
            // (10.5.11 [[OwnPropertyKeys]]), kept to Strings (20.1.2.10):
            // any String. The traps run in the operand's own
            // `Implicit OwnKeys` (`proxy_own_keys`).
            Cons::OwnNames { dst } => {
                self.add_prim(dst, prim::STR);
            }
            // Rule PXNEWT: a Proxy as the newTarget of Reflect.construct
            // gives no class layout a foreign birth: the object made is
            // its own birth of the target's layout (`proxy_nt_inst`),
            // its prototype Get( newTarget, "prototype" ) (10.1.14).
            Cons::NewTargetHard { .. } => {}
            // Rule PXPROTO2: GetIterator (7.4.3), IteratorNext / Close /
            // Complete / Value (7.4.x) and their async twins, Node's
            // `Buffer.from(object)`, InstanceofOperator's `@@hasInstance`
            // (13.10.2) and the error message's inspection of the Proxy:
            // each key the protocol looks up or reads is a [[Get]] of the
            // Proxy (10.5.8: the get trap and the forward), a method found
            // called with `this` = the Proxy (`implicit`). A formatter
            // showing a Proxy target that is itself a Proxy enumerates it
            // (`proxy_own_keys`: the ownKeys and getOwnPropertyDescriptor
            // traps) and reads its values ([[Get]] under any key).
            Cons::Implicit {
                reason,
                proto:
                    pr @ (Protocol::Iterate
                    | Protocol::IterNext
                    | Protocol::IterResult
                    | Protocol::AsyncIterate
                    | Protocol::AsyncIterNext
                    | Protocol::AsyncIterResult
                    | Protocol::BufferFrom
                    | Protocol::HasInstance
                    | Protocol::ShallowInspect
                    | Protocol::InspectTarget(_)),
                region,
            } => {
                if matches!(pr, Protocol::InspectTarget(_)) {
                    self.proxy_own_keys(p, reason);
                }
                let t = self.tok(TokKind::Proxy(p));
                self.implicit(t, reason, pr, region);
            }
            // The `next` of an iterated value read as its own iterator (the
            // ordinary object's rule, `Cons::IterNext`): p.[[Get]]("next")
            // (10.5.8, the get trap and the forward).
            Cons::IterNext { dst } => {
                let a = self.atom("next");
                let f = self.proxy_get(p, Key::Named(a), NO_NODE);
                self.edge(f, dst);
            }
            // StructuredSerialize (HTML 2.7.3) of a Proxy: an exotic object
            // with internal slots of its own throws a DataCloneError before
            // any internal method of it runs: no trap.
            Cons::Implicit {
                proto: Protocol::Clone,
                ..
            } => {}
            // InstanceofOperator( V, p ) (13.10.2): `@@hasInstance` and, for
            // OrdinaryHasInstance, `prototype` are [[Get]]s of the Proxy
            // (`implicit`); the method is called with `V`.
            Cons::HasInstance { reason, arg } => {
                let t = self.tok(TokKind::Proxy(p));
                self.has_instance(t, reason, arg);
            }
            // The hard kinds that reach a Proxy as an operand: as the target
            // of `Proxy.revocable` / a Proxy made by name (E9: its own
            // TopSink beside, the unknown code's), a `with` object (E15:
            // HasBinding's HasProperty and `@@unscopables` [[Get]] and the
            // binding's [[Get]] / [[Set]] are rules of their own on the
            // object, `Gen` WithGet / WithSet) and a transfer-list item (B3:
            // no transferable platform object, a DataCloneError before any
            // internal method runs). A Proxy has no layout to mark.
            Cons::Hard {
                kind: Hard::E9Proxy | Hard::E15With | Hard::B3Transfer,
                ..
            } => {}
            // Rule PXESC: a ⊤ sink and a store the runtime keeps (unknown
            // code holds what reaches them: `escape`), and a reflecting
            // runtime helper (`Protocol::Any`: it may run anything on its
            // operand, as it may call a function handed to it) give the Proxy
            // to unknown code: its escape (`proxy_escape`).
            Cons::TopSink { .. } | Cons::RuntimeStore { .. } => {
                let t = self.tok(TokKind::Proxy(p));
                self.apply_inner(n, c, Tokens::Slice(&[t]), 0);
            }
            Cons::Implicit {
                reason,
                proto: Protocol::Any,
                ..
            } => {
                let t = self.tok(TokKind::Proxy(p));
                self.escape(t, reason);
            }
            // Hard kinds no rule emits on a value node (`mark_hard` sets them
            // on a layout): E10 (OrdinaryObjectCreate sets [[Prototype]], no
            // internal method of the prototype runs), E11 (Reflect.construct
            // is `Cons::ReflectNew`), E21 (the `extends` value's `prototype`
            // [[Get]] is `Cons::NativeParent`'s) and the buffer kinds B1, B2,
            // B4 (a Proxy has no [[ArrayBufferData]]: a TypeError before any
            // internal method). A Proxy has no layout to mark.
            Cons::Hard {
                kind:
                    Hard::E10ObjectCreate
                    | Hard::E11ReflectConstruct
                    | Hard::E21NativeParent
                    | Hard::B1Exposed
                    | Hard::B2BufferResize
                    | Hard::B4Resizable,
                ..
            } => {}
            // Rule PXOWN: EnumerableOwnProperties( p, value ) (7.3.23:
            // Object.values / Object.entries): p.[[OwnPropertyKeys]]()
            // (10.5.11, the `ownKeys` trap), p.[[GetOwnProperty]]( k ) of
            // each String key (10.5.5, `getOwnPropertyDescriptor`), and
            // Get( p, k ) of each enumerable one (10.5.8, the `get` trap with
            // Receiver p, the forward the target's [[Get]]): into `dst`.
            Cons::OwnValues { dst } => {
                let r = self.reason("EnumerableOwnProperties of a Proxy");
                self.proxy_own_props(p, r);
                self.proxy_get_into(p, Key::any(), NO_NODE, dst);
            }
            // Rule PXOWN: EnumerableOwnProperties( p, key ) (Object.entries'
            // keys): the same two traps; a key is a String of the `ownKeys`
            // list (CreateListFromArrayLike, read in `proxy_keys_trap`), any
            // String.
            Cons::OwnKeys { dst } => {
                let r = self.reason("EnumerableOwnProperties of a Proxy");
                self.proxy_own_props(p, r);
                self.add_prim(dst, prim::STR);
            }
            // Rule PXOWN: Object.getOwnPropertyDescriptor(s) of p (20.1.2.8,
            // 20.1.2.9): p.[[OwnPropertyKeys]]() and p.[[GetOwnProperty]]( k )
            // (the `getOwnPropertyDescriptor` trap, its result
            // ToPropertyDescriptor'd, then FromPropertyDescriptor'd: a fresh
            // object whose `value` is the trap result's; `get` / `set` are
            // `Cons::OwnDescAcc`'s); without the trap the target's own
            // descriptor (the forward). getOwnPropertyDescriptor runs one
            // key's trap: the [[OwnPropertyKeys]] here is a superset.
            Cons::OwnDescValues { dst, key } => {
                let r = self.reason("getOwnPropertyDescriptor(s) of a Proxy");
                self.proxy_own_props(p, r);
                if !self.proxy_made(p, ProxyOp::DescValues(dst)) {
                    let d = self.proxies[p as usize].ops[&ProxyOp::OwnDesc];
                    let a = self.atom("value");
                    self.cons(
                        d,
                        Cons::Load {
                            key: Key::Named(a),
                            dst,
                        },
                    );
                    let t = self.proxies[p as usize].target;
                    self.cons(t, Cons::OwnDescValues { dst, key });
                }
            }
            // Rule PXOWN: the `get` (or `set`) of those descriptors: the
            // trap result's field (ToPropertyDescriptor 6.2.6.5 reads it,
            // FromPropertyDescriptor hands it out), and the forward, the
            // target's own accessors.
            Cons::OwnDescAcc { setter, dst } => {
                let r = self.reason("getOwnPropertyDescriptor(s) of a Proxy");
                self.proxy_own_props(p, r);
                if !self.proxy_made(p, ProxyOp::DescAcc(setter, dst)) {
                    let d = self.proxies[p as usize].ops[&ProxyOp::OwnDesc];
                    let a = self.atom(if setter { "set" } else { "get" });
                    self.cons(
                        d,
                        Cons::Load {
                            key: Key::Named(a),
                            dst,
                        },
                    );
                    let t = self.proxies[p as usize].target;
                    self.cons(t, Cons::OwnDescAcc { setter, dst });
                }
            }
            // Rule PXOWN: CopyDataProperties( target, p, excluded ) (7.3.25:
            // `{ ...p }`, object rest; Object.assign 20.1.2.1 the same reads):
            // p.[[OwnPropertyKeys]]() (`ownKeys`), p.[[GetOwnProperty]]( k )
            // of each key (`getOwnPropertyDescriptor`) and Get( p, k ) of each
            // enumerable one (`get`, Receiver p), stored under k, a String or
            // a Symbol, on each target: a computed-key store at the copy's
            // site (a definition for a spread, a [[Set]] for Object.assign).
            // No key is surely copied.
            Cons::Spread { into, site } => {
                let r = self.reason("CopyDataProperties from a Proxy");
                self.proxy_own_props(p, r);
                if let Some(&tm) = self.lit_spread_site.get(&site) {
                    self.spread_source(tm, site, Vec::new());
                }
                if !self.proxy_made(p, ProxyOp::Copy(into, site)) {
                    let g = self.proxy_get(p, Key::any(), NO_NODE);
                    let region = self.thrown;
                    self.cons(
                        into,
                        Cons::Store {
                            key: Key::any(),
                            src: g,
                            site,
                            region,
                        },
                    );
                }
            }
            // Rule PXOWN: ObjectDefineProperties( O, p ) (20.1.2.3.1) with the
            // Proxy as the descriptors map: p.[[OwnPropertyKeys]]() (`ownKeys`),
            // p.[[GetOwnProperty]]( k ) of each key (`getOwnPropertyDescriptor`)
            // and Get( p, k ) of each enumerable one (`get`, Receiver p): the
            // descriptor objects (`descs`, which generation reads as
            // ToPropertyDescriptor does), each defined on O under k, a
            // computed key (`define_any_on`: E5 on O, the value stored under
            // any key).
            Cons::DefineAll {
                o,
                descs,
                value,
                site,
                reason,
            } => {
                self.proxy_own_props(p, reason);
                let g = self.proxy_get(p, Key::any(), NO_NODE);
                self.edge(g, descs);
                self.define_any_on(o, descs, value, site, reason);
            }
            // Rule PXOWN: Array.from( p ) (23.1.2.1): GetMethod( p, @@iterator )
            // (step 4, the `get` trap; the iteration itself is the iteration
            // rule's); a nullish method reads p as an array-like (step 5):
            // LengthOfArrayLike (Get( p, "length" ), ToLength) and Get( p, k )
            // of each index (the `get` trap, Receiver p), into `dst`.
            Cons::FromArrayLike { dst } => {
                if !self.proxy_made(p, ProxyOp::FromLike(dst)) {
                    let wk = self.atom(builtins::WK_ITERATOR);
                    let it = self.proxy_get(p, Key::Named(wk), NO_NODE);
                    let src = self.node_l("proxy-from-source");
                    let t = self.tok(TokKind::Proxy(p));
                    self.add_tok(src, t);
                    let like = self.node_l("proxy-from-array-like");
                    self.cons(
                        it,
                        Cons::Guard {
                            test: guard::NULLISH,
                            from: src,
                            dst: like,
                        },
                    );
                    let la = self.atom("length");
                    let len = self.node_l("proxy-from-length");
                    self.cons(
                        like,
                        Cons::Load {
                            key: Key::Named(la),
                            dst: len,
                        },
                    );
                    let r = self.reason("ToLength of an Array.from array-like length");
                    let region = self.thrown;
                    self.cons(
                        len,
                        Cons::Implicit {
                            reason: r,
                            proto: Protocol::ToPrimitive,
                            region,
                        },
                    );
                    let ek = self.elem_key();
                    self.cons(like, Cons::Load { key: ek, dst });
                }
            }
            // Rule PXBIRTH: a literal's birth in a summarized body (13.2.5.5
            // / 13.2.4.1 CreateDataPropertyOrThrow on the new literal; the
            // reviver's CreateDataProperty on a parse-made object) writes its
            // own template's tokens only (`apply`): a Proxy the holder node
            // also holds is a foreign value of it. No internal method of the
            // Proxy runs: no [[DefineOwnProperty]] (10.5.6), no trap, no
            // forward, whatever the handler is.
            Cons::BirthField { .. } => {}
            // Rule PXENUMV: ENUM3's `EnumKeyAny` (`enum_key_watch`) watches
            // the values the enumerated object holds under keys no name
            // lists, for presence only (`apply`): once one is there, the
            // copy stores under some String key. A Proxy among them is a
            // value of the object. EnumerateObjectProperties (14.7.5.9) and
            // [[OwnPropertyKeys]] run on the enumerated object, never on its
            // property values: no internal method of the Proxy runs, no
            // trap, nothing escapes. A Proxy subject keeps its own rules.
            Cons::EnumKeyAny { o, src, site } => self.enum_key_any(o, src, site),
            // Rule PXESC: the rules with no Proxy model yet, each named (a new
            // rule must take a place in this match). The operation may run
            // any internal method of the Proxy: its escape, the rule named in
            // the reason (`proxy_escape`: each trap with unknown operands,
            // the target's own escape), and the rule's result is unknown.
            Cons::Accessor { .. }
            | Cons::AccessorKeyed { .. }
            | Cons::AddsKeys { .. }
            | Cons::AnnexBDefine { .. }
            | Cons::AnnexBLookup { .. }
            | Cons::BoundCall { .. }
            | Cons::CallerValue { .. }
            | Cons::CjsPublish { .. }
            | Cons::ContractThis { .. }
            | Cons::DefineDesc { .. }
            | Cons::EnumKeyStore { .. }
            | Cons::EnumLoad { .. }
            | Cons::ForInAcc { .. }
            | Cons::ForInBody { .. }
            | Cons::Fulfil { .. }
            | Cons::HandOver { .. }
            | Cons::IdxOwnLoad { .. }
            | Cons::Miss { .. }
            | Cons::NativeParent { .. }
            | Cons::NativeThis { .. }
            | Cons::NewNamed { .. }
            | Cons::NotBuffer { .. }
            | Cons::NotRegExp { .. }
            | Cons::OverrideThis { .. }
            | Cons::OwnComputedKey { .. }
            | Cons::OwnGate { .. }
            | Cons::OwnOnly { .. }
            | Cons::OwnTest { .. }
            | Cons::ProtoKey { .. }
            | Cons::ProtoKeySet { .. }
            | Cons::RegExpPattern { .. }
            | Cons::RequireSpec { .. }
            | Cons::ShareBuffer { .. }
            | Cons::StoreKeyed { .. }
            | Cons::StoreOut { .. }
            | Cons::SuperCtor { .. }
            | Cons::SuperCtorParent { .. }
            | Cons::ThisFrom { .. }
            | Cons::ViewedBuffer { .. }
            | Cons::VisitHolders { .. }
            | Cons::VisitStop { .. } => {
                let r = self.reason(&format!("Proxy operand of rule {} (no Proxy model)", c.kind_name()));
                let t = self.tok(TokKind::Proxy(p));
                self.escape(t, r);
                self.apply_inner(n, c, Tokens::Slice(&[TOP]), 0);
            }
        }
    }
}
