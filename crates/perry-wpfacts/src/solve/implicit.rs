//! Implicit protocol touches (`instanceof`, ToPrimitive, JSON, the console
//! formatter): the methods a value's protocols run, and where they throw.

use super::*;

impl Model {
    /// InstanceofOperator on `t` with left-hand side `lhs`: the protocol
    /// touch, its method called with `lhs`. Where the method is not
    /// modeled (⊤, a marker's run, a bound function's target), `lhs` is
    /// passed to unknown code: it escapes.
    pub fn has_instance(&mut self, t: Tok, reason: u32, lhs: NodeId) {
        self.instanceof_walk(lhs);
        let unknown =
            t == TOP || self.is_marker(t) || matches!(self.toks[t as usize], TokKind::Bound(_));
        if unknown {
            self.sink_node(lhs, reason);
            match self.toks[t as usize] {
                // `instanceof` a bound function runs its target's.
                TokKind::Bound(_) => self.escape(t, reason),
                _ => self.implicit(t, reason, Protocol::HasInstance, self.thrown),
            }
            return;
        }
        let arg = self.has_instance_node(t);
        self.edge(lhs, arg);
        self.implicit(t, reason, Protocol::HasInstance, self.thrown);
    }

    /// OrdinaryHasInstance (7.3.22 steps 5-6) walks the left side's chain
    /// with O.[[GetPrototypeOf]]() until it meets C.prototype or null. An
    /// ordinary link runs no code there; a Proxy (the value itself, or one
    /// on its chain) runs its `getPrototypeOf` trap and the forward
    /// (10.5.1, `Cons::ProtoOf` on it), and the walk goes on from what that
    /// returns: the walk node holds every link (`ProtoOf` of the left side
    /// and of the walk node itself). Made for every left side that meets
    /// InstanceofOperator, once per node: a superset where `@@hasInstance`
    /// is another function than Function.prototype's, which walks nothing.
    pub(crate) fn instanceof_walk(&mut self, lhs: NodeId) {
        if !self.idx_done.insert((lhs, TOP, lhs, IDX_INSTANCEOF_WALK)) {
            return;
        }
        let w = self.node_l("instanceof-chain");
        self.nodes[w as usize].frame = self.nodes[lhs as usize].frame;
        self.cons(lhs, Cons::ProtoOf { dst: w });
        self.cons(w, Cons::ProtoOf { dst: w });
    }

    /// The argument node of the `@@hasInstance` call on `t`, one per token.
    pub(super) fn has_instance_node(&mut self, t: Tok) -> NodeId {
        if let Some(&a) = self.has_instance_arg.get(&t) {
            return a;
        }
        let a = self.node_l("hasinstance-arg");
        self.nodes[a as usize].frame = self.fam_home(t);
        self.has_instance_arg.insert(t, a);
        a
    }

    /// JSON's touch of `t` reads its own values only (`implicit`): a callable
    /// (never descended) or an object whose own properties the solver
    /// models (`own_props_modelled`); any other keeps the any-key read.
    pub(super) fn json_reads_own(&self, t: Tok) -> bool {
        t != TOP && (self.is_callable_tok(t) || self.own_props_modelled(t))
    }

    /// `t` is callable (IsCallable, 7.2.3): a function, class, bound or
    /// builtin function value.
    pub(super) fn is_callable_tok(&self, t: Tok) -> bool {
        matches!(
            self.toks[t as usize],
            TokKind::Func(..) | TokKind::ClassObj(_) | TokKind::Bound(_) | TokKind::BuiltinFn(_)
        )
    }

    /// The throw region of touch `proto` on `t`, fed into `region`: `None`
    /// when the touch is already made (its region then feeds `region` too),
    /// else the new region its calls and nested touches throw into.
    pub(crate) fn implicit_throw(
        &mut self,
        t: Tok,
        proto: Protocol,
        region: NodeId,
    ) -> Option<NodeId> {
        if self.stop_top.is_some() {
            let p = self.stop_top.as_ref().unwrap().cur_pos();
            if p == crate::topstop::NOPOS {
                self.note_nopos(std::panic::Location::caller(), "implicit-request");
            }
            let what = || format!("{proto:?} touch on {}", self.tok_words(t));
            let w = if self
                .stop_top
                .as_ref()
                .unwrap()
                .shared_key
                .contains_key(&(t, proto))
            {
                String::new()
            } else {
                what()
            };
            let s = self.stop_top.as_mut().unwrap();
            let id = s.shared_call((t, proto), || w, p);
            if !self.implicit_done.contains_key(&(t, proto)) {
                // `implicit` builds the touch's nodes and rules under it.
                s.shared_pos = (crate::topstop::SHARED, id);
            }
        }
        if let Some(&throw) = self.implicit_done.get(&(t, proto)) {
            self.edge(throw, region);
            return None;
        }
        let throw = self.node_l("implicit-throw");
        self.implicit_done.insert((t, proto), throw);
        self.edge(throw, region);
        Some(throw)
    }

