//! The exhaustive remainder of `Expr`, part: builtin objects (dates, sets, math, strings, regexps and the web algorithms).

use super::*;

impl<'a> Gen<'a> {
    /// The other half of a fold HIR chose by the receiver's bare NAME
    /// (`pre_scan_weakref_locals`: `r.deref()` is `Expr::WeakRefDeref`
    /// wherever a binding called `r` holds a `new WeakRef` somewhere in the
    /// module). The runtime helper tests the receiver's kind first and
    /// hands any other receiver to ordinary dispatch
    /// (`dispatch_foreign_weak_receiver`): the receiver's own `method`
    /// runs with `args`, and its result is the expression's. `kind` is the
    /// builtin the helper serves without a lookup.
    fn name_fold_call(
        &mut self,
        recv: NodeId,
        kind: &'static str,
        method: &str,
        args: Vec<NodeId>,
        out: NodeId,
    ) {
        if !self.name_folds {
            return;
        }
        let a = self.m.atom(method);
        // The helper forwards the values: the call's arguments are copies,
        // and the fold's own operands keep the roles the intrinsic gives
        // them (an identity observation, a sink).
        let args = args
            .into_iter()
            .map(|v| {
                let forwarded = self.tmp("name-fold-arg");
                self.m.edge(v, forwarded);
                forwarded
            })
            .collect();
        let (call, ret) = self.new_call(ThisArg::Node(recv), args, vec![]);
        self.m.name_fold_calls.insert(call, kind);
        self.m.cons(
            recv,
            Cons::MCall {
                key: Key::Named(a),
                call,
            },
        );
        self.m.edge(ret, out);
    }

