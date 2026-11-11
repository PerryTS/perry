//! The exhaustive remainder of `Expr`, part: builtin objects (dates, sets, math, strings, regexps and the web algorithms).

use super::*;

impl<'a> Gen<'a> {
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
                let _ = self.expr(fx, registry);
                let _ = self.expr(fx, target);
                let h = self.expr(fx, held);
                self.to_sink(h, "FinalizationRegistry held value");
                if let Some(t) = token {
                    let _ = self.expr(fx, t);
                }
                self.prim_node(prim::UNDEF)
            }
            Expr::FinalizationRegistryUnregister { registry, token } => {
                let _ = self.expr(fx, registry);
                let _ = self.expr(fx, token);
                self.prim_node(prim::BOOL)
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
                        (self.new_named(fx, class_name, a, vec![]), own)
                    }
                    None => (self.expr(fx, d), [None, None]),
                };
                match self.for_in_key_of(mi, k).filter(|_| key.is_none()) {
                    Some(kp) => self.define_keyed(fx, on, kp, dn, r, accessor, own),
                    None => self.define_property(fx, on, key, dn, r, accessor, own),
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
                let [value, dget, dset] = self.descriptor_reads(descs);
                let site = self.store_site(fx.module, fx.f, "defineProperty".into(), None, false);
                let star = self.m.atom(crate::STAR_KEY);
                for (f, setter) in [(dget, false), (dset, true)] {
                    self.m.cons(
                        f,
                        Cons::Accessor {
                            holder: on,
                            key: star,
                            setter,
                        },
                    );
                }
                self.m.cons(
                    pn,
                    Cons::DefineAll {
                        o: on,
                        descs,
                        value,
                        site,
                        reason: r,
                    },
                );
                on
            }
            Expr::ObjectGetOwnPropertyDescriptor(o, k)
            | Expr::ReflectGetOwnPropertyDescriptor { target: o, key: k } => {
                let on = self.expr(fx, o);
                let kn = self.expr(fx, k);
                self.to_property_key(kn);
                self.descriptor_of(mi, on)
            }
            Expr::ObjectGetOwnPropertyDescriptors(o) => {
                let on = self.expr(fx, o);
                let d = self.descriptor_of(mi, on);
                let (t, n) = self.bobj(mi, "descriptors");
                let fa = self.m.field_any(t);
                self.m.edge(d, fa);
                n
            }
            Expr::ObjectCreate(proto, props) => {
                let p = self.expr(fx, proto);
                let r = self.m.reason("E10 Object.create");
                self.m.cons(p, Cons::CreateProto { reason: r });
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
                let pn = self.m.proto_node(t);
                self.m.edge(p, pn);
                let n = self.tok_node(t);
                if let Some(props) = props {
                    let pv = self.expr(fx, props);
                    let descs = self.load(pv, Key::any());
                    self.define_property(fx, n, None, descs, r, false, [None, None]);
                }
                n
            }
            Expr::ObjectFreeze(o) | Expr::ObjectSeal(o) | Expr::ObjectPreventExtensions(o) => {
                let on = self.expr(fx, o);
                let r = self.m.reason("E8 freeze/seal/preventExtensions");
                self.m.cons(
                    on,
                    Cons::Hard {
                        kind: Hard::E8Freeze,
                        reason: r,
                    },
                );
                on
            }
            Expr::ReflectPreventExtensions(o) => {
                let on = self.expr(fx, o);
                let r = self.m.reason("E8 Reflect.preventExtensions");
                self.m.cons(
                    on,
                    Cons::Hard {
                        kind: Hard::E8Freeze,
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
                self.m.cons(
                    on,
                    Cons::Hard {
                        kind: Hard::E6SetPrototypeOf,
                        reason: r,
                    },
                );
                self.m.cons(on, Cons::ProtoFrom { src: pn });
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
                self.json_value(mi)
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
                let v = self.json_value(mi);
                let s = self.prim_node(prim::STR);
                let vals = self.load(v, Key::any());
                let holder = v;
                let ret = self.call_value(rv, ThisArg::Node(holder), vec![s, vals], vec![]);
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
                let out = self.prim_op(
                    fx,
                    &ks[..ks.len().min(1)],
                    prim::STR | prim::UNDEF,
                    "JSON.stringify value",
                    Protocol::Json,
                );
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
                // The host's file contents are a Buffer: its own methods
                // are `Buffer.prototype`'s, the rest its typed array chain.
                let what = match e {
                    Expr::FsReadFileBinary(..) => "Buffer",
                    _ => "TypedArray",
                };
                let (t, n) = self.bobj(mi, what);
                let owner = self.begin_indexed_owner(mi, t, e);
                for k in ks {
                    let v = self.expr(fx, k);
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
                for k in ks {
                    let v = self.expr(fx, k);
                    let it = self.iterate(v);
                    let _ = it;
                }
                let (t, n) = self.bobj(mi, "TypedArray");
                let fa = self.m.field_any(t);
                self.m.add_prim(fa, prim::NUM);
                n
            }
            Expr::StructuredClone { value, options } => {
                // E17: the clone does not keep identity (⊤-origin result);
                // serializing the source reads its values recursively (own
                // getters run) and calls nothing.
                let v = self.expr(fx, value);
                let r = self.m.reason("structuredClone source");
                self.m.cons(
                    v,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::Clone,
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
            Expr::IterResultSet(v, _) => {
                let vn = self.expr(fx, v);
                let r = self.iter_res_node(fx);
                self.m.edge(vn, r);
                self.prim_node(prim::UNDEF)
            }
            Expr::IterResultGetValue => self.iter_res_node(fx),
            Expr::AsyncStepChain {
                value,
                step_closure,
            } => {
                let v = self.expr(fx, value);
                let s = self.expr(fx, step_closure);
                let aw = self.awaited(v);
                let arg = self.tmp("step-arg");
                self.m.edge(aw, arg);
                self.m.edge(self.thrown, arg);
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
                let v = self.expr(fx, value);
                let _ = self.expr(fx, step_closure);
                self.awaited(v)
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
            | Expr::ProxyApply { .. }
            | Expr::ProxyConstruct { .. }
            | Expr::ReflectDefineMetadata { .. }
            | Expr::ReflectGetMetadata { .. }
            | Expr::ReflectGetOwnMetadata { .. }
            | Expr::ReflectHasMetadata { .. }
            | Expr::ReflectHasOwnMetadata { .. }
            | Expr::ReflectGetMetadataKeys { .. }
            | Expr::ReflectGetOwnMetadataKeys { .. }
            | Expr::ReflectDeleteMetadata { .. }
            | Expr::WorkerNew { .. } => {
                let name = variant_name(e);
                self.fail_closed(fx, e, &name)
            }
            Expr::BoxedPrimitiveNew { .. } => {
                let _ = self.prim_kids(fx, e, prim::ALL, "boxed primitive", Protocol::ToPrimitive);
                self.bobj(mi, "BoxedPrimitive").1
            }
            Expr::DateNew(..) => {
                let _ = self.prim_kids(fx, e, prim::ALL, "Date operand", Protocol::ToPrimitive);
                self.bobj(mi, "Date").1
            }
            Expr::RegExp { .. } => self.bobj(mi, "RegExp").1,
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
                read_prim(&mut self.m, p, "source", "RegExp pattern source ToString");
                read_prim(&mut self.m, p, "flags", "RegExp pattern flags ToString");
                to_prim(&mut self.m, p, "RegExp pattern ToString");
                if let Some(f) = flags {
                    let f = self.expr(fx, f);
                    to_prim(&mut self.m, f, "RegExp flags ToString");
                }
                let out = self.bobj(mi, "RegExp").1;
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
}
