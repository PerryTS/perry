//! Switches of the activation-reach rules: each names a call edge or a
//! lookup the solve makes so that a body the program can enter is never
//! called inactive (`BodyReach::inactive`). Off restores the solve without
//! the rule.

use crate::gen::{Fx, Gen};
use crate::model::{
    prim, CallId, CallSite, Cons, IdentityRoute, Key, Model, NodeId, Protocol, ThisArg, TmplId, Tok,
};
use perry_hir::Expr;

/// `PERRY_FACTS_REACH_FOLDS=0`: a name-keyed fold (`Expr::WeakRefDeref`,
/// `Expr::FinalizationRegistryRegister` / `Unregister`) is its intrinsic
/// only, not also the ordinary method call its runtime helper makes on a
/// receiver of another kind (`Gen::name_fold_call`).
pub(crate) fn name_folds() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_REACH_FOLDS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `PERRY_FACTS_REACH_NEW_TARGET=0`: `Reflect.construct(T, args, N)` whose
/// result the model holds as unknown (a built-in `T`, a function `N`)
/// leaves what `N.prototype` holds where it is
/// (`Model::new_target_unknown`).
pub(crate) fn new_target() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_REACH_NEW_TARGET").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}
/// `PERRY_FACTS_REACH_STEP_DONE=0`: an iteration consumer reads only the
/// value of a step result, not its `done` first (`Model::step_done`).
pub(crate) fn step_done() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_REACH_STEP_DONE").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `PERRY_FACTS_REACH_SPREAD_ARGS=0`: a hand-written built-in rule reads
/// its arguments by position among the direct arguments only; what a
/// spread argument supplies reaches no position (`Model::positional`).
pub(crate) fn spread_args() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_REACH_SPREAD_ARGS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `PERRY_FACTS_REACH_ITER_CLOSE=0`: a consumer that can stop before its
/// iterator is done (`Array.from` with a mapper, a collection constructor,
/// `Object.fromEntries`, `groupBy`, `Promise.all` ...) never calls the
/// iterator's `return` (`Gen::iterate_closed`, `builtins::iterate`).
pub(crate) fn iter_close() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_REACH_ITER_CLOSE").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `PERRY_FACTS_REACH_GEN_MEMBERS=0`: a generator object has `next`,
/// `return` and `throw` only; `[Symbol.iterator]`, the iterator helpers
/// and a value read of any of them find nothing
/// (`builtins::gen_members`).
pub(crate) fn gen_members() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_REACH_GEN_MEMBERS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `PERRY_FACTS_REACH_THENABLE=0`: a promise's resolve function, `await`
/// and an adoption (`Promise.resolve`, a then-callback's result) copy a
/// thenable as it is; the resolve function calls no `then`
/// (`Cons::Fulfil`, `builtins::resolver`, `Gen::awaited`).
pub(crate) fn thenable() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_REACH_THENABLE").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// `PERRY_FACTS_REACH_JSON_REPLACER=0`: a `JSON.stringify` replacer
/// function is only escaped (called by unknown code with unknown
/// arguments); the values it is called with, and `this` = their holders,
/// are not its arguments (`Model::json_replacer_calls`). The sabotage
/// `noreachjsonreplacer` is the same.
pub(crate) fn json_replacer(sabotage: Option<&str>) -> bool {
    sabotage != Some("noreachjsonreplacer")
        && !matches!(
            std::env::var("PERRY_FACTS_REACH_JSON_REPLACER").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
}

/// `PERRY_FACTS_REACH_HELD_KEYS=0`: a function a host object that is no
/// node module holds (`Temporal.Duration`, `Intl.NumberFormat`,
/// `WebAssembly.Module`) declares no own key: its `prototype` and its
/// statics read as absent, so what the program stores or defines through
/// `F.prototype` has no receiver and goes nowhere (the realm table's `held`
/// rows, `Declarations::build`). The sabotage `noreachheldkeys` is the
/// same.
pub(crate) fn held_keys(sabotage: Option<&str>) -> bool {
    sabotage != Some("noreachheldkeys")
        && !matches!(
            std::env::var("PERRY_FACTS_REACH_HELD_KEYS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
}

/// `PERRY_FACTS_REACH_PROTO_ACCESSORS=0`: an accessor the program installs
/// on a built-in prototype runs only for the receivers whose lookup the
/// solve walks; a read or a write on a primitive or on an unknown value
/// calls none (`Cons::Accessor` on `ObjectProto`). The sabotage
/// `noreachprotoaccessors` is the same.
pub(crate) fn proto_accessors(sabotage: Option<&str>) -> bool {
    sabotage != Some("noreachprotoaccessors")
        && !matches!(
            std::env::var("PERRY_FACTS_REACH_PROTO_ACCESSORS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
}

/// The switches a model reads during the solve, set before generation.
/// Without the `held` rows the realm table is rebuilt as it was before
/// them.
pub(crate) fn configure(m: &mut Model, sabotage: Option<&str>) {
    m.reach_json_replacer = json_replacer(sabotage);
    m.reach_proto_accessors = proto_accessors(sabotage);
    if !held_keys(sabotage) {
        m.declarations = crate::builtins::realm_shapes::Declarations::build(false);
    }
}

/// The nodes of one `JSON.stringify` call with a replacer
/// (`Model::json_replacer_calls`).
pub(crate) struct JsonReplacerNodes {
    /// Every value the serialization visits.
    pub seen: NodeId,
    /// The values read off them.
    pub read: NodeId,
    /// The holders: `this` of each replacer call.
    pub this: NodeId,
    /// The key argument.
    pub key: NodeId,
}

impl Model {
    /// SerializeJSONProperty, SerializeJSONObject and SerializeJSONArray
    /// (ECMA-262 25.5.2.2-25.5.2.5) with a replacer function: the replacer
    /// is called with `this` = the holder and (key, value) for the root,
    /// whose holder is a fresh object `{"": value}` (`holder`), and then for
    /// every property of every object and every element of every array
    /// the serialization visits; `value` is what `toJSON(key)` returned
    /// where the value has one, and what the replacer returns replaces the
    /// value and is visited in turn. So every value reachable from `value`
    /// through properties and elements (getters run), every `toJSON`
    /// result and every replacer result is a value the replacer receives,
    /// and every object among them a holder it receives as `this`: it may
    /// store into them, delete or add keys, or keep them. `n.seen` holds
    /// them all (a closure under any-key reads); the runtime serializes
    /// each (`Protocol::Json`). The caller makes the two calls
    /// (`toJSON(key)` on `n.seen` and the replacer's with `this` = `n.this`
    /// and arguments `[n.key, n.seen]`) and sends both results to `n.seen`.
    pub(crate) fn json_replacer_calls(
        &mut self,
        value: NodeId,
        holder: Tok,
        n: &JsonReplacerNodes,
    ) {
        self.edge(value, n.seen);
        let key = Key::any();
        self.cons(n.seen, Cons::Load { key, dst: n.read });
        self.edge(n.read, n.seen);
        let root = self.field_any(holder);
        self.edge(value, root);
        self.add_tok(n.this, holder);
        self.edge(n.seen, n.this);
        self.add_prim(n.key, prim::STR);
        let reason = self.reason("JSON.stringify value");
        self.cons(
            n.seen,
            Cons::Implicit {
                reason,
                proto: Protocol::Json,
            },
        );
    }

    /// `json_replacer_calls` for a call of the built-in `JSON.stringify`
    /// value (`builtins::call_builtin`; an alias, a spread argument list,
    /// `.call`): arguments by position (`positional`), the root's holder
    /// `holder` made per call site.
    pub(crate) fn json_replacer_builtin_call(&mut self, call: CallId, holder: Tok) {
        let c = self.positional(call, 2);
        let (Some(&value), Some(&replacer)) = (c.args.first(), c.args.get(1)) else {
            return;
        };
        let frame = self.call_run(call);
        let node = |m: &mut Model, l: &'static str| {
            let x = m.node_l(l);
            m.nodes[x as usize].frame = frame;
            x
        };
        let n = JsonReplacerNodes {
            seen: node(self, "json-seen"),
            read: node(self, "json-seen-read"),
            this: node(self, "json-holder"),
            key: node(self, "json-key"),
        };
        self.json_replacer_calls(value, holder, &n);
        let to_json = Key::Named(self.atom("toJSON"));
        for (this, args, callee) in [
            (ThisArg::Node(n.seen), vec![n.key], None),
            (ThisArg::Node(n.this), vec![n.key, n.seen], Some(replacer)),
        ] {
            let ret = node(self, "json-call-ret");
            let id = self.calls.len() as CallId;
            let thrown = self.thrown;
            self.solver_call(
                call,
                CallSite {
                    this,
                    args,
                    spread: vec![],
                    ret,
                    site: id,
                    region: thrown,
                },
            );
            match callee {
                None => self.cons(
                    n.seen,
                    Cons::MCall {
                        key: to_json,
                        call: id,
                    },
                ),
                Some(f) => self.cons(f, Cons::Call { call: id }),
            }
            self.edge(ret, n.seen);
        }
    }

    /// The call site of `call` as a hand-written built-in rule reads it by
    /// position (`c.args.first()`, `c.args.get(i)`): its first `n`
    /// positions at least. Without a spread argument, the site itself. A
    /// spread shifts positions and they are not kept (`f(...a, b)` and
    /// `f(b, ...a)` are both `args [b]`, `spread [a]`), so position `i`
    /// holds a direct argument at index `i` or before, or an element a
    /// spread argument supplies: the rule the declared effects use
    /// (`effects::Run::root`, `RealmSlotRoot::Arg`). A callback given as
    /// `xs.forEach(...[cb])` is then the callback the rule calls. `spread`
    /// stays, so a rule can tell that a position may also be absent.
    pub(crate) fn positional(&mut self, call: CallId, n: usize) -> CallSite {
        let mut c = self.calls[call as usize].clone();
        if c.spread.is_empty() || !self.reach_spread_args {
            return c;
        }
        let frame = self.call_run(call);
        let elems = self.node_l("spread-arg-elems");
        self.nodes[elems as usize].frame = frame;
        let key = self.elem_key();
        for &s in &c.spread {
            self.cons(s, Cons::Load { key, dst: elems });
        }
        let direct = std::mem::take(&mut c.args);
        for i in 0..n.max(direct.len()) {
            let a = self.node_l("spread-arg");
            self.nodes[a as usize].frame = frame;
            for &d in direct.iter().take(i + 1) {
                self.edge(d, a);
            }
            self.edge(elems, a);
            c.args.push(a);
        }
        c
    }

    /// `json_reviver_values`: the templates in `fresh` now met a delete,
    /// defineProperty or `with`, or escaped (`note_inst`); a reviver whose
    /// parse made one of them is also given the any-key read of the parse
    /// objects, once.
    pub(crate) fn json_reviver_flush(&mut self, fresh: &[TmplId]) {
        if fresh.is_empty() || self.json_reviver_own.is_empty() {
            return;
        }
        let mut i = 0;
        while i < self.json_reviver_own.len() {
            if self.json_reviver_own[i].0.iter().any(|t| fresh.contains(t)) {
                let (_, parsed, own) = self.json_reviver_own.swap_remove(i);
                let key = Key::any();
                self.cons(parsed, Cons::Load { key, dst: own });
            } else {
                i += 1;
            }
        }
    }

    /// IteratorComplete (ECMA-262 7.4.5) on the step results `result`
    /// holds: every consumer of the iteration protocol (a for-of step,
    /// array destructuring, spread, `yield*`, `Array.from` and the other
    /// built-in consumers) reads `done` of what `next()` returned before
    /// it reads the value, a [[Get]] like any other: a getter there runs.
    /// The value read is ToBoolean'd by the runtime and goes nowhere; the
    /// node returned holds it (`None`: the rule is off).
    pub(crate) fn step_done(&mut self, result: NodeId) -> Option<NodeId> {
        if !self.reach_step_done {
            return None;
        }
        let done = self.atom("done");
        let dst = self.node_l("step-done");
        self.nodes[dst as usize].frame = self.nodes[result as usize].frame;
        let key = Key::Named(done);
        self.cons(result, Cons::Load { key, dst });
        Some(dst)
    }
}

impl Gen<'_> {
    /// `JSON.stringify(value, replacer, ...)` whose replacer is no literal
    /// `null`, `undefined` or array (`ks`: the operands, the replacer
    /// second), with the rule on: the operands as before (the value
    /// serialized and, with value records, its identity observed; the
    /// replacer and `space` escaped), and the replacer's calls
    /// (`Model::json_replacer_calls`). `None`: not such a call.
    pub(crate) fn json_replacer_stringify(
        &mut self,
        fx: &Fx,
        ks: &[&Expr],
        replacer: Option<&Expr>,
        identity: bool,
    ) -> Option<NodeId> {
        let calls_back =
            replacer.is_some_and(|r| !matches!(r, Expr::Null | Expr::Undefined | Expr::Array(..)));
        if !self.m.reach_json_replacer || !calls_back || ks.len() < 2 {
            return None;
        }
        let r = self.m.reason("JSON.stringify value");
        let value = self.expr(fx, ks[0]);
        self.m.cons(
            value,
            Cons::Implicit {
                reason: r,
                proto: Protocol::Json,
            },
        );
        if identity {
            self.m.identity(value, IdentityRoute::JsonReplacer);
        }
        let rr = self.m.reason("JSON.stringify replacer");
        let mut rest = Vec::new();
        for k in &ks[1..] {
            let v = self.expr(fx, k);
            self.m.cons(
                v,
                Cons::Implicit {
                    reason: rr,
                    proto: Protocol::Any,
                },
            );
            rest.push(v);
        }
        let out = self.tmp("primop");
        self.m.add_prim(out, prim::STR | prim::UNDEF);
        let (holder, _) = self.bobj(fx.module, "json");
        let n = JsonReplacerNodes {
            seen: self.tmp("json-seen"),
            read: self.tmp("json-seen-read"),
            this: self.tmp("json-holder"),
            key: self.tmp("json-key"),
        };
        self.m.json_replacer_calls(value, holder, &n);
        let to_json = Key::Named(self.m.atom("toJSON"));
        let tr = self.mcall(n.seen, to_json, vec![n.key], vec![]);
        self.m.edge(tr, n.seen);
        let rr = self.call_value(rest[0], ThisArg::Node(n.this), vec![n.key, n.seen], vec![]);
        self.m.edge(rr, n.seen);
        Some(out)
    }

    /// The values the reviver is called with, the rule on.
    /// InternalizeJSONProperty fixes each holder's key list first
    /// (EnumerableOwnProperties) and then makes an ordinary [[Get]] of each
    /// key, with the reviver running between them with `this` = the holder:
    /// while nothing can delete or redefine a key of the parse objects
    /// (`born..`: the templates `parsed` was made with), each [[Get]] reads
    /// an own data value, so the values are the parse objects' own values
    /// (`Cons::OwnValues`); an any-key read would also read their chain
    /// (`Object.prototype`, its methods), and with the result stored back
    /// (`json_reviver_calls`) those would come round as content. Once a
    /// delete, defineProperty or `with` reaches one of those templates, or
    /// it escapes to unknown code (`note_inst`), a [[Get]] may read the
    /// chain (`delete this.toString` before the walk reaches `toString`)
    /// or run a getter: the any-key read is added
    /// (`Model::json_reviver_flush`, monotone). `None`: the rule is off.
    pub(crate) fn json_reviver_values(&mut self, parsed: NodeId, born: usize) -> Option<NodeId> {
        if !self.m.reach_json_replacer {
            return None;
        }
        let own = self.tmp("json-reviver-values");
        self.m.cons(parsed, Cons::OwnValues { dst: own });
        let tmpls: Vec<TmplId> = (born..self.m.tmpls.len()).map(|t| t as TmplId).collect();
        let lost = tmpls
            .iter()
            .any(|t| self.m.inst_lose.contains(t) || self.m.inst_escaped.contains(t));
        if lost {
            let key = Key::any();
            self.m.cons(parsed, Cons::Load { key, dst: own });
        } else {
            self.m.json_reviver_own.push((tmpls, parsed, own));
        }
        Some(own)
    }

    /// `JSON.parse(text, reviver)`, the rule on: what the reviver returns
    /// is stored into its holder under the key (CreateDataProperty,
    /// InternalizeJSONProperty 25.5.1.1), so a named read of the holder
    /// sees it (`this.a` after the reviver returned an object for `a`). The
    /// arm's own call already gives the reviver the holders (`parsed`, the
    /// parse's single object and array tokens, which also stand for the
    /// root's `{"": value}`) and every parsed value (their any-key field);
    /// it stored the result under the element key only.
    pub(crate) fn json_reviver_calls(&mut self, parsed: NodeId, ret: NodeId) {
        if !self.m.reach_json_replacer {
            return;
        }
        // CreateDataProperty on an object the parse made: its content, as
        // the built-in arm's edge into the any-key field (no [[Set]]: no
        // inherited setter runs and no inherited method is overwritten).
        let key = Key::any();
        self.m.cons(parsed, Cons::BirthField { key, src: ret });
    }
}