    /// Builtin protocol touch on `t` (ToPrimitive, iteration, toJSON, then,
    /// inspect …): every function reachable as a property of `t` or its
    /// chain runs with `this` = `t` and unknown primitive arguments, or for
    /// `@@hasInstance` the `instanceof` left-hand sides (`has_instance`).
    /// What those calls throw completes the operation that asked for the
    /// touch (the runtime catches nothing: a throw from a console
    /// argument's `toJSON` or `[util.inspect.custom]` completes that
    /// `console.log`), so it reaches `region`: one `implicit-throw` node per
    /// `(t, proto)` is the region of the touch's calls and of the touches it
    /// runs in turn, and feeds the region of every request.
    #[track_caller]
    pub fn implicit(&mut self, t: Tok, reason: u32, proto: Protocol, region: NodeId) {
        self.shared_scope(|m| m.implicit_touch(t, reason, proto, region));
    }

    /// Rule R2 (perry-runtime builtins/formatting.rs): how the formatter
    /// treats the depth of value `t`.
    fn inspect_shape(&self, t: Tok) -> InspectShape {
        match &self.toks[t as usize] {
            // GC_TYPE_CLOSURE (formatting.rs:820, format_function_for_console
            // / format_class_for_console): no limit check, own properties at
            // depth 1. `Object.prototype` stands for every builtin prototype,
            // `Function.prototype` among them; a builtin namespace whose chain
            // is not `Object.prototype` is a constructor or function.
            TokKind::Func(..)
            | TokKind::ClassObj(_)
            | TokKind::Bound(_)
            | TokKind::BuiltinFn(_)
            | TokKind::ObjectProto => InspectShape::Function,
            TokKind::Builtin(ns) if !builtins::ns_is_ordinary(*ns) => InspectShape::Function,
            TokKind::NativeProto(name, _) if name == "Function" => InspectShape::Function,
            // GC_TYPE_ERROR (formatting.rs:720, errors.rs:90), a Promise
            // (:821) and a weak collection (`format_weak_wrapper`, :803):
            // members at depth + 1 with no limit check.
            TokKind::Obj(tm, _) => match self.tmpls[*tm as usize].kind {
                TmplKind::BObj {
                    what: "Error" | "Promise" | "WeakMap" | "WeakSet",
                    ..
                } => InspectShape::Unchecked,
                _ => InspectShape::Ordinary,
            },
            _ => InspectShape::Ordinary,
        }
    }

    /// The protocols run on what touch `proto` reads, an inspection's at
    /// `shown` (rule R2).
    pub(super) fn touches_at(proto: Protocol, shown: Option<InspectAt>) -> Vec<Protocol> {
        proto
            .read_touches()
            .iter()
            .map(|&p| match (p, shown) {
                (Protocol::InspectShown(_), Some(at)) => Protocol::InspectShown(at),
                _ => p,
            })
            .collect()
    }

    /// A console call's formatter request on node `n` (rule R2): shown
    /// (`InspectShown`) or consumable by a specifier (`Inspect`), at depth 0
    /// under the call's own limit `floor`. An `ambient` call sets no limit
    /// of its own: it runs under the limit of the extent it runs in, at
    /// least `floor` (`inspect_raised`).
    pub(crate) fn console_inspect(
        &mut self,
        n: NodeId,
        reason: u32,
        shown: bool,
        region: NodeId,
        floor: u8,
        ambient: bool,
    ) {
        let mut limit = floor;
        if ambient {
            self.inspect_reqs.push((n, reason, shown, region, floor));
            limit = limit.max(self.inspect_raised);
        }
        self.console_inspect_at(n, reason, shown, region, limit);
    }

    pub(super) fn console_inspect_at(
        &mut self,
        n: NodeId,
        reason: u32,
        shown: bool,
        region: NodeId,
        limit: u8,
    ) {
        let at = InspectAt::arg(limit.max(INSPECT_DEFAULT_LIMIT));
        let proto = if shown {
            Protocol::InspectShown(at)
        } else {
            Protocol::Inspect(at)
        };
        self.cons(
            n,
            Cons::Implicit {
                reason,
                proto,
                region,
            },
        );
    }

