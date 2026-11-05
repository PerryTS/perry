//! The exhaustive remainder of `Expr`, part: proxy and reflect, then the variants `gen_expr::expr` models.

use super::*;

impl<'a> Gen<'a> {
    pub(super) fn expr_rest_reflection(&mut self, fx: &Fx, e: &Expr) -> NodeId {
        let mi = fx.module;
        match e {
            // --------------------------------------------- proxy / reflect
            Expr::ProxyNew { target, handler } | Expr::ProxyRevocable { target, handler } => {
                // E9: the target is reached only through traps; the proxy
                // value itself is ⊤.
                let t = self.expr(fx, target);
                let r = self.m.reason("E9 Proxy target");
                self.m.cons(
                    t,
                    Cons::Hard {
                        kind: Hard::E9Proxy,
                        reason: r,
                    },
                );
                self.m.cons(t, Cons::TopSink { reason: r });
                let h = self.expr(fx, handler);
                self.to_sink(h, "E9 Proxy handler");
                self.m.top_val("E9 Proxy value")
            }
            Expr::ReflectGet {
                target,
                key,
                receiver,
            } => {
                let t = self.expr(fx, target);
                let k = self.expr(fx, key);
                let r = self.m.reason("ToPropertyKey");
                self.m.cons(
                    k,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::ToPrimitive,
                    },
                );
                let rv = self.expr(fx, receiver);
                let rr = self.m.reason("Reflect.get receiver (getters)");
                self.m.cons(
                    rv,
                    Cons::Implicit {
                        reason: rr,
                        proto: Protocol::Any,
                    },
                );
                let key = match key.as_ref() {
                    Expr::String(s) => Key::Named(self.m.atom(s)),
                    _ => Key::any(),
                };
                // DECISIONS 217: a computed key is `o[k]`'s (`IdxLoad`: the
                // key's values decide what it names). Under `__proto__`
                // Object.prototype's getter runs on the receiver.
                let out = match key {
                    Key::Any(_) => {
                        let dst = self.tmp("reflect.get");
                        self.m.cons(
                            t,
                            Cons::IdxLoad {
                                idx: k,
                                dst,
                                shape: 0,
                            },
                        );
                        dst
                    }
                    _ => self.load(t, key),
                };
                if self.m.is_proto_key(key) {
                    self.m.cons(rv, Cons::ProtoOf { dst: out });
                }
                out
            }
            Expr::ReflectSet {
                target,
                key,
                value,
                receiver,
            } => {
                let t = self.expr(fx, target);
                let k = self.expr(fx, key);
                let r = self.m.reason("ToPropertyKey");
                self.m.cons(
                    k,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::ToPrimitive,
                    },
                );
                let v = self.expr(fx, value);
                let rv = self.expr(fx, receiver);
                let key = match key.as_ref() {
                    Expr::String(s) => Key::Named(self.m.atom(s)),
                    _ => Key::any(),
                };
                self.store(fx, t, key, v, "Reflect.set".into(), None, false);
                self.store(fx, rv, key, v, "Reflect.set receiver".into(), None, false);
                self.prim_node(prim::BOOL)
            }
            Expr::ReflectDelete { target, key } => {
                let t = self.expr(fx, target);
                let _ = self.expr(fx, key);
                let r = self.m.reason("E7 Reflect.deleteProperty");
                self.m.cons(
                    t,
                    Cons::Hard {
                        kind: Hard::E7Delete,
                        reason: r,
                    },
                );
                self.prim_node(prim::BOOL)
            }
            Expr::ReflectApply {
                func,
                this_arg,
                args,
            } => {
                let f = self.expr(fx, func);
                let t = self.expr(fx, this_arg);
                let a = self.expr(fx, args);
                self.call_value(f, ThisArg::Node(t), vec![], vec![a])
            }
            Expr::ReflectConstruct {
                target,
                args,
                new_target,
            } => {
                let t = self.expr(fx, target);
                let a = self.expr(fx, args);
                let nt = self.expr(fx, new_target);
                let r = self.m.reason("E11 Reflect.construct newTarget");
                self.m.cons(nt, Cons::NewTargetHard { reason: r });
                // A new target picks the prototype of whatever the target
                // constructs, an Array exotic object included (#13645).
                self.m.array_member_write.get_or_insert(r);
                let (call, ret) = self.new_call(ThisArg::Undef, vec![], vec![a]);
                self.m.cons(t, Cons::New { call });
                ret
            }
            Expr::DynamicImport {
                paths,
                arg,
                options,
                ..
            } => {
                let a = self.expr(fx, arg);
                let r = self.m.reason("dynamic import specifier");
                self.m.cons(
                    a,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::ToPrimitive,
                    },
                );
                if let Some(o) = options {
                    let _ = self.expr(fx, o);
                }
                // DECISIONS 226: `paths` holds each specifier as written; the
                // module's dynamic import entry for it names the module, as
                // codegen's dispatch resolves it. A target outside this view
                // (native, another unit) is the fallback's ⊤.
                let out = self.tmp("dynimport");
                for p in paths {
                    self.dynimport_seen.insert((mi, p.clone()));
                }
                let imports = self.imports_of(mi as usize);
                for p in paths {
                    let loads = imports
                        .iter()
                        .find(|i| &i.source == p && (i.is_dynamic || i.is_dynamic_target))
                        .and_then(|i| i.resolved_path.as_ref())
                        .and_then(|r| self.path_index.get(r).copied());
                    // The site loads its target: its body runs.
                    if let Some(t) = loads.filter(|t| self.dynamic_only.contains(t)) {
                        let f = self.init_fn_of[&t];
                        self.m.activate(f, "import");
                    }
                    let target = loads.filter(|_| !self.dynimport_sabotage);
                    if let Some(t) = target {
                        let ns = self.mods[t as usize].ns_tok;
                        self.m.add_tok(out, ns);
                    }
                }
                // Codegen always emits the no-match fallback (a builtin
                // namespace, data, a data: URL module, a loader-hook redirect
                // of any specifier): runtime-made values, never a compiled
                // module outside `paths`.
                self.m.add_tok(out, TOP);
                self.promise_of(mi, out)
            }

            // Variants modeled in `gen_expr::expr` (never routed here).
            Expr::Undefined
            | Expr::Null
            | Expr::Bool(..)
            | Expr::Number(..)
            | Expr::Integer(..)
            | Expr::BigInt(..)
            | Expr::String(..)
            | Expr::WtfString(..)
            | Expr::I18nString { .. }
            | Expr::LocalGet(..)
            | Expr::LocalSet(..)
            | Expr::ScopedTemp { .. }
            | Expr::GlobalGet(..)
            | Expr::GlobalSet(..)
            | Expr::WithGet { .. }
            | Expr::WithSet { .. }
            | Expr::Update { .. }
            | Expr::Binary { .. }
            | Expr::Unary { .. }
            | Expr::Compare { .. }
            | Expr::Logical { .. }
            | Expr::Call { .. }
            | Expr::CallSpread { .. }
            | Expr::SuperCallSpread(..)
            | Expr::FuncRef(..)
            | Expr::ExternFuncRef { .. }
            | Expr::PropertyGet { .. }
            | Expr::PropertySet { .. }
            | Expr::PropertyUpdate { .. }
            | Expr::BufferLength(..)
            | Expr::Uint8ArrayLength(..)
            | Expr::BufferIndexGet { .. }
            | Expr::Uint8ArrayGet { .. }
            | Expr::IndexGet { .. }
            | Expr::BufferIndexSet { .. }
            | Expr::Uint8ArraySet { .. }
            | Expr::IndexSet { .. }
            | Expr::IndexUpdate { .. }
            | Expr::Object(..)
            | Expr::ObjectSpread { .. }
            | Expr::ObjectAssign { .. }
            | Expr::LiteralChunks(..)
            | Expr::LiteralChunkObject
            | Expr::LiteralSlotInit { .. }
            | Expr::Array(..)
            | Expr::ArraySpread(..)
            | Expr::Conditional { .. }
            | Expr::TypeOf(..)
            | Expr::Void(..)
            | Expr::InstanceOf { .. }
            | Expr::In { .. }
            | Expr::Await(..)
            | Expr::Yield { .. }
            | Expr::New { .. }
            | Expr::NewDynamic { .. }
            | Expr::NewDynamicSpread { .. }
            | Expr::NewTarget
            | Expr::ClassRef(..)
            | Expr::EnumMember { .. }
            | Expr::EnumImage { .. }
            | Expr::StaticFieldGet { .. }
            | Expr::StaticFieldSet { .. }
            | Expr::ClassStaticSymbolSet { .. }
            | Expr::RegisterClassParentDynamic { .. }
            | Expr::RegisterClassCaptures { .. }
            | Expr::RefreshClassExprCaptures { .. }
            | Expr::ClassEnvGet { .. }
            | Expr::ClassEnvSet { .. }
            | Expr::ClassEnvStamp { .. }
            | Expr::ClassEnvCurrent { .. }
            | Expr::ClassCaptureValue { .. }
            | Expr::RegisterClassStaticSymbol { .. }
            | Expr::RegisterClassComputedMethod { .. }
            | Expr::RegisterClassComputedAccessor { .. }
            | Expr::ClassExprFresh { .. }
            | Expr::ClassIsFirstEvaluation { .. }
            | Expr::SetFunctionPrototype { .. }
            | Expr::RegisterPrototypeMethod { .. }
            | Expr::RegisterFunctionPrototypeMethod { .. }
            | Expr::GetFunctionPrototypeMethod { .. }
            | Expr::StaticMethodCall { .. }
            | Expr::This
            | Expr::SuperCall(..)
            | Expr::SuperMethodCall { .. }
            | Expr::SuperMethodCallSpread { .. }
            | Expr::SuperPropertyGet { .. }
            | Expr::SuperPropertySet { .. }
            | Expr::ModuleTopThis
            | Expr::Sequence(..)
            | Expr::Delete(..)
            | Expr::Closure { .. }
            | Expr::ObjectRest { .. }
            | Expr::PutValueSet { .. } => self.fail_closed(fx, e, "routed"),
            _ => unreachable!("internal compiler error: expr_rest: not a reflection variant"),
        }
    }
}