    pub(super) fn expr_rest_builtin_objects(&mut self, fx: &Fx, e: &Expr) -> NodeId {
        let mi = fx.module;
        match e {
            // ------------------------------------------- builtin objects
            Expr::WeakRefNew(x) => {
                let v = self.expr(fx, x);
                let (t, n) = self.bobj(mi, "WeakRef");
                let a = self.m.atom("#target");
                let f = self.m.field(t, a);
                self.m.edge(v, f);
                n
            }
            Expr::WeakRefDeref(x) => {
                let r = self.expr(fx, x);
                let a = self.m.atom("#target");
                let out = self.load(r, Key::Named(a));
                self.m.add_prim(out, prim::UNDEF);
                self.name_fold_call(r, "WeakRef", "deref", vec![], out);
                out
            }
            Expr::FinalizationRegistryNew(cb) => {
                let c = self.expr(fx, cb);
                self.to_sink(c, "FinalizationRegistry callback");
                self.bobj(mi, "FinalizationRegistry").1
            }
            Expr::FinalizationRegistryRegister {
                registry,
                target,
                held,
                token,
            } => {
                let r = self.expr(fx, registry);
                // The registry keeps the target and the token by identity
                // (the weak collections' route).
                let t = self.expr(fx, target);
                self.m.identity(t, IdentityRoute::Collection);
                let h = self.expr(fx, held);
                self.to_sink(h, "FinalizationRegistry held value");
                let k = match token {
                    Some(k) => {
                        let k = self.expr(fx, k);
                        self.m.identity(k, IdentityRoute::Collection);
                        k
                    }
                    None => self.prim_node(prim::UNDEF),
                };
                if !self.name_folds {
                    return self.prim_node(prim::UNDEF);
                }
                let out = self.tmp("finreg-register");
                self.m.add_prim(out, prim::UNDEF);
                self.name_fold_call(r, "FinalizationRegistry", "register", vec![t, h, k], out);
                out
            }
            Expr::FinalizationRegistryUnregister { registry, token } => {
                let r = self.expr(fx, registry);
                let t = self.expr(fx, token);
                self.m.identity(t, IdentityRoute::Collection);
                if !self.name_folds {
                    return self.prim_node(prim::BOOL);
                }
                let out = self.tmp("finreg-unregister");
                self.m.add_prim(out, prim::BOOL);
                self.name_fold_call(r, "FinalizationRegistry", "unregister", vec![t], out);
                out
            }
            // Rule BD-A (`CtorPhase::birth_defines`): the constructor's
            // define creates fresh keys on the unobserved instance; each
            // is a birth store of the descriptor's value (or `undefined`).
            Expr::ObjectDefineProperty(o, ..) | Expr::ObjectDefineProperties(o, ..)
                if self.birth_define_owner(fx, e, o).is_some() =>
            {
                self.birth_define(fx, e, o)
            }
            Expr::ObjectDefineProperty(o, k, d)
            | Expr::ReflectDefineProperty {
                target: o,
                key: k,
                descriptor: d,
            } => {
                let on = self.expr(fx, o);
                let kn = self.expr(fx, k);
                let r = self.m.reason("E5 Object.defineProperty");
                self.m.cons(
                    kn,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::ToPrimitive,
                        region: self.m.thrown,
                    },
                );
                let key = self.const_key(mi, k);
                let accessor = self.accessor_descriptor(mi, d);
                // An accessor literal's own `get` / `set` is its constructor
                // argument: each define installs its own, whatever its key.
                // The literal's shape has one constructor for every site, so
                // a read of the built descriptor's field joins them all.
                let (dn, own) = match self.accessor_literal(mi, d) {
                    Some((class_name, args, get, set)) => {
                        let a = self.evaluate_all(fx, args);
                        let own = [get, set].map(|i| i.and_then(|i| a.get(i).copied()));
                        // K3 (B): an accessor written in place is a method of
                        // what it is defined on, under its key (`recv_family`).
                        if let Some(ks) = key.as_deref() {
                            let ka = self.m.atom(ks);
                            for i in [get, set].into_iter().flatten() {
                                if let (Expr::Closure { .. }, Some(&n)) = (&args[i], a.get(i)) {
                                    for ft in self.m.nodes[n as usize].pts.to_sorted() {
                                        self.m.rfam_home(ft, &[ka], None);
                                    }
                                }
                            }
                        }
                        (self.new_named(fx, class_name, a, vec![]), own)
                    }
                    None => (self.expr(fx, d), [None, None]),
                };
                match self.for_in_key_of(mi, k).filter(|_| key.is_none()) {
                    Some(kp) => self.define_keyed(fx, on, kp, dn, r, accessor, own),
                    None => self.define_property(fx, on, key, dn, r, accessor, own, Some(kn)),
                }
                on
            }
            Expr::ObjectDefineProperties(o, props) => {
                let on = self.expr(fx, o);
                let pn = self.expr(fx, props);
                let why = if self.m.explain {
                    format!(
                        "E5 Object.defineProperties at {}::{}",
                        self.mods[fx.module as usize].path, self.m.funcs[fx.f as usize].name
                    )
                } else {
                    "E5 Object.defineProperties".to_string()
                };
                let r = self.m.reason(&why);
                // A map of exactly known keys defines each by its descriptor
                // (`Cons::DefineAll`); any other map's own values are these
                // descriptors, defined under a computed key.
                let descs = self.tmp("own-descriptors");
                let reads = self.descriptor_reads(descs);
                let site = self.store_site(fx.module, fx.f, "defineProperty".into(), None, false);
                crate::builtins::object_statics::define_all(
                    &mut self.m,
                    on,
                    pn,
                    descs,
                    reads,
                    site,
                    r,
                );
                on
            }
            Expr::ObjectGetOwnPropertyDescriptor(o, k)
            | Expr::ReflectGetOwnPropertyDescriptor { target: o, key: k } => {
                let on = self.expr(fx, o);
                let kn = self.expr(fx, k);
                self.to_property_key(kn);
                // A for-in run's key is the one property described.
                let key = self.for_in_key_of(mi, k).unwrap_or(NO_NODE);
                let d = self.descriptor_of(mi, on, key);
                // 20.1.2.8 / 28.1.7: an absent property gives `undefined`.
                self.m.add_prim(d, prim::UNDEF);
                d
            }
            Expr::ObjectGetOwnPropertyDescriptors(o) => {
                let on = self.expr(fx, o);
                let d = self.descriptor_of(mi, on, NO_NODE);
                let (t, n) = self.bobj(mi, "descriptors");
                let fa = self.m.field_any(t);
                self.m.edge(d, fa);
                n
            }
            Expr::ObjectCreate(proto, props) => {
                let p = self.expr(fx, proto);
                let r = self.m.reason("E10 Object.create");
                let site = self.next_site(mi);
                let tm = self.m.tmpls.len() as TmplId;
                self.m.tmpls.push(Template {
                    kind: TmplKind::ObjCreate { module: mi, site },
                    display: format!(
                        "{}:Object.create#{site}",
                        self.mods[mi as usize].path_short()
                    ),
                    birth_keys: vec![],
                    definite: vec![],
                    declared: Vec::new(),
                    reserved: Vec::new(),
                    inert_definite: Vec::new(),
                    inert_reads: Vec::new(),
                    parent: None,
                    ctor: None,
                    native_parent: None,
                    accessor_keys: vec![],
                    has_computed_members: true,
                    field_init: None,
                    proto_members: vec![],
                    parent_value: None,
                    parent_links: Vec::new(),
                    null_proto: false,
                    chain_miss: false,
                    chain_miss_open: false,
                    species: None,
                    per_evaluation: false,
                    static_methods: Vec::new(),
                });
                let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
                crate::builtins::object_statics::create_proto(&mut self.m, p, t, r);
                let n = self.tok_node(t);
                if let Some(props) = props {
                    let pv = self.expr(fx, props);
                    let descs = self.load(pv, Key::any());
                    self.define_property(fx, n, None, descs, r, false, [None, None], None);
                }
                n
            }
            Expr::ObjectFreeze(o) | Expr::ObjectSeal(o) | Expr::ObjectPreventExtensions(o) => {
                let on = self.expr(fx, o);
                let r = self.m.reason("E8 freeze/seal/preventExtensions");
                let kind = match e {
                    Expr::ObjectFreeze(_) => Hard::E8Freeze,
                    Expr::ObjectSeal(_) => Hard::E8Seal,
                    _ => Hard::E8PreventExtensions,
                };
                self.m.cons(on, Cons::Hard { kind, reason: r });
                on
            }
            Expr::ReflectPreventExtensions(o) => {
                let on = self.expr(fx, o);
                let r = self.m.reason("E8 Reflect.preventExtensions");
                self.m.cons(
                    on,
                    Cons::Hard {
                        kind: Hard::E8PreventExtensions,
                        reason: r,
                    },
                );
                self.prim_node(prim::BOOL)
            }
            Expr::ObjectGetPrototypeOf(o) | Expr::ReflectGetPrototypeOf(o) => {
                let on = self.expr(fx, o);
                let out = self.tmp("protoof");
                self.m.cons(on, Cons::ProtoOf { dst: out });
                self.m.add_prim(out, prim::NULL);
                out
            }
            Expr::ObjectSetPrototypeOf(o, p)
            | Expr::ReflectSetPrototypeOf {
                target: o,
                proto: p,
            } => {
                let on = self.expr(fx, o);
                let pn = self.expr(fx, p);
                let r = self.m.reason("E6 Object.setPrototypeOf");
                crate::builtins::object_statics::set_prototype_of(&mut self.m, on, pn, r);
                // ECMA-262 20.1.2.23 step 5: Object.setPrototypeOf returns O
                // itself; 28.1.13 step 4: Reflect.setPrototypeOf returns the
                // Boolean of O.[[SetPrototypeOf]](proto), never O.
                if matches!(e, Expr::ReflectSetPrototypeOf { .. }) {
                    return self.prim_node(prim::BOOL);
                }
                on
            }
            Expr::JsonParse(..) | Expr::JsonParseTyped { .. } | Expr::JsonRawJson(..) => {
                let _ = self.prim_kids(fx, e, prim::STR, "JSON.parse text", Protocol::ToPrimitive);
                {
                    let positions = self.m.json_positions;
                    self.json_value(mi, positions, false)
                }
            }
            Expr::JsonParseReviver { text, reviver }
            | Expr::JsonParseWithReviver(text, reviver) => {
                let _ = self.prim_op(
                    fx,
                    &[text],
                    prim::STR,
                    "JSON.parse text",
                    Protocol::ToPrimitive,
                );
                let rv = self.expr(fx, reviver);
                let born = self.m.tmpls.len();
                let v = self.json_value(mi, false, true);
                let s = self.prim_node(prim::STR);
                // Own values while sure, the rule on (`json_reviver_values`).
                let vals = match self.json_reviver_values(v, born) {
                    Some(own) => own,
                    None => self.load(v, Key::any()),
                };
                let holder = v;
                let ret = self.call_value(rv, ThisArg::Node(holder), vec![s, vals], vec![]);
                // Stored under its key (`json_reviver_calls`).
                self.json_reviver_calls(v, ret);
                // The reviver's results replace values in the parsed tree.
                let tmp = self.tmp("revived");
                self.m.edge(ret, tmp);
                self.elem_store(fx, v, tmp, "reviver");
                let out = self.tmp("json-out");
                self.m.edge(v, out);
                self.m.edge(ret, out);
                out
            }
            Expr::JsonStringify(..)
            | Expr::JsonStringifyFull(..)
            | Expr::JsonStringifyPretty { .. } => {
                // toJSON / getters run; replacer functions are called by
                // the runtime with runtime values.
                let ks = kids(e);
                let replacer = match e {
                    Expr::JsonStringifyFull(_, r, _) => Some(&**r),
                    Expr::JsonStringifyPretty { replacer, .. } => replacer.as_deref(),
                    _ => None,
                };
                // A replacer function sees every value serialized, unseen
                // by the model (finding 14): with value
                // records on, the value's identity is observed.
                let replaced = self.m.value_records
                    && replacer.is_some_and(|r| {
                        !matches!(r, Expr::Null | Expr::Undefined | Expr::Array(..))
                    });
                // It is called back with them (`json_replacer_stringify`).
                if let Some(out) = self.json_replacer_stringify(fx, &ks, replacer, replaced) {
                    return out;
                }
                let out = if replaced && !ks.is_empty() {
                    let r = self.m.reason("JSON.stringify value");
                    let v = self.expr(fx, ks[0]);
                    self.m.cons(
                        v,
                        Cons::Implicit {
                            reason: r,
                            proto: Protocol::Json,
                            region: self.m.thrown,
                        },
                    );
                    self.m.identity(v, IdentityRoute::JsonReplacer);
                    let out = self.tmp("primop");
                    self.m.add_prim(out, prim::STR | prim::UNDEF);
                    out
                } else {
                    self.prim_op(
                        fx,
                        &ks[..ks.len().min(1)],
                        prim::STR | prim::UNDEF,
                        "JSON.stringify value",
                        Protocol::Json,
                    )
                };
                let rest = &ks[ks.len().min(1)..];
                let _ = self.prim_op(
                    fx,
                    rest,
                    prim::ALL,
                    "JSON.stringify replacer",
                    Protocol::Any,
                );
                out
            }
            Expr::WebAssemblyCompile(..)
            | Expr::WebAssemblyModuleNew(..)
            | Expr::WebAssemblyModuleExports(..)
            | Expr::WebAssemblyModuleImports(..)
            | Expr::WebAssemblyModuleCustomSections { .. }
            | Expr::WebAssemblyInstantiate { .. }
            | Expr::WebAssemblyCallExport { .. } => self.fail_closed(fx, e, "WebAssembly"),
            // Web helper algorithms live beside this exhaustive routing match.
            Expr::TextEncoderNew
            | Expr::TextDecoderNew { .. }
            | Expr::TextEncoderEncode(..)
            | Expr::TextEncoderEncodeInto { .. }
            | Expr::TextDecoderDecode { .. }
            | Expr::TextDecoderEncoding(..)
            | Expr::TextDecoderFatal(..)
            | Expr::TextDecoderIgnoreBom(..)
            | Expr::UrlInstanceToString(..)
            | Expr::UrlInstanceToJSON(..)
            | Expr::UrlSearchParamsGet { .. }
            | Expr::UrlSearchParamsGetAll { .. }
            | Expr::UrlSearchParamsHas { .. }
            | Expr::UrlSearchParamsSet { .. }
            | Expr::UrlSearchParamsAppend { .. }
            | Expr::UrlSearchParamsDelete { .. }
            | Expr::UrlSearchParamsToString(..)
            | Expr::UrlSearchParamsSort(..)
            | Expr::UrlSearchParamsEntries(..)
            | Expr::UrlSearchParamsKeys(..)
            | Expr::UrlSearchParamsValues(..)
            | Expr::UrlSearchParamsForEach { .. }
            | Expr::UrlSearchParamsMissingArgs { .. }
            | Expr::UrlGetHref(..)
            | Expr::UrlGetPathname(..)
            | Expr::UrlGetProtocol(..)
            | Expr::UrlGetHost(..)
            | Expr::UrlGetHostname(..)
            | Expr::UrlGetPort(..)
            | Expr::UrlGetSearch(..)
            | Expr::UrlGetHash(..)
            | Expr::UrlGetOrigin(..)
            | Expr::UrlGetSearchParams(..)
            | Expr::UrlSetPathname { .. }
            | Expr::UrlSetSearch { .. }
            | Expr::UrlSetHash { .. }
            | Expr::UrlSetProtocol { .. }
            | Expr::UrlSetHostname { .. }
            | Expr::UrlSetPort { .. }
            | Expr::UrlSetUsername { .. }
            | Expr::UrlSetPassword { .. }
            | Expr::UrlSetHref { .. }
            | Expr::UrlNew { .. }
            | Expr::UrlSearchParamsNew(..)
            | Expr::UrlPatternNew { .. }
            | Expr::UrlParse(..)
            | Expr::UrlParseWithBase { .. }
            | Expr::UrlCanParse(..)
            | Expr::UrlCanParseWithBase { .. } => self.web_expr(fx, e),
            Expr::BufferFrom { .. }
            | Expr::BufferAlloc { .. }
            | Expr::BufferAllocUnsafe(..)
            | Expr::FsReadFileBinary(..) => {
                // Typed arrays / buffers hold numbers; an array argument is
                // read, a buffer/view argument is shared (B1, flow only).
                let ks = kids(e);
                // Each is a Buffer (the host's file contents too): its own
                // methods are `Buffer.prototype`'s, the rest its typed
                // array chain's.
                let (t, n) = self.bobj_view(mi, "Buffer");
                // Its buffer (`VIEWED_BUFFER`): `Buffer.alloc` allocates one
                // (lib/buffer.js `new FastBuffer(size)`); `Buffer.from` of an
                // ArrayBuffer views it; the rest may view the pool.
                let pool = crate::builtins::buffer_pool(&mut self.m);
                let born = match e {
                    Expr::BufferAlloc { .. } => Some(self.bobj_view(mi, "ArrayBuffer").0),
                    Expr::BufferFrom { .. } => None,
                    _ => Some(pool),
                };
                if let Some(ab) = born {
                    let f = crate::builtins::viewed_buffer(&mut self.m, t);
                    self.m.add_tok(f, ab);
                }
                let owner = self.begin_indexed_owner(mi, t, e);
                let mut first = born.is_none();
                for k in ks {
                    let v = self.expr(fx, k);
                    if std::mem::take(&mut first) {
                        self.m.cons(
                            v,
                            Cons::ViewedBuffer {
                                view: t,
                                fresh: Some(pool),
                                share: false,
                            },
                        );
                    }
                    if owner {
                        if let TokKind::Obj(tm, _) = self.m.toks[t as usize] {
                            self.m.ta_birth_input.insert(tm, v);
                        }
                    }
                    let r = self.m.reason("typed-array source");
                    self.m.cons(
                        v,
                        Cons::Implicit {
                            reason: r,
                            proto: Protocol::Any,
                            region: self.m.thrown,
                        },
                    );
                    self.m.cons(v, Cons::ShareBuffer { ta: t });
                }
                let fa = self.m.field_any(t);
                self.m.add_prim(fa, prim::NUM);
                n
            }
            Expr::Uint8ArrayNew(..) | Expr::Uint8ArrayFrom(..) | Expr::TypedArrayNew { .. } => {
                self.typed_array_new(fx, e)
            }
            Expr::BufferFromArrayBuffer { .. }
            | Expr::BufferSlice { .. }
            | Expr::BufferConcat(..)
            | Expr::BufferConcatWithLength { .. } => {
                let ks = kids(e);
                let mut vs = Vec::new();
                for k in ks {
                    let v = self.expr(fx, k);
                    let it = self.iterate(v);
                    let _ = it;
                    vs.push(v);
                }
                let (t, n) = self.bobj_view(mi, "TypedArray");
                // Its buffer (`VIEWED_BUFFER`): `Buffer.from(ab, ...)` views
                // ab, a Buffer slice its receiver's, a concatenation may
                // view the pool (lib/buffer.js `allocUnsafe`).
                let pool = crate::builtins::buffer_pool(&mut self.m);
                match (e, vs.first()) {
                    (Expr::BufferFromArrayBuffer { .. } | Expr::BufferSlice { .. }, Some(&v)) => {
                        let share = matches!(e, Expr::BufferSlice { .. });
                        self.m.cons(
                            v,
                            Cons::ViewedBuffer {
                                view: t,
                                fresh: (!share).then_some(pool),
                                share,
                            },
                        );
                    }
                    _ => {
                        let f = crate::builtins::viewed_buffer(&mut self.m, t);
                        self.m.add_tok(f, pool);
                    }
                }
                let fa = self.m.field_any(t);
                self.m.add_prim(fa, prim::NUM);
                n
            }
            Expr::StructuredClone { value, options } => {
                // E17: the clone does not keep identity (⊤-origin result);
                // serializing the source reads its values recursively (own
                // getters run) and calls nothing.
                let v = self.expr(fx, value);
                // The copy keeps the sharing of what it copies.
                self.m.identity(v, IdentityRoute::Clone);
                let r = self.m.reason("structuredClone source");
                self.m.cons(
                    v,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::Clone,
                        region: self.m.thrown,
                    },
                );
                let o = self.expr(fx, options);
                let rb = self.m.reason("B3 transfer list");
                let tl = self.load(o, Key::any());
                let items = self.load(tl, Key::any());
                self.m.cons(
                    items,
                    Cons::Hard {
                        kind: Hard::B3Transfer,
                        reason: rb,
                    },
                );
                self.m.top_val("structuredClone/transfer result")
            }
            Expr::LinkGeneratorPrototype {
                obj,
                is_async,
                fixed_state,
                i32_controls,
                i1_controls,
                ..
            } => {
                let o = self.expr(fx, obj);
                // The fixed resume boundary writes the public next value
                // into the step record's third capture before entering its
                // body. Lowering therefore omits the corresponding body store.
                if *fixed_state {
                    if let Expr::Closure {
                        params, captures, ..
                    } = &**obj
                    {
                        let value = if params.len() == 1 {
                            params.first()
                        } else {
                            params.get(1)
                        };
                        if let (Some(value), Some(&sent)) = (value, captures.get(2)) {
                            let value = self.local(mi, value.id);
                            let sent = self.local(mi, sent);
                            self.m.edge(value, sent);
                        }
                    }
                }
                // The generator object is its step closure. Its [[Prototype]]
                // is the running generator function's `prototype` object
                // (OrdinaryCreateFromConstructor at the generator's start):
                // an ordinary object the program may write (`g.prototype.next
                // = ...`), whose members a lookup on the generator object
                // finds before the generator protocol. It is the token a read
                // of `g.prototype` gives (`tok_load`), so what is stored there
                // and what escapes with either is the same value; a generator
                // object born after `g.prototype = o` inherits from `o`, so the
                // chain is the field, not this token. A generator
                // function is never constructed (`new g()` throws), so the
                // instance template's birth keys are never read. Every token
                // of the step function (one per closure context) has it.
                let publish = |gen: &mut Self| {
                    let g = fx.f;
                    let tm = gen.m.fn_inst_template(g);
                    let p = gen.m.tok(TokKind::Proto(tm));
                    let gt = gen.m.funcs[g as usize].token;
                    let pa = gen.m.atom("prototype");
                    let gf = gen.m.field(gt, pa);
                    gen.m.add_tok(gf, p);
                    // Every token of `g` (one per closure context, made later by
                    // `closure_tok`) feeds its own `prototype` field here.
                    let gp = match gen.m.funcs[g as usize].gen_fn_protos {
                        Some(n) => n,
                        None => {
                            let n = gen.m.node_l("gen-fn-protos");
                            gen.m.funcs[g as usize].gen_fn_protos = Some(n);
                            n
                        }
                    };
                    gen.m.edge(gf, gp);
                    for t in gen.m.nodes[o as usize].pts.to_sorted() {
                        if let TokKind::Func(f, _) = gen.m.toks[t as usize] {
                            gen.m.funcs[f as usize].gen_proto = Some(gp);
                            gen.m.funcs[f as usize].gen_async = *is_async;
                            // A chain made before this link (`proto_node` seeds
                            // later ones from `gen_proto`).
                            if let Some(&pn) = gen.m.proto_node.get(&t) {
                                gen.m.edge(gp, pn);
                            }
                        }
                    }
                };
                #[cfg(test)]
                self.packet_generator_effect(fx.f, o, *is_async, publish);
                #[cfg(not(test))]
                publish(self);
                for id in i32_controls.iter().chain(i1_controls.iter()) {
                    let l = self.local(mi, *id);
                    self.m.add_prim(l, prim::NUM | prim::BOOL);
                }
                o
            }
            Expr::IterResultSet(v, done) => {
                let vn = self.expr(fx, v);
                let r = self.iter_res_node(fx, *done);
                self.m.edge(vn, r);
                self.prim_node(prim::UNDEF)
            }
            Expr::IterResultGetValue => {
                let done = self.iter_done_arm == Some(fx.f);
                self.iter_res_node(fx, done)
            }
            Expr::AsyncStepChain {
                value,
                step_closure,
            } => {
                let v = self.expr(fx, value);
                let s = self.expr(fx, step_closure);
                // R-JOBTHROW: the error resume carries the awaited
                // promise's reason (and `thrown`, the rejections no promise
                // holds), not an unrelated promise's.
                let (aw, rj) = self.awaited_r(v);
                let arg = self.tmp("step-arg");
                self.m.edge(aw, arg);
                // A plain-async step resumes with the value as a normal
                // completion and the reason as a throw completion, each
                // through its own carrier (`StepSplits`); any other step
                // takes both through its value parameter.
                let err = match step_closure.as_ref() {
                    Expr::LocalGet(id) => self.step.by_self.get(&(mi, *id)).copied(),
                    _ => None,
                };
                let err = err.unwrap_or(arg);
                self.m.edge(rj, err);
                self.m.edge(self.thrown, err);
                let b = self.prim_node(prim::BOOL);
                self.async_callback(s, vec![arg, b])
            }
            Expr::AsyncStepResumeNow {
                value,
                step_closure,
            } => {
                // On admission the awaited value (promise unwrapped) is the
                // expression's result, which the step uses as its next resume
                // value; a decline yields the TDZ sentinel, never observed.
                // It admits only a primitive or a fulfilled native promise
                // (`js_async_step_resume_now`, P4), so it never rejects and
                // calls no `then`: a rejected or pending promise declines to
                // the `AsyncStepChain`, which carries its reason.
                let v = self.expr(fx, value);
                let _ = self.expr(fx, step_closure);
                self.settled_now(v)
            }
            Expr::AsyncStepDeclined(e) => {
                let _ = self.expr(fx, e);
                self.prim_node(prim::BOOL)
            }
            Expr::AsyncStepDone {
                value,
                step_closure,
            } => {
                let v = self.expr(fx, value);
                let _ = self.expr(fx, step_closure);
                v
            }
            Expr::CurrentStepClosure => {
                let t = self.m.funcs[fx.f as usize].token;
                self.tok_node(t)
            }
            Expr::AsyncFirstCall { step_closure } => {
                let s = self.expr(fx, step_closure);
                let u = self.prim_node(prim::UNDEF);
                let b = self.prim_node(prim::BOOL);
                let r = self.async_callback(s, vec![u, b]);
                self.promise_of(mi, r)
            }
            Expr::AsyncGenResume {
                step_closure,
                value,
                ..
            } => {
                let s = self.expr(fx, step_closure);
                let v = self.expr(fx, value);
                let arg = self.tmp("gen-arg");
                self.m.edge(v, arg);
                self.m.edge(self.thrown, arg);
                let b = self.prim_node(prim::BOOL);
                let r = self.async_callback(s, vec![arg, b]);
                let p = self.promise_of(mi, r);
                self.m.add_tok(p, TOP);
                p
            }
            Expr::WebCryptoDigest { .. }
            | Expr::WebCryptoImportKey { .. }
            | Expr::WebCryptoExportKey { .. }
            | Expr::WebCryptoSign { .. }
            | Expr::WebCryptoVerify { .. }
            | Expr::WebCryptoDeriveBits { .. }
            | Expr::WebCryptoDeriveKey { .. }
            | Expr::WebCryptoEncrypt { .. }
            | Expr::WebCryptoDecrypt { .. }
            | Expr::WebCryptoGenerateKey { .. }
            | Expr::WebCryptoWrapKey { .. }
            | Expr::WebCryptoUnwrapKey { .. }
            | Expr::CryptoRandomFillSync { .. }
            | Expr::NativeArenaAlloc(..)
            | Expr::NativeArenaView { .. }
            | Expr::NativePodView { .. }
            | Expr::NativeArenaDispose(..)
            | Expr::NativeMemoryFillU32 { .. }
            | Expr::NativeMemoryCopy { .. }
            | Expr::ChildProcessExecSync { .. }
            | Expr::ChildProcessSpawnSync { .. }
            | Expr::ChildProcessSpawn { .. }
            | Expr::ChildProcessFork { .. }
            | Expr::ChildProcessExec { .. }
            | Expr::ChildProcessExecFile { .. }
            | Expr::ChildProcessExecFileSync { .. }
            | Expr::ChildProcessSpawnBackground { .. }
            | Expr::FetchWithOptions { .. }
            | Expr::FetchGetWithAuth { .. }
            | Expr::FetchPostWithAuth { .. }
            | Expr::NetCreateServer { .. }
            | Expr::NetCreateConnection { .. }
            | Expr::NetConnect { .. }
            | Expr::StaticPluginResolve(..)
            | Expr::JsLoadModule { .. }
            | Expr::JsGetExport { .. }
            | Expr::JsCallFunction { .. }
            | Expr::JsCallMethod { .. }
            | Expr::JsCallValue { .. }
            | Expr::JsGetProperty { .. }
            | Expr::JsSetProperty { .. }
            | Expr::JsNew { .. }
            | Expr::JsNewFromHandle { .. }
            | Expr::JsCreateCallback { .. }
            | Expr::ProxyGet { .. }
            | Expr::ProxySet { .. }
            | Expr::ProxyHas { .. }
            | Expr::ProxyDelete { .. }
            | Expr::ReflectDefineMetadata { .. }
            | Expr::ReflectGetMetadata { .. }
            | Expr::ReflectGetOwnMetadata { .. }
            | Expr::ReflectHasMetadata { .. }
            | Expr::ReflectHasOwnMetadata { .. }
            | Expr::ReflectGetMetadataKeys { .. }
            | Expr::ReflectGetOwnMetadataKeys { .. }
            | Expr::ReflectDeleteMetadata { .. }
            | Expr::WorkerNew { is_eval: true, .. } => {
                let name = variant_name(e);
                self.fail_closed(fx, e, &name)
            }
            // `new Worker(filename, options)` (node:worker_threads Worker;
            // an eval-mode Worker runs JS SOURCE and stays unmodeled above:
            // CLOSED-WORLD PREMISE: no eval / dynamic code). The worker
            // module is a program module run in its own agent with its own
            // realm; the construct shares no value with it: the filename is
            // converted to a string (a URL: ToString, its href), each option
            // is read by [[Get]] (getters run), workerData / argv / env are
            // cloned (StructuredSerialize reads their values recursively,
            // getters run; argv and env values are string-coerced),
            // transferList items are detached. The result is a fresh host
            // Worker object: no program alias, never a realm global; its
            // members are host members the model does not describe.
            Expr::WorkerNew {
                filename,
                options,
                is_eval: false,
                ..
            } => {
                let f = self.expr(fx, filename);
                let rf = self.m.reason("Worker filename");
                self.m.cons(
                    f,
                    Cons::Implicit {
                        reason: rf,
                        proto: Protocol::ToPrimitive,
                        region: self.m.thrown,
                    },
                );
                if let Some(o) = options {
                    let o = self.expr(fx, o);
                    let opt = self.load(o, Key::any());
                    let rc = self.m.reason("Worker option cloned");
                    self.m.cons(
                        opt,
                        Cons::Implicit {
                            reason: rc,
                            proto: Protocol::Clone,
                            region: self.m.thrown,
                        },
                    );
                    let item = self.load(opt, Key::any());
                    let rs = self.m.reason("Worker argv/env value");
                    self.m.cons(
                        item,
                        Cons::Implicit {
                            reason: rs,
                            proto: Protocol::ToPrimitive,
                            region: self.m.thrown,
                        },
                    );
                    let rb = self.m.reason("B3 transfer list");
                    self.m.cons(
                        item,
                        Cons::Hard {
                            kind: Hard::B3Transfer,
                            reason: rb,
                        },
                    );
                }
                // W-HOST: the Worker is a fresh host object no unknown code
                // holds; the runtime only calls the listeners the program
                // installed on it (`worker_host`).
                let (t, n) = self.bobj(mi, "Worker");
                self.worker_host(mi, t, n);
                n
            }
            Expr::BoxedPrimitiveNew { kind, .. } => {
                let _ = self.prim_kids(fx, e, prim::ALL, "boxed primitive", Protocol::ToPrimitive);
                let what = match kind {
                    perry_hir::ir::BoxedPrimitiveKind::Number => "BoxedNumber",
                    perry_hir::ir::BoxedPrimitiveKind::String => "BoxedString",
                    perry_hir::ir::BoxedPrimitiveKind::Boolean => "BoxedBoolean",
                };
                self.bobj(mi, what).1
            }
            Expr::DateNew(..) => {
                let _ = self.prim_kids(fx, e, prim::ALL, "Date operand", Protocol::ToPrimitive);
                self.bobj(mi, "Date").1
            }
            Expr::RegExp { pattern, flags } => {
                let (t, n) = self.bobj(mi, "RegExp");
                if let &TokKind::Obj(tm, _) = &self.m.toks[t as usize] {
                    if crate::builtins::regex_tail::source_is_pattern(pattern) {
                        self.m.regexp_source_of.insert(tm, pattern.to_string());
                    }
                    if let Some(mk) = crate::builtins::regex_tail::global_class(pattern, flags) {
                        self.m.regexp_class_of.insert(tm, mk);
                    }
                }
                if let (&TokKind::Obj(tm, _), Some(tail)) = (
                    &self.m.toks[t as usize],
                    crate::builtins::regex_tail::analyse(pattern, flags),
                ) {
                    let i = self.m.regexp_tails.len() as u32;
                    self.m.regexp_tails.push(tail);
                    self.m.regexp_tail_of.insert(tm, i);
                }
                n
            }
            // ECMA-262 22.2.4.1: IsRegExp(pattern) reads `@@match`; for a
            // regexp-like pattern `source` and `flags` are read (and in the
            // call form `constructor`, returning the pattern itself); then
            // ToString(pattern) and ToString(flags).
            Expr::RegExpDynamic {
                pattern,
                flags,
                is_call,
            } => {
                use crate::builtins::string_methods::{read_only, read_prim, to_prim};
                let p = self.expr(fx, pattern);
                let wk = format!("{}match", crate::builtins::WK_PREFIX);
                read_only(&mut self.m, p, &wk);
                if *is_call {
                    read_only(&mut self.m, p, "constructor");
                }
                // Step 4: a pattern with [[RegExpMatcher]] gives its
                // [[OriginalSource]] / [[OriginalFlags]] (perry-runtime
                // regex.rs `js_regexp_construct`; `Cons::NotRegExp`).
                let q = self.m.node_l("RegExp pattern");
                self.m.cons(p, Cons::NotRegExp { dst: q });
                read_prim(&mut self.m, q, "source", "RegExp pattern source ToString");
                read_prim(&mut self.m, q, "flags", "RegExp pattern flags ToString");
                to_prim(&mut self.m, q, "RegExp pattern ToString");
                let fnode = flags.as_ref().map(|f| self.expr(fx, f));
                if let Some(f) = fnode {
                    to_prim(&mut self.m, f, "RegExp flags ToString");
                }
                let (t, out) = self.bobj(mi, "RegExp");
                // Rule M9: both operands, for a global one-class pattern.
                if let Some(f) = fnode {
                    self.m.regexp_ctor_args.insert(t, (p, f));
                }
                // Rule M7 (22.2.3.2 RegExpInitialize): a pattern of one String
                // constant and no flags operand is the object's
                // [[OriginalSource]] with flags "", as a literal's
                // (`Model::regexp_pattern`).
                if flags.is_none() {
                    self.m.regexp_ctor_toks.insert(t);
                    self.m.cons(p, Cons::RegExpPattern { tok: t });
                }
                if *is_call {
                    self.m.edge(p, out);
                }
                out
            }
            Expr::ErrorNew(..)
            | Expr::TypeErrorNew(..)
            | Expr::RangeErrorNew(..)
            | Expr::ReferenceErrorNew(..)
            | Expr::SyntaxErrorNew(..) => {
                // ECMA-262 20.5.1.1 step 3: ToString(message), nothing else.
                let _ = self.prim_kids(fx, e, prim::ALL, "Error message", Protocol::ToPrimitive);
                self.error_obj(mi, None)
            }
            Expr::ErrorNewWithCause { message, cause } => {
                let _ = self.prim_op(
                    fx,
                    &[message],
                    prim::ALL,
                    "Error message",
                    Protocol::ToPrimitive,
                );
                let c = self.expr(fx, cause);
                self.error_obj(mi, Some(c))
            }
            Expr::ErrorNewWithOptions {
                message, options, ..
            } => {
                let _ = self.prim_op(
                    fx,
                    &[message],
                    prim::ALL,
                    "Error message",
                    Protocol::ToPrimitive,
                );
                let o = self.expr(fx, options);
                let ca = self.m.atom("cause");
                let c = self.load(o, Key::Named(ca));
                self.error_obj(mi, Some(c))
            }
            Expr::AggregateErrorNew {
                errors,
                message,
                options,
            } => {
                let es = self.expr(fx, errors);
                let it = self.iterate(es);
                let arr = self.new_arr_from(mi, "errors", &[it]);
                let _ = self.prim_op(
                    fx,
                    &[message],
                    prim::ALL,
                    "Error message",
                    Protocol::ToPrimitive,
                );
                let c = options.as_ref().map(|o| {
                    let on = self.expr(fx, o);
                    let ca = self.m.atom("cause");
                    self.load(on, Key::Named(ca))
                });
                let n = self.error_obj(mi, c);
                let ea = self.m.atom("errors");
                let site = self.store_site(mi, fx.f, "AggregateError.errors".into(), None, true);
                self.m.cons(
                    n,
                    Cons::Store {
                        key: Key::Named(ea),
                        src: arr,
                        site,
                        region: self.region(),
                    },
                );
                n
            }
            Expr::ErrorMessage(x) => {
                let v = self.expr(fx, x);
                let a = self.m.atom("message");
                let out = self.load(v, Key::Named(a));
                self.m.add_prim(out, prim::STR);
                out
            }
            // The string methods dispatch on their pattern operand by the
            // spec's algorithm (`builtins::string_methods`): the receiver is
            // ToString'd, and the method runs on a string.
            Expr::StringReplace {
                string,
                pattern,
                replacement,
            } => self.string_method(fx, string, "replace", &[pattern, replacement]),
            Expr::StringSplit(string, sep) => self.string_method(fx, string, "split", &[sep]),
            Expr::StringMatch { string, regex } => {
                self.string_method(fx, string, "match", &[regex])
            }
            Expr::StringMatchAll { string, regex } => {
                self.string_method(fx, string, "matchAll", &[regex])
            }
            _ => unreachable!("internal compiler error: expr_rest: not a builtin_objects variant"),
        }
    }

    /// Rule BD-A: the class whose constructor's birth define `e` is
    /// (`CtorPhase::birth_defines`), when every key it defines is new to the
    /// whole chain: no ancestor's birth key (a redefinition runs 10.1.6.3
    /// on an existing property: the general define, HARD) and no member of a
    /// prototype on the chain (an own data key would shadow it: the general
    /// define handles that as before).
    fn birth_define_owner(&self, fx: &Fx, e: &Expr, o: &Expr) -> Option<TmplId> {
        let tm = self.phase_owner(fx, e, o)?;
        let ks = self
            .ctor_phase
            .get(&tm)?
            .birth_defines
            .get(&(e as *const Expr as usize))?;
        let mut cur = Some(tm);
        let mut own = true;
        while let Some(c) = cur {
            let t = &self.m.tmpls[c as usize];
            for (k, _) in ks {
                let a = self.m.atom_index.get(k.as_str()).copied();
                let held = a.is_some_and(|a| {
                    (!own && t.birth_keys.contains(&a))
                        || t.proto_members.contains(&a)
                        || t.accessor_keys.contains(&a)
                });
                if held {
                    return None;
                }
            }
            if t.parent_value.is_some() && t.parent.is_none() {
                return None;
            }
            own = false;
            cur = t.parent;
        }
        Some(tm)
    }

    /// Rule BD-A lowering: the descriptor fields are evaluated (in order),
    /// then each key is stored at birth with its value, its site carrying
    /// the attribute entry (`StoreSite::define_attrs`).
    fn birth_define(&mut self, fx: &Fx, e: &Expr, o: &Expr) -> NodeId {
        let mi = fx.module;
        let on = self.expr(fx, o);
        let Some(tm) = self.birth_define_owner(fx, e, o) else {
            return on;
        };
        let shapes = crate::ctor_phase::plain_shapes(&self.modules[mi as usize].1.classes);
        let ds = crate::ctor_phase::literal_defines(&shapes, e).unwrap_or_default();
        for (k, attrs, value, values) in ds {
            let mut vn = None;
            for v in values {
                let n = self.expr(fx, v);
                if value.is_some_and(|x| std::ptr::eq(x, v)) {
                    vn = Some(n);
                }
            }
            let src = vn.unwrap_or_else(|| self.prim_node(prim::UNDEF));
            let a = self.m.atom(k);
            self.m.birth_attrs.insert((tm, a), attrs);
            let site = self.store_site(mi, fx.f, format!("define {k}"), Some(tm), false);
            self.m.stores[site as usize].define_attrs = Some(attrs);
            self.m.cons(
                on,
                Cons::Store {
                    key: Key::Named(a),
                    src,
                    site,
                    region: self.region(),
                },
            );
        }
        on
    }
}

