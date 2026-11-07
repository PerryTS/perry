//! Constraint generation, part 4: the exhaustive remainder of `Expr`.
//!
//! This match has NO wildcard arm. The variants it does not handle itself are
//! or-pattern groups handed to `builtin_objects`, `collections` and
//! `reflection`, whose own matches are `unreachable!` for the other groups'
//! variants; the last group lists the variants `gen_expr::expr` already
//! models, which fail closed if ever routed here. Groups:
//! - primitive-only operations: operands get the protocol touch, the result
//!   is primitive (or a fresh container of primitives);
//! - builtin data structures (arrays, maps, sets, promises …): modeled flows;
//! - everything else fails closed (operands escape, result ⊤).

use crate::gen::{variant_name, Fx, Gen};
use crate::model::*;
use perry_hir::{Expr, Stmt};

#[path = "gen/accessor_descriptor.rs"]
mod accessor_descriptor;
mod builtin_objects;
mod collections;
mod nodes;
mod reflection;
mod web;

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
        // A folded builtin-method node is the call it folds: the compiled
        // code runs an own or inherited user method of that name, and the
        // builtin's own effects are its declaration (perry_hir::folded_call).
        if let Some(call) = perry_hir::folded_call::folded_call(e) {
            return self.folded_call(fx, &call);
        }
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
                if module == "__perry_borrowed_proof" {
                    self.prim_node(prim::BOOL)
                } else if module == "__perry_runtime" {
                    self.runtime_glue(fx, method, o, a, args)
                } else if m0 == "module" && method == "createRequire" && object.is_none() {
                    let t = self.m.tok(TokKind::Builtin(BuiltinNs::Require));
                    self.tok_node(t)
                } else if let (true, Some(o)) =
                    (module == "array" && method == "push_field_single", o)
                {
                    // `push_field_single` checks the receiver and runs its own
                    // `push` otherwise, which is the method call itself.
                    let push = self.m.atom("push");
                    self.mcall(o, Key::Named(push), a, vec![])
                } else if module == "array" && matches!(method.as_str(), "push_single" | "push") {
                    // The array push the HIR folds for a receiver it proved an
                    // array (`this.items.push(x)`): the arguments are its new
                    // elements, as `Expr::ArrayPush` stores them; the result
                    // is the new length. The grown array's write-back into its
                    // holder stores the same array.
                    match o {
                        Some(o) => {
                            for v in a {
                                self.elem_store(fx, o, v, "push");
                            }
                        }
                        None => {
                            for v in a {
                                self.to_sink(v, "argument of native module array.push");
                            }
                        }
                    }
                    self.prim_node(prim::NUM)
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
            Expr::ThisProvenClass(_) => self.prim_node(prim::BOOL),
            Expr::PrivateBrandCheck { object, .. } => {
                let _ = self.expr(fx, object);
                self.prim_node(prim::BOOL)
            }
            // The guard returns its receiver or throws (a receiver without
            // the brand, any primitive among them): its value is the
            // receiver's, nothing more.
            Expr::PrivateGuard { object, .. } => {
                let o = self.expr(fx, object);
                let out = self.tmp("privguard");
                self.m.edge(o, out);
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
                    let k = self.key_of(fx.module, key);
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
                    let k = self.key_of(fx.module, key);
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
                    let k = self.key_of(fx.module, key);
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
            Expr::ProcessArgv => {
                let n = self.m.node_l("process.argv");
                self.m.own(n);
                let a = self.m.process_argv;
                self.m.edge(a, n);
                n
            }
            Expr::ProcessEnv
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
            | Expr::ForInKeys(..)
            | Expr::TaggedTemplateStrings { .. }
            | Expr::TemplateRaw(..)
            | Expr::CryptoRandomBytes(..) => {
                // Own keys and enumeration run no lookup; the others read
                // elements or dispatch through `@@split` / `@@match`.
                let proto = match e {
                    Expr::ForInKeys(..) => Protocol::OwnKeys,
                    // path.parse(p) ToString's its argument (validated as a
                    // string); crypto.randomBytes(n) validates a number.
                    Expr::PathParse(..) | Expr::CryptoRandomBytes(..) => Protocol::ToPrimitive,
                    _ => Protocol::Any,
                };
                // The cooked strings of a tagged template are read, not coerced.
                if let Expr::TaggedTemplateStrings { cooked, .. } = e {
                    for k in cooked {
                        let _ = self.expr(fx, k);
                    }
                    return self.primbox_kids_none(fx, "primitive container builtin");
                }
                self.primbox_kids(fx, e, "primitive container builtin", proto)
            }
            // The calls they lower (`builtins` Object.keys, Reflect.ownKeys):
            // a fresh Array of the operand's own keys, no primitive.
            Expr::ObjectKeys(..)
            | Expr::ObjectGetOwnPropertyNames(..)
            | Expr::ObjectGetOwnPropertySymbols(..)
            | Expr::ReflectOwnKeys(..) => {
                let ks = kids(e);
                let _ = self.prim_op(
                    fx,
                    &ks,
                    prim::UNDEF,
                    "primitive container builtin",
                    Protocol::OwnKeys,
                );
                let (t, n) = self.bobj(mi, "arraybox");
                let fa = self.m.field_any(t);
                self.m.add_prim(fa, prim::STR | prim::SYMBOL);
                n
            }
            Expr::NamespacePatched(_) => self.prim_node(prim::BOOL),
            Expr::GlobalThisExpr => {
                let t = self.m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
                self.tok_node(t)
            }
            Expr::ProcessStdin | Expr::ProcessStdout | Expr::ProcessStderr => {
                let ns = match e {
                    Expr::ProcessStdin => BuiltinNs::Stdin,
                    Expr::ProcessStdout => BuiltinNs::Stdout,
                    _ => BuiltinNs::Stderr,
                };
                let t = self.m.tok(TokKind::Builtin(ns));
                self.tok_node(t)
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
            | Expr::StringFromCharCode(..)
            | Expr::StringFromCharCodeSpread(..)
            | Expr::StringFromCodePoint(..)
            | Expr::StringRaw { .. }
            | Expr::StringAt { .. }
            | Expr::StringCodePointAt { .. }
            | Expr::MapSize(..)
            | Expr::ArrayIterationPatched
            | Expr::ImportMetaUrl(..)
            | Expr::ProxyRevoke(..) => {
                // Mixed: element reads, `raw` / `toISOString` lookups, spreads.
                self.prim_kids(fx, e, prim::ALL, "primitive builtin operand", Protocol::Any)
            }
            Expr::MapClear(map) => {
                let recv = self.expr(fx, map);
                let name = self.m.atom("clear");
                self.mcall(recv, Key::Named(name), Vec::new(), Vec::new())
            }
            Expr::EncodeURI(..)
            | Expr::DecodeURI(..)
            | Expr::EncodeURIComponent(..)
            | Expr::DecodeURIComponent(..) => {
                // URI helpers only coerce their operand to a string; unrelated
                // members are neither read nor called (builtins/globals.rs).
                self.prim_kids(
                    fx,
                    e,
                    prim::ALL,
                    "URI operand ToString",
                    Protocol::ToPrimitive,
                )
            }
            Expr::DateValueOf(..) => self.receiver_method(fx, e, "valueOf"),
            Expr::DateToString(..) => self.receiver_method(fx, e, "toString"),
            Expr::DateToUTCString(..) => self.receiver_method(fx, e, "toUTCString"),
            // Static Date conversions do not read a receiver. parse
            // ToString's one operand; UTC ToNumber's at most 7 components.
            Expr::DateNow => self.prim_node(prim::NUM),
            Expr::DateParse(x) => self.prim_op(
                fx,
                &[x],
                prim::NUM,
                "ToString of Date.parse text",
                Protocol::ToPrimitive,
            ),
            Expr::DateUtc(xs) => self.prim_op(
                fx,
                &xs.iter().take(7).collect::<Vec<_>>(),
                prim::NUM,
                "ToNumber of Date.UTC component",
                Protocol::ToPrimitive,
            ),
            // Set clear retains/calls no operand; size reads an internal slot.
            Expr::SetClear(..) => self.receiver_method(fx, e, "clear"),
            Expr::SetSize(x) => {
                let recv = self.expr(fx, x);
                let a = self.m.atom("size");
                self.load(recv, Key::Named(a))
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
            Expr::MathMinSpread(s) | Expr::MathMaxSpread(s) => {
                let s = self.expr(fx, s);
                let it = self.iterate(s);
                let r = self.m.reason("ToNumber of a spread Math operand");
                self.m.cons(
                    it,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::ToPrimitive,
                    },
                );
                self.prim_node(prim::NUM)
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

            Expr::WeakRefNew { .. }
            | Expr::WeakRefDeref { .. }
            | Expr::FinalizationRegistryNew { .. }
            | Expr::FinalizationRegistryRegister { .. }
            | Expr::FinalizationRegistryUnregister { .. }
            | Expr::ObjectDefineProperty { .. }
            | Expr::ReflectDefineProperty { .. }
            | Expr::ObjectDefineProperties { .. }
            | Expr::ObjectGetOwnPropertyDescriptor { .. }
            | Expr::ReflectGetOwnPropertyDescriptor { .. }
            | Expr::ObjectGetOwnPropertyDescriptors { .. }
            | Expr::ObjectCreate { .. }
            | Expr::ObjectFreeze { .. }
            | Expr::ObjectSeal { .. }
            | Expr::ObjectPreventExtensions { .. }
            | Expr::ReflectPreventExtensions { .. }
            | Expr::ObjectGetPrototypeOf { .. }
            | Expr::ReflectGetPrototypeOf { .. }
            | Expr::ObjectSetPrototypeOf { .. }
            | Expr::ReflectSetPrototypeOf { .. }
            | Expr::JsonParse { .. }
            | Expr::JsonParseTyped { .. }
            | Expr::JsonRawJson { .. }
            | Expr::JsonParseReviver { .. }
            | Expr::JsonParseWithReviver { .. }
            | Expr::JsonStringify { .. }
            | Expr::JsonStringifyFull { .. }
            | Expr::JsonStringifyPretty { .. }
            | Expr::WebAssemblyCompile { .. }
            | Expr::WebAssemblyModuleNew { .. }
            | Expr::WebAssemblyModuleExports { .. }
            | Expr::WebAssemblyModuleImports { .. }
            | Expr::WebAssemblyModuleCustomSections { .. }
            | Expr::WebAssemblyInstantiate { .. }
            | Expr::WebAssemblyCallExport { .. }
            | Expr::TextEncoderNew { .. }
            | Expr::TextDecoderNew { .. }
            | Expr::TextEncoderEncode { .. }
            | Expr::TextEncoderEncodeInto { .. }
            | Expr::TextDecoderDecode { .. }
            | Expr::TextDecoderEncoding { .. }
            | Expr::TextDecoderFatal { .. }
            | Expr::TextDecoderIgnoreBom { .. }
            | Expr::UrlInstanceToString { .. }
            | Expr::UrlInstanceToJSON { .. }
            | Expr::UrlSearchParamsGet { .. }
            | Expr::UrlSearchParamsGetAll { .. }
            | Expr::UrlSearchParamsHas { .. }
            | Expr::UrlSearchParamsSet { .. }
            | Expr::UrlSearchParamsAppend { .. }
            | Expr::UrlSearchParamsDelete { .. }
            | Expr::UrlSearchParamsToString { .. }
            | Expr::UrlSearchParamsSort { .. }
            | Expr::UrlSearchParamsEntries { .. }
            | Expr::UrlSearchParamsKeys { .. }
            | Expr::UrlSearchParamsValues { .. }
            | Expr::UrlSearchParamsForEach { .. }
            | Expr::UrlSearchParamsMissingArgs { .. }
            | Expr::UrlGetHref { .. }
            | Expr::UrlGetPathname { .. }
            | Expr::UrlGetProtocol { .. }
            | Expr::UrlGetHost { .. }
            | Expr::UrlGetHostname { .. }
            | Expr::UrlGetPort { .. }
            | Expr::UrlGetSearch { .. }
            | Expr::UrlGetHash { .. }
            | Expr::UrlGetOrigin { .. }
            | Expr::UrlGetSearchParams { .. }
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
            | Expr::UrlSearchParamsNew { .. }
            | Expr::UrlPatternNew { .. }
            | Expr::UrlParse { .. }
            | Expr::UrlParseWithBase { .. }
            | Expr::UrlCanParse { .. }
            | Expr::UrlCanParseWithBase { .. }
            | Expr::BufferFrom { .. }
            | Expr::BufferAlloc { .. }
            | Expr::BufferAllocUnsafe { .. }
            | Expr::FsReadFileBinary { .. }
            | Expr::Uint8ArrayNew { .. }
            | Expr::Uint8ArrayFrom { .. }
            | Expr::TypedArrayNew { .. }
            | Expr::BufferFromArrayBuffer { .. }
            | Expr::BufferSlice { .. }
            | Expr::BufferConcat { .. }
            | Expr::BufferConcatWithLength { .. }
            | Expr::StructuredClone { .. }
            | Expr::LinkGeneratorPrototype { .. }
            | Expr::IterResultSet { .. }
            | Expr::IterResultGetValue { .. }
            | Expr::AsyncStepChain { .. }
            | Expr::AsyncStepResumeNow { .. }
            | Expr::AsyncStepDeclined { .. }
            | Expr::AsyncStepDone { .. }
            | Expr::CurrentStepClosure { .. }
            | Expr::AsyncFirstCall { .. }
            | Expr::AsyncGenResume { .. }
            | Expr::WebCryptoDigest { .. }
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
            | Expr::NativeArenaAlloc { .. }
            | Expr::NativeArenaView { .. }
            | Expr::NativePodView { .. }
            | Expr::NativeArenaDispose { .. }
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
            | Expr::StaticPluginResolve { .. }
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
            | Expr::WorkerNew { .. }
            | Expr::BoxedPrimitiveNew { .. }
            | Expr::DateNew { .. }
            | Expr::RegExp { .. }
            | Expr::RegExpDynamic { .. }
            | Expr::ErrorNew { .. }
            | Expr::TypeErrorNew { .. }
            | Expr::RangeErrorNew { .. }
            | Expr::ReferenceErrorNew { .. }
            | Expr::SyntaxErrorNew { .. }
            | Expr::ErrorNewWithCause { .. }
            | Expr::ErrorNewWithOptions { .. }
            | Expr::AggregateErrorNew { .. }
            | Expr::ErrorMessage { .. }
            | Expr::StringReplace { .. }
            | Expr::StringSplit { .. }
            | Expr::StringMatch { .. }
            | Expr::StringMatchAll { .. } => self.expr_rest_builtin_objects(fx, e),
            // Handled before the match: a folded call is its method call.
            Expr::ArrayAt { .. }
            | Expr::ArrayCopyWithin { .. }
            | Expr::ArrayEntries { .. }
            | Expr::ArrayEvery { .. }
            | Expr::ArrayFilter { .. }
            | Expr::ArrayFind { .. }
            | Expr::ArrayFindIndex { .. }
            | Expr::ArrayFindLast { .. }
            | Expr::ArrayFindLastIndex { .. }
            | Expr::ArrayFlat { .. }
            | Expr::ArrayFlatMap { .. }
            | Expr::ArrayForEach { .. }
            | Expr::ArrayIncludes { .. }
            | Expr::ArrayIndexOf { .. }
            | Expr::ArrayJoin { .. }
            | Expr::ArrayKeys { .. }
            | Expr::ArrayLastIndexOf { .. }
            | Expr::ArrayMap { .. }
            | Expr::ArrayPush { .. }
            | Expr::ArrayReduce { .. }
            | Expr::ArrayReduceRight { .. }
            | Expr::ArrayShift { .. }
            | Expr::ArraySlice { .. }
            | Expr::ArraySome { .. }
            | Expr::ArraySort { .. }
            | Expr::ArraySplice { .. }
            | Expr::ArrayToReversed { .. }
            | Expr::ArrayToSorted { .. }
            | Expr::ArrayToSpliced { .. }
            | Expr::ArrayUnshift { .. }
            | Expr::ArrayValues { .. }
            | Expr::ArrayWith { .. }
            | Expr::DateGetDate { .. }
            | Expr::DateGetDay { .. }
            | Expr::DateGetFullYear { .. }
            | Expr::DateGetHours { .. }
            | Expr::DateGetMilliseconds { .. }
            | Expr::DateGetMinutes { .. }
            | Expr::DateGetMonth { .. }
            | Expr::DateGetSeconds { .. }
            | Expr::DateGetTime { .. }
            | Expr::DateGetTimezoneOffset { .. }
            | Expr::DateGetUtcDate { .. }
            | Expr::DateGetUtcDay { .. }
            | Expr::DateGetUtcFullYear { .. }
            | Expr::DateGetUtcHours { .. }
            | Expr::DateGetUtcMilliseconds { .. }
            | Expr::DateGetUtcMinutes { .. }
            | Expr::DateGetUtcMonth { .. }
            | Expr::DateGetUtcSeconds { .. }
            | Expr::DateSetDate { .. }
            | Expr::DateSetFullYear { .. }
            | Expr::DateSetHours { .. }
            | Expr::DateSetMilliseconds { .. }
            | Expr::DateSetMinutes { .. }
            | Expr::DateSetMonth { .. }
            | Expr::DateSetSeconds { .. }
            | Expr::DateSetTime { .. }
            | Expr::DateSetUtcDate { .. }
            | Expr::DateSetUtcFullYear { .. }
            | Expr::DateSetUtcHours { .. }
            | Expr::DateSetUtcMilliseconds { .. }
            | Expr::DateSetUtcMinutes { .. }
            | Expr::DateSetUtcMonth { .. }
            | Expr::DateSetUtcSeconds { .. }
            | Expr::DateToDateString { .. }
            | Expr::DateToISOString { .. }
            | Expr::DateToJSON { .. }
            | Expr::DateToLocaleDateString { .. }
            | Expr::DateToLocaleString { .. }
            | Expr::DateToLocaleTimeString { .. }
            | Expr::DateToTimeString { .. }
            | Expr::MapDelete { .. }
            | Expr::MapGet { .. }
            | Expr::MapHas { .. }
            | Expr::MapSet { .. }
            | Expr::SetAdd { .. }
            | Expr::SetDelete { .. }
            | Expr::SetHas { .. } => {
                unreachable!(
                    "internal compiler error: expr_rest: a folded call (perry_hir::folded_call)"
                )
            }
            Expr::ArrayPushSpread { .. }
            | Expr::ArrayPop { .. }
            | Expr::ArrayReverseValue { .. }
            | Expr::ArrayCopyWithinValue { .. }
            | Expr::ArrayLikeMethod { .. }
            | Expr::MapNew { .. }
            | Expr::SetNew { .. }
            | Expr::MapNewFromArray { .. }
            | Expr::SetNewFromArray { .. }
            | Expr::MapEntryKeyAt { .. }
            | Expr::MapEntryValueAt { .. }
            | Expr::SetValueAt { .. }
            | Expr::MapEntries { .. }
            | Expr::MapKeys { .. }
            | Expr::MapValues { .. }
            | Expr::SetValues { .. }
            | Expr::ObjectFromEntries { .. }
            | Expr::ObjectValues { .. }
            | Expr::ObjectEntries { .. }
            | Expr::ObjectGroupBy { .. }
            | Expr::MapGroupBy { .. }
            | Expr::ArrayFrom { .. }
            | Expr::IteratorToArray { .. }
            | Expr::ForOfToArray { .. }
            | Expr::ForAwaitToArray { .. }
            | Expr::ArrayFromArrayLikeHoley { .. }
            | Expr::ArrayFromMapped { .. }
            | Expr::IteratorFrom { .. }
            | Expr::GetIterator { .. }
            | Expr::GetAsyncIterator { .. }
            | Expr::ObjectCoerce { .. } => self.expr_rest_collections(fx, e),
            Expr::ProxyNew { .. }
            | Expr::ProxyRevocable { .. }
            | Expr::ReflectGet { .. }
            | Expr::ReflectSet { .. }
            | Expr::ReflectDelete { .. }
            | Expr::ReflectApply { .. }
            | Expr::ReflectConstruct { .. }
            | Expr::DynamicImport { .. }
            | Expr::Undefined { .. }
            | Expr::Null { .. }
            | Expr::Bool { .. }
            | Expr::Number { .. }
            | Expr::Integer { .. }
            | Expr::BigInt { .. }
            | Expr::String { .. }
            | Expr::WtfString { .. }
            | Expr::I18nString { .. }
            | Expr::LocalGet { .. }
            | Expr::LocalSet { .. }
            | Expr::ScopedTemp { .. }
            | Expr::GlobalGet { .. }
            | Expr::GlobalSet { .. }
            | Expr::WithGet { .. }
            | Expr::WithSet { .. }
            | Expr::Update { .. }
            | Expr::Binary { .. }
            | Expr::Unary { .. }
            | Expr::Compare { .. }
            | Expr::Logical { .. }
            | Expr::Call { .. }
            | Expr::CallSpread { .. }
            | Expr::SuperCallSpread { .. }
            | Expr::FuncRef { .. }
            | Expr::ExternFuncRef { .. }
            | Expr::PropertyGet { .. }
            | Expr::PropertySet { .. }
            | Expr::PropertyUpdate { .. }
            | Expr::BufferLength { .. }
            | Expr::Uint8ArrayLength { .. }
            | Expr::BufferIndexGet { .. }
            | Expr::Uint8ArrayGet { .. }
            | Expr::IndexGet { .. }
            | Expr::BufferIndexSet { .. }
            | Expr::Uint8ArraySet { .. }
            | Expr::IndexSet { .. }
            | Expr::IndexUpdate { .. }
            | Expr::Object { .. }
            | Expr::ObjectSpread { .. }
            | Expr::ObjectAssign { .. }
            | Expr::LiteralChunks { .. }
            | Expr::LiteralChunkObject { .. }
            | Expr::LiteralSlotInit { .. }
            | Expr::Array { .. }
            | Expr::ArraySpread { .. }
            | Expr::Conditional { .. }
            | Expr::TypeOf { .. }
            | Expr::Void { .. }
            | Expr::InstanceOf { .. }
            | Expr::In { .. }
            | Expr::Await { .. }
            | Expr::Yield { .. }
            | Expr::New { .. }
            | Expr::NewDynamic { .. }
            | Expr::NewDynamicSpread { .. }
            | Expr::NewTarget { .. }
            | Expr::ClassRef { .. }
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
            | Expr::This { .. }
            | Expr::SuperCall { .. }
            | Expr::SuperMethodCall { .. }
            | Expr::SuperMethodCallSpread { .. }
            | Expr::SuperPropertyGet { .. }
            | Expr::SuperPropertySet { .. }
            | Expr::ModuleTopThis { .. }
            | Expr::Sequence { .. }
            | Expr::Delete { .. }
            | Expr::Closure { .. }
            | Expr::ObjectRest { .. }
            | Expr::PutValueSet { .. } => self.expr_rest_reflection(fx, e),
        }
    }
}