    /// A touch under raised limit `limit` may run program code (a hook, a
    /// getter): a console call that code makes formats under `limit`
    /// (`InspectDepthLimitGuard` holds for the dynamic extent). Every
    /// ambient console request is re-issued under it (monotone).
    pub(super) fn raise_inspect_limit(&mut self, limit: u8) {
        let limit = limit.min(INSPECT_HARD_CAP);
        if limit <= self.inspect_raised.max(INSPECT_DEFAULT_LIMIT) {
            return;
        }
        self.inspect_raised = limit;
        let reqs = self.inspect_reqs.clone();
        for (n, reason, shown, region, floor) in reqs {
            if floor < limit {
                self.console_inspect_at(n, reason, shown, region, limit);
            }
        }
    }

    #[track_caller]
    pub(super) fn implicit_touch(&mut self, t: Tok, reason: u32, proto: Protocol, region: NodeId) {
        // A Proxy's ToPrimitive runs its [[Get]]s (`proxy_to_primitive`),
        // the formatter shows it running no trap (`proxy_inspect_shown`),
        // JSON serializes it through its internal methods (`proxy_json`).
        if let TokKind::Proxy(p) = self.toks[t as usize] {
            match proto {
                Protocol::ToPrimitive => return self.proxy_to_primitive(p, reason, region),
                Protocol::InspectShown(at) => {
                    return self.proxy_inspect_shown(p, at, reason, region)
                }
                Protocol::Json => return self.proxy_json(p, reason, region),
                Protocol::Inspect(at) => return self.proxy_inspect(p, at, reason, region),
                _ => {}
            }
        }
        // Rule R2: the formatter reads a value only down to the inspect
        // depth limit (`InspectAt`): past the hard cap nothing; an ordinary
        // object past the limit prints `[Object]` with no read and no hook
        // lookup; a function's own values are at depth 1; an Error's, a
        // Promise's and a weak collection's are read with no limit check.
        let shown = match proto.inspect_at() {
            None => None,
            Some(at) => {
                if at.depth > INSPECT_HARD_CAP {
                    return;
                }
                let next = if t == TOP || self.is_marker(t) {
                    at.member()
                } else {
                    match self.inspect_shape(t) {
                        InspectShape::Function => at.fn_member(),
                        InspectShape::Unchecked => at.member(),
                        InspectShape::Ordinary if at.depth > at.limit => return,
                        InspectShape::Ordinary => at.member(),
                    }
                };
                if at.limit > INSPECT_DEFAULT_LIMIT && t != TOP {
                    self.raise_inspect_limit(at.limit);
                }
                Some(next)
            }
        };
        // I32 at rest (PR B): a protocol touch reads elements undecoded.
        self.rest_used(t);
        // The formatter's `%j` serializes the ARGUMENT (JSON.stringify,
        // 25.5.2.1): its own values are serialized by that touch's own
        // reads, never a value only the formatter shows (a chain member,
        // a class's `prototype`).
        if matches!(proto, Protocol::Inspect(_)) {
            self.implicit(t, reason, Protocol::Json, region);
        }
        // The touch looks up and calls the protocol's keys and reads its
        // values, nothing else.
        let keys: Option<Vec<Atom>> = proto
            .keys()
            .map(|ks| ks.iter().map(|k| self.atom(k)).collect());
        let mut reads: Vec<Key> = proto
            .reads()
            .iter()
            .map(|k| Key::Named(self.atom(k)))
            .collect();
        // JSON (25.5.2.2 SerializeJSONProperty): after the `toJSON` lookup
        // (a chain [[Get]], `keys`), a callable value serializes as
        // `undefined` and is never descended (step 10); any other object
        // serializes its ENUMERABLE OWN properties (25.5.2.5 / 25.5.2.6 via
        // EnumerableOwnProperties, 7.3.23): never its chain (`constructor`,
        // methods) nor a non-enumerable own one (a function's `prototype`).
        // `json_own`: those values (`Cons::OwnValues`, getters of own
        // accessors run) where the solver models the own properties.
        let mut json_own = false;
        if proto.reads_all() {
            if proto == Protocol::Json && self.json_reads_own(t) {
                json_own = !self.is_callable_tok(t);
            } else {
                reads.push(Key::any());
            }
        }
        let calls = keys.as_ref().is_none_or(|k| !k.is_empty());
        if !calls && reads.is_empty() && !json_own {
            return;
        }
        // `PERRY_WPF_IMPLSRC=1`: one stderr line per protocol request on `t`
        // (`keys` `None`: any method may run), naming the requesting rule
        // (the escape ledger's reason), the protocol, the token and its
        // kind; a second line names the implicit call the request opens.
        if implsrc_on() {
            let kind = format!("{:?}", self.toks[t as usize]);
            eprintln!(
                "perry: wpfacts implsrc: tok={t} marker={} done={} proto={proto:?} reason={:?} name={:?} kind={}",
                self.is_marker(t),
                self.implicit_done.contains_key(&(t, proto)),
                self.reasons[reason as usize],
                self.tok_name(t),
                &kind[..kind.len().min(80)]
            );
        }
        self.mark_use(t, proto.touch_bit());
        // A marker: the protocol runs on the values beside it; on the
        // marker it calls what the marker reaches under the protocol's
        // keys, an escape of that (`method_on`, DECISIONS 111). It has no
        // accessors, so its reads run nothing; what JSON serializes in turn
        // is what it reaches, touched (DECISIONS 45).
        if self.is_marker(t) {
            match &keys {
                None => {
                    let r = self.reach_of(t);
                    self.mark_escape_why(r, &|_| "protocol: on a marker".into());
                }
                Some(ks) => {
                    let inst: Vec<Atom> = proto
                        .instance_of_reads()
                        .iter()
                        .map(|k| self.atom(k))
                        .collect();
                    for &a in ks.iter().chain(&inst) {
                        let m = self.marker_load(t, Some(a));
                        self.mark_escape_why(m, &|_| "protocol: on a marker".into());
                    }
                }
            }
            if proto.reads_all() {
                let r = self.reach_of(t);
                self.mark_use(r, crate::model::MARK_TOUCH);
            }
            return;
        }
        if t == TOP {
            return;
        }
        let Some(throw) = self.implicit_throw(t, proto, region) else {
            return;
        };
        // The protocol calls on a marker family's token are of its run
        // (`call_run`): what they find on it is the family's.
        let home = self.fam_home(t);
        // The values the protocol reads (getters run in the load) and, for
        // JSON, serializes in turn.
        for key in reads {
            let recv = self.node_l("implicit-recv");
            let dst = self.node_l("implicit-read");
            self.nodes[recv as usize].frame = home;
            self.nodes[dst as usize].frame = home;
            self.nodes[dst as usize].sink = true;
            self.cons(recv, Cons::Load { key, dst });
            for proto in Self::touches_at(proto, shown) {
                self.cons(
                    dst,
                    Cons::Implicit {
                        reason,
                        proto,
                        region: throw,
                    },
                );
            }
            self.add_tok(recv, t);
        }
        if json_own {
            let recv = self.node_l("implicit-recv");
            let dst = self.node_l("implicit-read");
            self.nodes[recv as usize].frame = home;
            self.nodes[dst as usize].frame = home;
            self.nodes[dst as usize].sink = true;
            self.cons(recv, Cons::OwnValues { dst });
            for proto in Self::touches_at(proto, shown) {
                self.cons(
                    dst,
                    Cons::Implicit {
                        reason,
                        proto,
                        region: throw,
                    },
                );
            }
            self.add_tok(recv, t);
        }
        // An Error's `errors` elements (errors.rs:77 format_error_array) are
        // shown one level below the array, with no limit check either.
        if let (Some(at), true) = (shown, self.native_error_tok(t)) {
            let key = Key::Named(self.atom("errors"));
            let ek = self.elem_key();
            let recv = self.node_l("implicit-recv");
            let arr = self.node_l("implicit-read");
            let dst = self.node_l("implicit-read");
            for n in [recv, arr, dst] {
                self.nodes[n as usize].frame = home;
            }
            self.nodes[arr as usize].sink = true;
            self.nodes[dst as usize].sink = true;
            self.cons(recv, Cons::Load { key, dst: arr });
            self.cons(arr, Cons::Load { key: ek, dst });
            self.cons(
                dst,
                Cons::Implicit {
                    reason,
                    proto: Protocol::InspectShown(at.member()),
                    region: throw,
                },
            );
            self.add_tok(recv, t);
        }
        // `t instanceof C` for the constructors on its chain: `C`'s
        // `@@hasInstance` runs with `t` (`has_instance`).
        for k in proto.instance_of_reads() {
            let key = Key::Named(self.atom(k));
            let recv = self.node_l("implicit-recv");
            let dst = self.node_l("implicit-ctor");
            let lhs = self.node_l("implicit-lhs");
            for n in [recv, dst, lhs] {
                self.nodes[n as usize].frame = home;
            }
            self.nodes[dst as usize].sink = true;
            self.cons(recv, Cons::Load { key, dst });
            self.cons(dst, Cons::HasInstance { reason, arg: lhs });
            self.add_tok(lhs, t);
            self.add_tok(recv, t);
        }
        if !calls {
            return;
        }
        // JSON-RT reads what `toJSON` returns (`json_rt_ret`).
        let ret = match proto {
            Protocol::Json => self.json_rt_ret(t),
            _ => self.node_named("implicit-ret", || format!("implicit-ret({t})")),
        };
        if proto == Protocol::ToPrimitive {
            self.to_prim_ret.insert(t, ret);
            self.to_prim_ret_nodes.insert(ret);
        }
        // Protocol results go back to the runtime only (touched below);
        // an awaited one is adopted first (below).
        self.nodes[ret as usize].sink = !proto.awaits_result();
        self.nodes[ret as usize].frame = home;
        // Protocol arguments are primitives (hints, keys), except
        // `@@hasInstance`'s, the `instanceof` left-hand sides. The
        // one protocol that hands user code runtime functions,
        // `then(resolve, reject)`, is modeled where it happens
        // (`builtins::adopt`): the resolvers there are synthetic functions
        // whose argument reaches the result.
        let args = match proto {
            Protocol::HasInstance => vec![self.has_instance_node(t)],
            _ => {
                let arg = self.node_l("implicit-arg");
                self.nodes[arg as usize].frame = home;
                self.add_prim(arg, prim::ALL);
                vec![arg; 4]
            }
        };
        let arg = args[0];
        let call = self.calls.len() as CallId;
        self.calls.push(CallSite {
            first_arg_fixed: false,
            this: ThisArg::Tok(t),
            args,
            spread: vec![],
            ret,
            site: 0,
            region: throw,
        });
        self.protocol_calls.insert(call);
        self.why_mint(call, Some(t), reason);
        self.rel(|| format!("implicitcall {call} {t} {ret} {arg}"));
        if implsrc_on() {
            eprintln!(
                "perry: wpfacts implsrc-call: call={call} tok={t} proto={proto:?} reason={:?}",
                self.reasons[reason as usize]
            );
        }
        // A Proxy's protocol keys are its [[Get]]s (10.5.8, `proxy_get`),
        // each value called with `this` = the Proxy; OrdinaryHasInstance
        // (7.3.21 step 4) also reads Get( p, "prototype" ), a value compared
        // only.
        if let TokKind::Proxy(p) = self.toks[t as usize] {
            let mut ks: Vec<Key> = match &keys {
                None => vec![Key::any()],
                Some(ks) => ks.iter().map(|&a| Key::Named(a)).collect(),
            };
            if proto == Protocol::HasInstance {
                ks.push(Key::Named(self.atom("prototype")));
            }
            for key in ks {
                let f = self.proxy_get(p, key, NO_NODE);
                self.cons(f, Cons::Call { call });
            }
        } else {
            match keys {
                None => self.method_on(t, Key::any(), call),
                Some(ks) => {
                    for a in ks {
                        self.method_on(t, Key::Named(a), call);
                    }
                }
            }
        }
        // Results of protocol methods flow back to the runtime: what the
        // protocol runs on them (an iterator's `next`, a result's `value`,
        // what `toJSON` returns) is a touch of its own.
        if let Some(next) = proto.of_result() {
            let settled = if proto.awaits_result() {
                let dst = self.node_l("implicit-settled");
                self.nodes[dst as usize].sink = true;
                self.nodes[dst as usize].frame = home;
                crate::builtins::adopt(self, call, ret, dst);
                dst
            } else {
                ret
            };
            self.cons(
                settled,
                Cons::Implicit {
                    reason,
                    proto: next,
                    region: throw,
                },
            );
        }
    }
}

/// `PERRY_WPF_IMPLSRC=1` (`Model::implicit`).
pub(crate) fn implsrc_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PERRY_WPF_IMPLSRC").is_some())
}

/// How the runtime's formatter treats a value's depth (rule R2,
/// `Model::inspect_shape`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum InspectShape {
    /// Read only at a depth within the limit; members one level deeper.
    Ordinary,
    /// Read at any depth within the hard cap; own values at depth 1.
    Function,
    /// Read at any depth within the hard cap; members one level deeper.
    Unchecked,
}
