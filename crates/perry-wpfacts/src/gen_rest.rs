//! Constraint generation, part 4: the exhaustive remainder of `Expr`.
//!
//! This match has NO wildcard arm. Variants `gen_expr::expr` already models
//! are listed in the final arm and fail closed if ever routed here. Groups:
//! - primitive-only operations: operands get the protocol touch, the result
//!   is primitive (or a fresh container of primitives);
//! - builtin data structures (arrays, maps, sets, promises …): modeled flows;
//! - everything else fails closed (operands escape, result ⊤).

use crate::gen::{variant_name, Fx, Gen};
use crate::model::*;
use perry_hir::{Expr, Stmt};

#[path = "gen/accessor_descriptor.rs"]
mod accessor_descriptor;
mod nodes;

fn kids(e: &Expr) -> Vec<&Expr> {
    let mut v = Vec::new();
    perry_hir::walker::walk_expr_children(e, &mut |c| v.push(c));
    v
}

impl<'a> Gen<'a> {
    /// An accessor descriptor literal (`accessor_descriptor`): its class, its
    /// constructor arguments, and which of them its own `get` and `set` are
    /// (a literal shape's constructor stores parameter i under its key).
    fn accessor_literal<'e>(
        &self,
        mi: u32,
        d: &'e Expr,
    ) -> Option<(&'e str, &'e [Expr], Option<usize>, Option<usize>)> {
        let Expr::New {
            class_name, args, ..
        } = d
        else {
            return None;
        };
        if !self.accessor_descriptor(mi, d)
            || args.iter().any(|a| matches!(a, Expr::LiteralChunks(_)))
        {
            return None;
        }
        let module = self.modules[mi as usize].1;
        let ctor = module
            .classes
            .iter()
            .find(|c| &c.name == class_name)?
            .constructor
            .as_ref()?;
        let arg = |key: &str| {
            ctor.body.iter().find_map(|s| match s {
                Stmt::Expr(Expr::PropertySet {
                    object,
                    property,
                    value,
                    ..
                }) if matches!(object.as_ref(), Expr::This) && property == key => {
                    match value.as_ref() {
                        Expr::LocalGet(p) => ctor.params.iter().position(|q| q.id == *p),
                        _ => None,
                    }
                }
                _ => None,
            })
        };
        Some((class_name, args, arg("get"), arg("set")))
    }


    /// The class whose method home `home` reads, when it is that home.
    fn class_home_of(&self, fx: &Fx, home: &Expr) -> Option<TmplId> {
        match home {
            Expr::LocalGet(id) => self.class_homes.get(&(fx.module, *id)).copied(),
            _ => None,
        }
    }

    pub(crate) fn expr_rest(&mut self, fx: &Fx, e: &Expr) -> NodeId {
        let mi = fx.module;
        match e {
            Expr::NativeModuleRef(m) => {
                // node:module as a value hands out `Module._cache`.
                if m.strip_prefix("node:").unwrap_or(m) == "module" {
                    let cache = self.m.cjs_cache;
                    self.to_sink(cache, "node:module names Module._cache");
                }
                self.m.top_val(&format!("native module ref {m}"))
            }
            Expr::NativeMethodCall {
                module,
                object,
                method,
                args,
                ..
            } => {
                let o = object.as_ref().map(|o| self.expr(fx, o));
                let a: Vec<NodeId> = args.iter().map(|x| self.expr(fx, x)).collect();
                let m0 = module.strip_prefix("node:").unwrap_or(module);
                if module == "__perry_runtime" {
                    self.runtime_glue(fx, method, o, a, args)
                } else if m0 == "module" && method == "createRequire" && object.is_none() {
                    let t = self.m.tok(TokKind::Builtin(BuiltinNs::Require));
                    self.tok_node(t)
                } else {
                    if module == "vm" || module == "node:vm" {
                        crate::builtins::set_dyn_eval(&mut self.m, "E14 node:vm");
                    }
                    let why = format!("argument of native module {module}.{method}");
                    for n in o.into_iter().chain(a) {
                        self.to_sink(n, &why);
                    }
                    self.native_success_result(module, object.is_some(), method, args)
                }
            }
            Expr::PrivateLexicalBrand(x) | Expr::PrivateLexicalBrandPush(x) => {
                let _ = self.expr(fx, x);
                self.prim_node(prim::UNDEF | prim::BOOL)
            }
            Expr::PrivateLexicalBrandPop => self.prim_node(prim::UNDEF),
            Expr::PrivateBrandCheck { object, .. } => {
                let _ = self.expr(fx, object);
                self.prim_node(prim::BOOL)
            }
            Expr::PrivateGuard { object, .. } => {
                let o = self.expr(fx, object);
                let out = self.tmp("privguard");
                self.m.edge(o, out);
                self.m.add_prim(out, prim::ALL);
                out
            }
            Expr::ClassMethodHome { is_static, .. } => match fx.class {
                Some(tm) => {
                    let t = if *is_static {
                        self.m.tok(TokKind::ClassObj(tm))
                    } else {
                        self.m.tok(TokKind::Proto(tm))
                    };
                    self.tok_node(t)
                }
                None => self.fail_closed(fx, e, "class method home outside a class"),
            },
            Expr::ObjectSuperPropertyGet {
                home,
                key,
                receiver,
            } => match self.class_home_of(fx, home) {
                Some(tm) => {
                    let _ = self.expr(fx, key);
                    let k = self.key_of(key);
                    let r = self.expr(fx, receiver);
                    let dst = self.tmp("super.get");
                    for start in self.super_starts_of(tm) {
                        self.m.cons(
                            r,
                            Cons::Super {
                                start,
                                key: k,
                                act: ChainAct::Load(dst),
                            },
                        );
                    }
                    dst
                }
                None => self.fail_closed(fx, e, "object-literal super"),
            },
            Expr::ObjectSuperPropertySet {
                home,
                key,
                value,
                receiver,
            } => match self.class_home_of(fx, home) {
                Some(tm) => {
                    let _ = self.expr(fx, key);
                    let k = self.key_of(key);
                    let r = self.expr(fx, receiver);
                    let v = self.expr(fx, value);
                    self.store(fx, r, k, v, "super.x=".into(), None, false);
                    for start in self.super_starts_of(tm) {
                        self.m.cons(
                            r,
                            Cons::Super {
                                start,
                                key: k,
                                act: ChainAct::Store(v),
                            },
                        );
                    }
                    v
                }
                None => self.fail_closed(fx, e, "object-literal super"),
            },
            Expr::ObjectSuperMethodCall {
                home,
                key,
                receiver,
                args,
            } => match self.class_home_of(fx, home) {
                Some(tm) => {
                    let _ = self.expr(fx, key);
                    let k = self.key_of(key);
                    let r = self.expr(fx, receiver);
                    let a = self.evaluate_all(fx, args);
                    let sp = Vec::new();
                    let (call, ret) = self.new_call(ThisArg::Node(r), a, sp);
                    for start in self.super_starts_of(tm) {
                        self.m.cons(
                            r,
                            Cons::Super {
                                start,
                                key: k,
                                act: ChainAct::Call(call),
                            },
                        );
                    }
                    ret
                }
                None => self.fail_closed(fx, e, "object-literal super"),
            },
            Expr::EnvGet(_) | Expr::EnvGetDynamic(_) => self.prim_kids(
                fx,
                e,
                prim::STR | prim::UNDEF,
                "process.env key",
                Protocol::ToPrimitive,
            ),
            Expr::ProcessEnv
            | Expr::ProcessArgv
            | Expr::ProcessMemoryUsage
            | Expr::ProcessVersions
            | Expr::ProcessHrtime(..)
            | Expr::ProcessThreadCpuUsage(..)
            | Expr::ProcessCpuUsage(..)
            | Expr::ProcessResourceUsage
            | Expr::ProcessActiveResourcesInfo
            | Expr::PathParse(..)
            | Expr::OsCpus
            | Expr::OsNetworkInterfaces
            | Expr::OsUserInfo
            | Expr::OsUserInfoBuffer
            | Expr::OsLoadavg
            | Expr::RegExpExecGroups
            | Expr::ObjectKeys(..)
            | Expr::ForInKeys(..)
            | Expr::ObjectGetOwnPropertyNames(..)
            | Expr::ObjectGetOwnPropertySymbols(..)
            | Expr::ReflectOwnKeys(..)
            | Expr::StringSplit(..)
            | Expr::StringMatch { .. }
            | Expr::StringMatchAll { .. }
            | Expr::TaggedTemplateStrings { .. }
            | Expr::TemplateRaw(..)
            | Expr::CryptoRandomBytes(..)
            | Expr::TextEncoderEncodeInto { .. }
            | Expr::UrlGetSearchParams(..)
            | Expr::UrlSearchParamsGetAll { .. }
            | Expr::UrlSearchParamsEntries(..)
            | Expr::UrlSearchParamsKeys(..)
            | Expr::UrlSearchParamsValues(..)
            | Expr::UrlParse(..)
            | Expr::UrlParseWithBase { .. } => {
                // Own keys and enumeration run no lookup; the others read
                // elements or dispatch through `@@split` / `@@match`.
                let proto = match e {
                    Expr::ObjectKeys(..)
                    | Expr::ForInKeys(..)
                    | Expr::ObjectGetOwnPropertyNames(..)
                    | Expr::ObjectGetOwnPropertySymbols(..)
                    | Expr::ReflectOwnKeys(..) => Protocol::OwnKeys,
                    _ => Protocol::Any,
                };
                self.primbox_kids(fx, e, "primitive container builtin", proto)
            }
            Expr::NamespacePatched(_) => self.prim_node(prim::BOOL),
            Expr::GlobalThisExpr => {
                let t = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
                self.tok_node(t)
            }
            Expr::ProcessStdin | Expr::ProcessStdout | Expr::ProcessStderr => {
                self.m.top_val("process stdio")
            }
            Expr::ProcessNextTick { callback, args } => {
                let cb = self.expr(fx, callback);
                let a: Vec<NodeId> = args.iter().map(|x| self.expr(fx, x)).collect();
                let _ = self.callback(cb, a);
                self.prim_node(prim::UNDEF)
            }
            Expr::QueueMicrotask(cb) => {
                let c = self.expr(fx, cb);
                let _ = self.callback(c, vec![]);
                self.prim_node(prim::UNDEF)
            }
            Expr::ProcessOn { event, handler }
            | Expr::ProcessOnce { event, handler }
            | Expr::ProcessStdinOn { event, handler }
            | Expr::ProcessStdinRemoveListener { event, handler }
            | Expr::ProcessStdoutOn { event, handler } => {
                let _ = self.expr(fx, event);
                let h = self.expr(fx, handler);
                self.to_sink(h, "process event handler (called by the runtime)");
                self.m.top_val("process event registration result")
            }
            Expr::ProcessUptime
            | Expr::ProcessCwd
            | Expr::ProcessPid
            | Expr::ProcessPpid
            | Expr::ProcessVersion
            | Expr::ProcessHrtimeBigint
            | Expr::ProcessChdir(..)
            | Expr::ProcessKill { .. }
            | Expr::ProcessExit(..)
            | Expr::ProcessAbort
            | Expr::ProcessUmask(..)
            | Expr::ProcessAvailableMemory
            | Expr::ProcessConstrainedMemory
            | Expr::ProcessPosixCredential(..)
            | Expr::ProcessEmitWarning(..)
            | Expr::ProcessTitle
            | Expr::ProcessSetTitle(..)
            | Expr::ProcessStdinSetRawMode(..)
            | Expr::ProcessStdinLifecycle(..)
            | Expr::ProcessStdinIsTTY
            | Expr::ProcessStdoutIsTTY
            | Expr::ProcessStderrIsTTY
            | Expr::ProcessStdoutColumns
            | Expr::ProcessStdoutRows
            | Expr::TtyIsAtty(..)
            | Expr::FsReadFileSync(..)
            | Expr::FsWriteFileSync(..)
            | Expr::FsExistsSync(..)
            | Expr::FsMkdirSync(..)
            | Expr::FsUnlinkSync(..)
            | Expr::FsAppendFileSync(..)
            | Expr::FsRmRecursive(..)
            | Expr::PathJoin(..)
            | Expr::PathDirname(..)
            | Expr::PathBasename(..)
            | Expr::PathBasenameExt(..)
            | Expr::PathExtname(..)
            | Expr::PathResolve(..)
            | Expr::PathIsAbsolute(..)
            | Expr::PathRelative(..)
            | Expr::PathNormalize(..)
            | Expr::PathFormat(..)
            | Expr::PathSep
            | Expr::PathDelimiter
            | Expr::PathToNamespacedPath(..)
            | Expr::PathMatchesGlob(..)
            | Expr::PathResolveJoin(..)
            | Expr::PathWin32Join(..)
            | Expr::PathWin32 { .. }
            | Expr::ObjectIsFrozen(..)
            | Expr::ObjectIsSealed(..)
            | Expr::ObjectIsExtensible(..)
            | Expr::ReflectIsExtensible(..)
            | Expr::SymbolNew(..)
            | Expr::SymbolFor(..)
            | Expr::SymbolKeyFor(..)
            | Expr::SymbolDescription(..)
            | Expr::RegExpEscape(..)
            | Expr::SymbolToString(..)
            | Expr::FileURLToPath(..)
            | Expr::RegExpExecIndex
            | Expr::JsonIsRawJson(..)
            | Expr::PerformanceNow
            | Expr::WebAssemblyValidate(..)
            | Expr::Atob(..)
            | Expr::Btoa(..)
            | Expr::TextDecoderDecode { .. }
            | Expr::TextDecoderEncoding(..)
            | Expr::TextDecoderFatal(..)
            | Expr::TextDecoderIgnoreBom(..)
            | Expr::EncodeURI(..)
            | Expr::DecodeURI(..)
            | Expr::EncodeURIComponent(..)
            | Expr::DecodeURIComponent(..)
            | Expr::IterResultGetDone
            | Expr::CryptoRandomUUID
            | Expr::CryptoRandomUUIDv7
            | Expr::CryptoSha256(..)
            | Expr::CryptoMd5(..)
            | Expr::OsPlatform
            | Expr::OsArch
            | Expr::OsHostname
            | Expr::OsHomedir
            | Expr::OsTmpdir
            | Expr::OsTotalmem
            | Expr::OsFreemem
            | Expr::OsUptime
            | Expr::OsType
            | Expr::OsRelease
            | Expr::OsEOL
            | Expr::OsDevNull
            | Expr::OsAvailableParallelism
            | Expr::OsEndianness
            | Expr::OsMachine
            | Expr::OsVersion
            | Expr::BufferIsBuffer(..)
            | Expr::BufferIsEncoding(..)
            | Expr::BufferByteLength { .. }
            | Expr::BufferToString { .. }
            | Expr::BufferCopy { .. }
            | Expr::BufferWrite { .. }
            | Expr::BufferFill { .. }
            | Expr::BufferEquals { .. }
            | Expr::PodLayoutSizeOf { .. }
            | Expr::PodLayoutAlignOf { .. }
            | Expr::PodLayoutOffsetOf { .. }
            | Expr::ChildProcessGetProcessStatus(..)
            | Expr::ChildProcessKillProcess(..)
            | Expr::ArrayIndexOf { .. }
            | Expr::ArrayLastIndexOf { .. }
            | Expr::ArrayIncludes { .. }
            | Expr::StringFromCharCode(..)
            | Expr::StringFromCharCodeSpread(..)
            | Expr::StringFromCodePoint(..)
            | Expr::StringRaw { .. }
            | Expr::StringAt { .. }
            | Expr::StringCodePointAt { .. }
            | Expr::MapHas { .. }
            | Expr::MapDelete { .. }
            | Expr::MapSize(..)
            | Expr::MapClear(..)
            | Expr::SetHas { .. }
            | Expr::SetDelete { .. }
            | Expr::SetSize(..)
            | Expr::SetClear(..)
            | Expr::DateNow
            | Expr::DateGetTime(..)
            | Expr::DateToISOString(..)
            | Expr::DateGetFullYear(..)
            | Expr::DateGetMonth(..)
            | Expr::DateGetDate(..)
            | Expr::DateGetDay(..)
            | Expr::DateGetHours(..)
            | Expr::DateGetMinutes(..)
            | Expr::DateGetSeconds(..)
            | Expr::DateGetMilliseconds(..)
            | Expr::DateParse(..)
            | Expr::DateUtc(..)
            | Expr::DateGetUtcDay(..)
            | Expr::DateGetUtcFullYear(..)
            | Expr::DateGetUtcMonth(..)
            | Expr::DateGetUtcDate(..)
            | Expr::DateGetUtcHours(..)
            | Expr::DateGetUtcMinutes(..)
            | Expr::DateGetUtcSeconds(..)
            | Expr::DateGetUtcMilliseconds(..)
            | Expr::DateSetUtcFullYear { .. }
            | Expr::DateSetUtcMonth { .. }
            | Expr::DateSetUtcDate { .. }
            | Expr::DateSetUtcHours { .. }
            | Expr::DateSetUtcMinutes { .. }
            | Expr::DateSetUtcSeconds { .. }
            | Expr::DateSetUtcMilliseconds { .. }
            | Expr::DateSetFullYear { .. }
            | Expr::DateSetMonth { .. }
            | Expr::DateSetDate { .. }
            | Expr::DateSetHours { .. }
            | Expr::DateSetMinutes { .. }
            | Expr::DateSetSeconds { .. }
            | Expr::DateSetMilliseconds { .. }
            | Expr::DateSetTime { .. }
            | Expr::DateValueOf(..)
            | Expr::DateToString(..)
            | Expr::DateToDateString(..)
            | Expr::DateToTimeString(..)
            | Expr::DateToUTCString(..)
            | Expr::DateToLocaleDateString(..)
            | Expr::DateToLocaleTimeString(..)
            | Expr::DateToLocaleString(..)
            | Expr::DateGetTimezoneOffset(..)
            | Expr::DateToJSON(..)
            | Expr::UrlGetHref(..)
            | Expr::UrlGetPathname(..)
            | Expr::UrlGetProtocol(..)
            | Expr::UrlGetHost(..)
            | Expr::UrlGetHostname(..)
            | Expr::UrlGetPort(..)
            | Expr::UrlGetSearch(..)
            | Expr::UrlGetHash(..)
            | Expr::UrlGetOrigin(..)
            | Expr::UrlCanParse(..)
            | Expr::UrlCanParseWithBase { .. }
            | Expr::UrlInstanceToString(..)
            | Expr::UrlInstanceToJSON(..)
            | Expr::UrlSetPathname { .. }
            | Expr::UrlSetSearch { .. }
            | Expr::UrlSetHash { .. }
            | Expr::UrlSetProtocol { .. }
            | Expr::UrlSetHostname { .. }
            | Expr::UrlSetPort { .. }
            | Expr::UrlSetUsername { .. }
            | Expr::UrlSetPassword { .. }
            | Expr::UrlSetHref { .. }
            | Expr::UrlSearchParamsMissingArgs { .. }
            | Expr::UrlSearchParamsGet { .. }
            | Expr::UrlSearchParamsHas { .. }
            | Expr::UrlSearchParamsSet { .. }
            | Expr::UrlSearchParamsAppend { .. }
            | Expr::UrlSearchParamsDelete { .. }
            | Expr::UrlSearchParamsToString(..)
            | Expr::UrlSearchParamsSort(..)
            | Expr::ArrayIterationPatched
            | Expr::ImportMetaUrl(..)
            | Expr::ProxyRevoke(..) => {
                // Mixed: element reads, `raw` / `toISOString` lookups, spreads.
                self.prim_kids(fx, e, prim::ALL, "primitive builtin operand", Protocol::Any)
            }
            // Every Math function returns a Number (ECMA-262 Math).
            Expr::MathFloor(..)
            | Expr::MathCeil(..)
            | Expr::MathRound(..)
            | Expr::MathTrunc(..)
            | Expr::MathSign(..)
            | Expr::MathAbs(..)
            | Expr::MathSqrt(..)
            | Expr::MathLog(..)
            | Expr::MathLog2(..)
            | Expr::MathLog10(..)
            | Expr::MathPow(..)
            | Expr::MathMin(..)
            | Expr::MathMax(..)
            | Expr::MathImul(..)
            | Expr::MathRandom
            | Expr::MathSin(..)
            | Expr::MathCos(..)
            | Expr::MathTan(..)
            | Expr::MathAsin(..)
            | Expr::MathAcos(..)
            | Expr::MathAtan(..)
            | Expr::MathAtan2(..)
            | Expr::MathCbrt(..)
            | Expr::MathHypot(..)
            | Expr::MathFround(..)
            | Expr::MathF16round(..)
            | Expr::MathClz32(..)
            | Expr::MathExpm1(..)
            | Expr::MathLog1p(..)
            | Expr::MathSinh(..)
            | Expr::MathCosh(..)
            | Expr::MathTanh(..)
            | Expr::MathAsinh(..)
            | Expr::MathAcosh(..)
            | Expr::MathAtanh(..)
            | Expr::MathExp(..) => self.prim_kids(
                fx,
                e,
                prim::NUM,
                "primitive builtin operand",
                Protocol::ToPrimitive,
            ),
            // The global conversions and predicates: the operation fixes the
            // kind of its result (ECMA-262; the HIR operation's contract).
            Expr::ParseInt { .. } | Expr::ParseFloat(..) | Expr::NumberCoerce(..) => self
                .prim_kids(
                    fx,
                    e,
                    prim::NUM,
                    "primitive builtin operand",
                    Protocol::ToPrimitive,
                ),
            // The spread is iterated, then each element is ToNumber'd.
            Expr::MathMinSpread(..) | Expr::MathMaxSpread(..) => {
                self.prim_kids(fx, e, prim::NUM, "spread Math operand", Protocol::Any)
            }
            Expr::BigIntCoerce(..) => self.prim_kids(
                fx,
                e,
                prim::BIGINT,
                "primitive builtin operand",
                Protocol::ToPrimitive,
            ),
            Expr::StringCoerce(..) | Expr::TemplateStringCoerce(..) => self.prim_kids(
                fx,
                e,
                prim::STR,
                "primitive builtin operand",
                Protocol::ToPrimitive,
            ),
            // The elements are read and each is ToString'd.
            Expr::ArrayJoin { .. } => {
                self.prim_kids(fx, e, prim::STR, "array join operand", Protocol::Any)
            }
            Expr::ObjectIs(..)
            | Expr::ObjectHasOwn(..)
            | Expr::ArrayIsArray(..)
            | Expr::BooleanCoerce(..)
            | Expr::IsNaN(..)
            | Expr::IsUndefinedOrBareNan(..)
            | Expr::IsFinite(..)
            | Expr::NumberIsNaN(..)
            | Expr::NumberIsFinite(..)
            | Expr::NumberIsInteger(..)
            | Expr::NumberIsSafeInteger(..)
            | Expr::ReflectHas { .. } => self.prim_kids(
                fx,
                e,
                prim::BOOL,
                "primitive builtin operand",
                Protocol::ToPrimitive,
            ),

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
                match self.for_in_key_of(mi, k).filter(|_| key.is_none()) {
                    Some(kp) => {
                        // An accessor literal's own `get` / `set` is its
                        // constructor argument: each run defines its own (the
                        // literal's token, one per birth site, holds them all).
                        let (dn, own) = match self.accessor_literal(mi, d) {
                            Some((class_name, args, get, set)) => {
                                let a = self.evaluate_all(fx, args);
                                let own = [get, set].map(|i| i.and_then(|i| a.get(i).copied()));
                                (self.new_named(fx, class_name, a, vec![]), own)
                            }
                            None => (self.expr(fx, d), [None, None]),
                        };
                        self.define_keyed(fx, on, kp, dn, r, accessor, own);
                    }
                    None => {
                        let dn = self.expr(fx, d);
                        self.define_property(fx, on, key, dn, r, accessor);
                    }
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
                let va = self.m.atom("value");
                let value = self.load(descs, Key::Named(va));
                let site = self.store_site(fx.module, fx.f, "defineProperty".into(), None, false);
                let star = self.m.atom(crate::STAR_KEY);
                for (name, setter) in [("get", false), ("set", true)] {
                    let a = self.m.atom(name);
                    let f = self.load(descs, Key::Named(a));
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
                let _ = self.expr(fx, k);
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
                });
                let t = self.m.tok(TokKind::Obj(tm, NO_CTX));
                let pn = self.m.proto_node(t);
                self.m.edge(p, pn);
                let n = self.tok_node(t);
                if let Some(props) = props {
                    let pv = self.expr(fx, props);
                    let descs = self.load(pv, Key::Any);
                    self.define_property(fx, n, None, descs, r, false);
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
                let vals = self.load(v, Key::Any);
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
            Expr::TextEncoderNew | Expr::TextDecoderNew { .. } => {
                let _ = kids(e).into_iter().map(|k| self.expr(fx, k)).count();
                self.bobj(mi, "TextCodec").1
            }
            Expr::TextEncoderEncode(..)
            | Expr::BufferFrom { .. }
            | Expr::BufferAlloc { .. }
            | Expr::BufferAllocUnsafe(..)
            | Expr::Uint8ArrayNew(..)
            | Expr::Uint8ArrayFrom(..)
            | Expr::TypedArrayNew { .. }
            | Expr::FsReadFileBinary(..) => {
                // Typed arrays / buffers hold numbers; an array argument is
                // read, a buffer/view argument is shared (B1, flow only).
                let ks = kids(e);
                let (t, n) = self.bobj(mi, "TypedArray");
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
                // reading the source runs getters.
                let v = self.expr(fx, value);
                let r = self.m.reason("structuredClone source");
                self.m.cons(
                    v,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::Any,
                    },
                );
                let o = self.expr(fx, options);
                let rb = self.m.reason("B3 transfer list");
                let tl = self.load(o, Key::Any);
                let items = self.load(tl, Key::Any);
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
                i32_controls,
                i1_controls,
                ..
            } => {
                let o = self.expr(fx, obj);
                // The generator object is its step closure.
                for t in self.m.nodes[o as usize].pts.to_sorted() {
                    if let TokKind::Func(f, _) = self.m.toks[t as usize] {
                        self.m.funcs[f as usize].gen_step = true;
                    }
                }
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
                self.callback(s, vec![arg, b])
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
                let r = self.callback(s, vec![u, b]);
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
                let r = self.callback(s, vec![arg, b]);
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
            | Expr::UrlSearchParamsForEach { .. }
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
            Expr::UrlNew { .. } | Expr::UrlPatternNew { .. } | Expr::UrlSearchParamsNew(..) => {
                let _ = self.prim_kids(
                    fx,
                    e,
                    prim::ALL,
                    "URL constructor operand",
                    Protocol::ToPrimitive,
                );
                self.bobj(mi, "URL").1
            }
            Expr::BoxedPrimitiveNew { .. } => {
                let _ = self.prim_kids(fx, e, prim::ALL, "boxed primitive", Protocol::ToPrimitive);
                self.bobj(mi, "BoxedPrimitive").1
            }
            Expr::DateNew(..) => {
                let _ = self.prim_kids(fx, e, prim::ALL, "Date operand", Protocol::ToPrimitive);
                self.bobj(mi, "Date").1
            }
            Expr::RegExp { .. } | Expr::RegExpDynamic { .. } => {
                let _ = self.prim_kids(fx, e, prim::ALL, "RegExp operand", Protocol::Any);
                self.bobj(mi, "RegExp").1
            }
            Expr::ErrorNew(..)
            | Expr::TypeErrorNew(..)
            | Expr::RangeErrorNew(..)
            | Expr::ReferenceErrorNew(..)
            | Expr::SyntaxErrorNew(..) => {
                let _ = self.prim_kids(fx, e, prim::ALL, "Error message", Protocol::Any);
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
            Expr::StringReplace {
                string,
                pattern,
                replacement,
            } => {
                let _ = self.prim_op(
                    fx,
                    &[string, pattern],
                    prim::STR,
                    "String.replace",
                    Protocol::Any,
                );
                let r = self.expr(fx, replacement);
                let s = self.prim_node(prim::STR);
                let ret = self.callback(r, vec![s; 4]);
                let why = self.m.reason("replace callback result ToString");
                self.m.cons(
                    ret,
                    Cons::Implicit {
                        reason: why,
                        proto: Protocol::ToPrimitive,
                    },
                );
                let rc = self.m.reason("replacement ToString");
                self.m.cons(
                    r,
                    Cons::Implicit {
                        reason: rc,
                        proto: Protocol::ToPrimitive,
                    },
                );
                self.prim_node(prim::STR)
            }

            // ------------------------------------------------------ arrays
            Expr::ArrayPush {
                array_id, value, ..
            }
            | Expr::ArrayUnshift { array_id, value } => {
                let a = self.local(mi, *array_id);
                let v = self.expr(fx, value);
                self.elem_store(fx, a, v, "push");
                self.prim_node(prim::NUM)
            }
            Expr::ArrayPushSpread { array_id, source } => {
                let a = self.local(mi, *array_id);
                let s = self.expr(fx, source);
                let it = self.iterate(s);
                self.elem_store(fx, a, it, "push...");
                self.prim_node(prim::NUM)
            }
            Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
                let a = self.local(mi, *id);
                let out = self.elem_load(a);
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            Expr::ArraySlice {
                array, start, end, ..
            } => {
                let a = self.expr(fx, array);
                let _ = self.prim_op(
                    fx,
                    &[start],
                    prim::NUM,
                    "slice index",
                    Protocol::ToPrimitive,
                );
                if let Some(x) = end {
                    let _ = self.prim_op(fx, &[x], prim::NUM, "slice index", Protocol::ToPrimitive);
                }
                let el = self.elem_load(a);
                self.new_arr_from(mi, "slice", &[el])
            }
            Expr::ArraySplice {
                array_id,
                start,
                delete_count,
                items,
            } => {
                let a = self.local(mi, *array_id);
                let _ = self.prim_op(
                    fx,
                    &[start],
                    prim::NUM,
                    "splice index",
                    Protocol::ToPrimitive,
                );
                if let Some(d) = delete_count {
                    let _ =
                        self.prim_op(fx, &[d], prim::NUM, "splice count", Protocol::ToPrimitive);
                }
                for it in items {
                    let v = self.expr(fx, it);
                    self.elem_store(fx, a, v, "splice");
                }
                let el = self.elem_load(a);
                self.new_arr_from(mi, "splice", &[el])
            }
            Expr::ArrayForEach { array, callback } => {
                let _ = self.array_cb(fx, array, callback);
                self.prim_node(prim::UNDEF)
            }
            Expr::ArrayMap { array, callback } => {
                let (_, _, ret) = self.array_cb(fx, array, callback);
                self.new_arr_from(mi, "map", &[ret])
            }
            Expr::ArrayFlatMap { array, callback } => {
                let (_, _, ret) = self.array_cb(fx, array, callback);
                let inner = self.elem_load(ret);
                self.new_arr_from(mi, "flatMap", &[ret, inner])
            }
            Expr::ArrayFilter { array, callback } => {
                let (_, el, _) = self.array_cb(fx, array, callback);
                self.new_arr_from(mi, "filter", &[el])
            }
            Expr::ArrayFind { array, callback } | Expr::ArrayFindLast { array, callback } => {
                let (_, el, _) = self.array_cb(fx, array, callback);
                let out = self.tmp("find");
                self.m.edge(el, out);
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            Expr::ArrayFindIndex { array, callback }
            | Expr::ArrayFindLastIndex { array, callback } => {
                let _ = self.array_cb(fx, array, callback);
                self.prim_node(prim::NUM)
            }
            Expr::ArraySome { array, callback } | Expr::ArrayEvery { array, callback } => {
                let _ = self.array_cb(fx, array, callback);
                self.prim_node(prim::BOOL)
            }
            Expr::ArraySort { array, comparator } => {
                let a = self.expr(fx, array);
                let c = self.expr(fx, comparator);
                let el = self.elem_load(a);
                let r = self.callback(c, vec![el, el]);
                let why = self.m.reason("sort comparator result");
                self.m.cons(
                    r,
                    Cons::Implicit {
                        reason: why,
                        proto: Protocol::ToPrimitive,
                    },
                );
                a
            }
            Expr::ArrayToSorted { array, comparator } => {
                let a = self.expr(fx, array);
                let el = self.elem_load(a);
                if let Some(c) = comparator {
                    let cn = self.expr(fx, c);
                    let _ = self.callback(cn, vec![el, el]);
                }
                self.new_arr_from(mi, "toSorted", &[el])
            }
            Expr::ArrayReduce {
                array,
                callback,
                initial,
            }
            | Expr::ArrayReduceRight {
                array,
                callback,
                initial,
            } => {
                let a = self.expr(fx, array);
                let cb = self.expr(fx, callback);
                let el = self.elem_load(a);
                let acc = self.tmp("acc");
                match initial {
                    Some(i) => {
                        let iv = self.expr(fx, i);
                        self.m.edge(iv, acc);
                    }
                    None => self.m.edge(el, acc),
                }
                let num = self.prim_node(prim::NUM);
                let ret = self.callback(cb, vec![acc, el, num, a]);
                self.m.edge(ret, acc);
                acc
            }
            Expr::ArrayFlat { array } => {
                let a = self.expr(fx, array);
                let el = self.elem_load(a);
                let inner = self.elem_load(el);
                self.new_arr_from(mi, "flat", &[el, inner])
            }
            Expr::ArrayToReversed { array } => {
                let a = self.expr(fx, array);
                let el = self.elem_load(a);
                self.new_arr_from(mi, "copy", &[el])
            }
            Expr::ArrayValues(array) => {
                let a = self.expr(fx, array);
                let ek = self.m.elem_key();
                let el = self.load(a, ek);
                self.new_iter_from(mi, &[el])
            }
            Expr::ArrayToSpliced {
                array,
                start,
                delete_count,
                items,
            } => {
                let a = self.expr(fx, array);
                let _ = self.prim_op(
                    fx,
                    &[start, delete_count],
                    prim::NUM,
                    "toSpliced index",
                    Protocol::ToPrimitive,
                );
                let el = self.elem_load(a);
                let mut parts = vec![el];
                for it in items {
                    parts.push(self.expr(fx, it));
                }
                self.new_arr_from(mi, "toSpliced", &parts)
            }
            Expr::ArrayWith {
                array,
                index,
                value,
            } => {
                let a = self.expr(fx, array);
                let _ = self.prim_op(fx, &[index], prim::NUM, "with index", Protocol::ToPrimitive);
                let v = self.expr(fx, value);
                let el = self.elem_load(a);
                self.new_arr_from(mi, "with", &[el, v])
            }
            Expr::ArrayAt { array, index } => {
                let a = self.expr(fx, array);
                let _ = self.prim_op(fx, &[index], prim::NUM, "at index", Protocol::ToPrimitive);
                let out = self.elem_load(a);
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            Expr::ArrayReverseValue { receiver } => self.expr(fx, receiver),
            Expr::ArrayCopyWithin {
                array_id,
                target,
                start,
                end,
            } => {
                let _ = self.prim_op(
                    fx,
                    &[target, start],
                    prim::NUM,
                    "copyWithin",
                    Protocol::ToPrimitive,
                );
                if let Some(x) = end {
                    let _ = self.prim_op(fx, &[x], prim::NUM, "copyWithin", Protocol::ToPrimitive);
                }
                self.local(mi, *array_id)
            }
            Expr::ArrayCopyWithinValue {
                receiver,
                target,
                start,
                end,
            } => {
                let r = self.expr(fx, receiver);
                let _ = self.prim_op(
                    fx,
                    &[target, start],
                    prim::NUM,
                    "copyWithin",
                    Protocol::ToPrimitive,
                );
                if let Some(x) = end {
                    let _ = self.prim_op(fx, &[x], prim::NUM, "copyWithin", Protocol::ToPrimitive);
                }
                r
            }
            Expr::ArrayEntries(array) => {
                let a = self.expr(fx, array);
                let ek = self.m.elem_key();
                let el = self.load(a, ek);
                let num = self.prim_node(prim::NUM);
                let pair = self.new_arr_from(mi, "entry", &[el, num]);
                self.new_iter_from(mi, &[pair])
            }
            Expr::ArrayKeys(array) => {
                let _ = self.expr(fx, array);
                let num = self.prim_node(prim::NUM);
                self.new_iter_from(mi, &[num])
            }
            Expr::ArrayLikeMethod { receiver, args, .. } => {
                let r = self.expr(fx, receiver);
                let why = self.m.reason("Array.prototype method on an array-like");
                self.m.cons(
                    r,
                    Cons::Implicit {
                        reason: why,
                        proto: Protocol::Any,
                    },
                );
                let el = self.load(r, Key::Any);
                let num = self.prim_node(prim::NUM);
                let mut outs = vec![el];
                for a in args {
                    let an = self.expr(fx, a);
                    let ret = self.callback(an, vec![el, num, r]);
                    outs.push(ret);
                    // Array-likes may also receive the arguments as elements.
                    self.elem_store(fx, r, an, "array-like method");
                }
                let arr = self.new_arr_from(mi, "arraylike", &outs);
                let out = self.tmp("arraylike-out");
                self.m.edge(arr, out);
                self.m.edge(r, out);
                self.m.add_prim(out, prim::ALL);
                out
            }

            // ---------------------------------------------- maps and sets
            Expr::MapNew | Expr::SetNew => self.bobj(mi, "Collection").1,
            Expr::MapNewFromArray(x) => {
                // Entries `[key, value]`: the map holds their contents.
                let v = self.expr(fx, x);
                let it = self.iterate(v);
                let ek = self.m.elem_key();
                let inner = self.load(it, ek);
                let (t, n) = self.bobj(mi, "Collection");
                let fa = self.m.field_any(t);
                self.m.edge(inner, fa);
                n
            }
            Expr::SetNewFromArray(x) => {
                let v = self.expr(fx, x);
                let it = self.iterate(v);
                let (t, n) = self.bobj(mi, "Collection");
                let fa = self.m.field_any(t);
                self.m.edge(it, fa);
                n
            }
            Expr::MapSet { map, key, value } => {
                let m = self.expr(fx, map);
                let k = self.expr(fx, key);
                let v = self.expr(fx, value);
                self.elem_store(fx, m, k, "map.set key");
                self.elem_store(fx, m, v, "map.set");
                m
            }
            Expr::SetAdd { set_id, value } => {
                let s = self.local(mi, *set_id);
                let v = self.expr(fx, value);
                self.elem_store(fx, s, v, "set.add");
                s
            }
            Expr::MapGet { map, key } => {
                let m = self.expr(fx, map);
                let _ = self.expr(fx, key);
                let out = self.load(m, Key::Any);
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            Expr::MapEntryKeyAt { map, idx }
            | Expr::MapEntryValueAt { map, idx }
            | Expr::SetValueAt { set: map, idx } => {
                let m = self.expr(fx, map);
                let _ = self.expr(fx, idx);
                let out = self.load(m, Key::Any);
                self.m.add_prim(out, prim::UNDEF);
                out
            }
            Expr::MapEntries(x) => {
                let m = self.expr(fx, x);
                let ek = self.m.elem_key();
                let el = self.load(m, ek);
                let pair = self.new_arr_from(mi, "entry", &[el]);
                self.new_arr_from(mi, "entries", &[pair])
            }
            Expr::MapKeys(x) | Expr::MapValues(x) | Expr::SetValues(x) => {
                let m = self.expr(fx, x);
                let ek = self.m.elem_key();
                let el = self.load(m, ek);
                self.new_arr_from(mi, "values", &[el])
            }
            Expr::ObjectFromEntries(x) => {
                let v = self.expr(fx, x);
                let it = self.iterate(v);
                let inner = self.load(it, Key::Any);
                let (t, n) = self.bobj(mi, "fromEntries");
                let fa = self.m.field_any(t);
                self.m.edge(inner, fa);
                n
            }
            Expr::ObjectValues(x) => {
                let v = self.expr(fx, x);
                let el = self.load(v, Key::Any);
                self.new_arr_from(mi, "values", &[el])
            }
            Expr::ObjectEntries(x) => {
                let v = self.expr(fx, x);
                let el = self.load(v, Key::Any);
                let s = self.prim_node(prim::STR);
                let pair = self.new_arr_from(mi, "entry", &[el, s]);
                self.new_arr_from(mi, "entries", &[pair])
            }
            Expr::ObjectGroupBy { items, key_fn } | Expr::MapGroupBy { items, key_fn } => {
                let v = self.expr(fx, items);
                let it = self.iterate(v);
                let f = self.expr(fx, key_fn);
                let num = self.prim_node(prim::NUM);
                let k = self.callback(f, vec![it, num]);
                let why = self.m.reason("groupBy key");
                self.m.cons(
                    k,
                    Cons::Implicit {
                        reason: why,
                        proto: Protocol::ToPrimitive,
                    },
                );
                let group = self.new_arr_from(mi, "group", &[it]);
                let (t, n) = self.bobj(mi, "groups");
                let fa = self.m.field_any(t);
                self.m.edge(group, fa);
                self.m.edge(k, fa);
                n
            }
            Expr::ArrayFrom(x, _) | Expr::IteratorToArray(x) | Expr::ForOfToArray(x) => {
                let v = self.expr(fx, x);
                let it = self.iterate(v);
                self.new_arr_from(mi, "from", &[it])
            }
            Expr::ForAwaitToArray(x) => {
                let v = self.expr(fx, x);
                let it = self.iterate(v);
                let aw = self.awaited(it);
                let arr = self.new_arr_from(mi, "fromAsync", &[aw]);
                self.promise_of(mi, arr)
            }
            Expr::ArrayFromArrayLikeHoley(x) => {
                let v = self.expr(fx, x);
                let el = self.load(v, Key::Any);
                self.m.add_prim(el, prim::UNDEF);
                self.new_arr_from(mi, "fromLike", &[el])
            }
            Expr::ArrayFromMapped {
                iterable,
                map_fn,
                this_arg,
            } => {
                let v = self.expr(fx, iterable);
                let it = self.iterate(v);
                let f = self.expr(fx, map_fn);
                let this = match this_arg {
                    Some(t) => ThisArg::Node(self.expr(fx, t)),
                    None => ThisArg::Undef,
                };
                let num = self.prim_node(prim::NUM);
                let ret = self.call_value(f, this, vec![it, num], vec![]);
                self.new_arr_from(mi, "fromMapped", &[ret])
            }
            Expr::IteratorFrom(x) | Expr::GetIterator(x) | Expr::GetAsyncIterator(x) => {
                let v = self.expr(fx, x);
                let it = self.iterate(v);
                let out = self.new_arr_from(mi, "iterator", &[it]);
                self.m.edge(v, out);
                out
            }
            Expr::ObjectCoerce(x) => {
                let v = self.expr(fx, x);
                let (_, b) = self.bobj(mi, "Object(prim)");
                let out = self.tmp("objcoerce");
                self.m.edge(v, out);
                self.m.edge(b, out);
                out
            }

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
                    _ => Key::Any,
                };
                // DECISIONS 217: a computed key is `o[k]`'s (`IdxLoad`: the
                // key's values decide what it names). Under `__proto__`
                // Object.prototype's getter runs on the receiver.
                let out = match key {
                    Key::Any => {
                        let dst = self.tmp("reflect.get");
                        self.m.cons(t, Cons::IdxLoad { idx: k, dst });
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
                    _ => Key::Any,
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
        }
    }
}