impl<'a> Gen<'a> {
    /// W-HOST (LADDER18): what the runtime does with the host Worker `t`
    /// (node `n`) made at this site (perry-stdlib worker_threads
    /// worker_pump.rs `dispatch_worker_event_with_ports`, worker_threads.rs
    /// `call_callback1` / `call_message_callback1` / `event_object` /
    /// `worker_new_prepared`; HTML Workers, StructuredDeserialize).
    ///
    /// - A message crosses as a structured clone the runtime makes
    ///   (thread/clone_write.rs, Message mode: a function is a
    ///   DataCloneError, a class instance becomes a plain object): the
    ///   clone tree `[[WorkerPayload]]` is fresh ordinary objects and
    ///   arrays, Map, Set, Date, RegExp, Error, ArrayBuffer and typed
    ///   arrays, each holding the others and any primitive; never a
    ///   program object, function or program prototype. It is also the
    ///   payload of the `error` (an Error) and `exit` (a Number) events.
    /// - A web listener (`onmessage` / `onerror` / `onmessageerror`,
    ///   `addEventListener`) gets an event of `[[WorkerEvent]]`: for a
    ///   message a fresh MessageEvent (chain %MessageEvent.prototype% ->
    ///   %Event.prototype% -> %Object.prototype%) whose `[[data]]` is the
    ///   clone tree; for any other event a fresh plain record (HOSTEV) of
    ///   own `type`, `target` and `currentTarget` (the Worker),
    ///   `defaultPrevented` and `constructor` (a plain `{ name: "Event" }`).
    /// - A property handler is called with `this` = the Worker and the
    ///   event; its throw is reported by the host
    ///   (`run_resource_scope_catching`): no program `catch` sees it.
    /// - The construct queues `process.emit("worker", worker)`: [[Get]]
    ///   of `emit` on `process`, called with the Worker.
    ///
    /// Listener registration and `postMessage` are the Worker methods of
    /// `bobj_method` (builtins/collections.rs `worker_method`).
    fn worker_host(&mut self, mi: u32, t: Tok, n: NodeId) {
        let (obj, _) = self.bobj(mi, "json");
        let arr = self.new_arr_template(mi, "json");
        let arr = self.m.tok(TokKind::Obj(arr, NO_CTX));
        let mut tree = vec![obj, arr];
        for what in [
            "Map",
            "Set",
            "Date",
            "RegExp",
            "Error",
            "ArrayBuffer",
            "TypedArray",
        ] {
            let (x, _) = self.bobj(mi, what);
            tree.push(x);
        }
        for &o in &tree {
            let fa = self.m.field_any(o);
            self.m.add_prim(fa, prim::ALL);
            for &x in &tree {
                self.m.add_tok(fa, x);
            }
        }
        let pa = crate::builtins::worker_slot_atom(&mut self.m, "WorkerPayload");
        let payload = self.m.field_r(t, pa);
        self.m.add_prim(payload, prim::ALL);
        for &x in &tree {
            self.m.add_tok(payload, x);
        }
        // A message's event: a MessageEvent (message_event.rs `delivery`),
        // [[Prototype]] %MessageEvent.prototype% -> %Event.prototype%, no
        // own property; its `data` getter returns its [[data]] slot, the
        // clone tree. Its `ports` stay the generic MessagePort path.
        let (mev, _) = self.bobj(mi, "MessageEvent.prototype");
        let da = crate::builtins::worker_slot_atom(&mut self.m, "data");
        let data = self.m.field_r(mev, da);
        self.m.add_prim(data, prim::ALL);
        for &x in &tree {
            self.m.add_tok(data, x);
        }
        // HOSTEV: any other event (`error`, `messageerror`, `exit`) is the
        // plain object of perry-stdlib worker_threads.rs `event_object`
        // (Object.prototype): exactly the own data fields `type` (the event
        // name), `target` and `currentTarget` (the Worker),
        // `defaultPrevented` (false) and `constructor`, a fresh plain object
        // whose one own field `name` is "Event". No clone value reaches it
        // (worker_pump.rs hands `data` to the "message" event only), so a
        // read of any other key (`e.data`) is the Object.prototype chain.
        let (ev, _) = self.bobj(mi, "hostrecord");
        let (ctor, _) = self.bobj(mi, "hostrecord");
        let na = self.m.atom("name");
        let name = self.m.field(ctor, na);
        self.m.add_prim(name, prim::STR);
        for (k, p, v) in [
            ("type", prim::STR, None),
            ("target", 0, Some(t)),
            ("currentTarget", 0, Some(t)),
            ("defaultPrevented", prim::FALSE, None),
            ("constructor", 0, Some(ctor)),
        ] {
            let a = self.m.atom(k);
            let f = self.m.field(ev, a);
            if p != 0 {
                self.m.add_prim(f, p);
            }
            if let Some(v) = v {
                self.m.add_tok(f, v);
            }
        }
        let ea = crate::builtins::worker_slot_atom(&mut self.m, "WorkerEvent");
        let evn = self.m.field_r(t, ea);
        self.m.add_tok(evn, mev);
        self.m.add_tok(evn, ev);
        let host = self.m.host_report;
        let any = self.m.field_any(t);
        for k in ["onmessage", "onerror", "onmessageerror"] {
            let a = self.m.atom(k);
            let f = self.m.field_r(t, a);
            let arg = self.tok_node(if k == "onmessage" { mev } else { ev });
            let (id, _) = self.new_call(ThisArg::Node(n), vec![arg], vec![]);
            self.m.calls[id as usize].region = host;
            self.m.cons(f, Cons::Call { call: id });
            self.m.cons(any, Cons::Call { call: id });
        }
        let p = self.m.tok(TokKind::Builtin(BuiltinNs::Process));
        let pn = self.tok_node(p);
        let name = self.m.node_l("worker-event-name");
        self.m.add_prim(name, prim::STR);
        let (emit, _) = self.new_call(ThisArg::Node(pn), vec![name, n], vec![]);
        let (tick, _) = self.new_call(ThisArg::Node(pn), vec![n], vec![]);
        self.m.calls[emit as usize].region = host;
        self.m.calls[tick as usize].region = host;
        crate::builtins::process_worker_event(&mut self.m, n, emit, tick);
    }
}
