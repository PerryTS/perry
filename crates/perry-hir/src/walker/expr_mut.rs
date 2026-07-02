use super::*;

/// Visit every direct sub-expression of `expr` in evaluation order.
///
/// See module docs for what counts as a "direct sub-expression."
pub fn walk_expr_children_mut<F>(expr: &mut Expr, f: &mut F)
where
    F: FnMut(&mut Expr),
{
    match expr {
        // ─── Pure leaves: no Expr children ────────────────────────────────
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::BigInt(_)
        | Expr::String(_)
        | Expr::WtfString(_)
        | Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::ExternFuncRef { .. }
        | Expr::NativeModuleRef(_)
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. }
        | Expr::NewTarget
        | Expr::ClassRef(_)
        | Expr::This
        | Expr::NewTarget
        | Expr::SuperPropertyGet { .. }
        | Expr::EnumMember { .. }
        | Expr::StaticFieldGet { .. }
        | Expr::Update { .. }
        | Expr::EnvGet(_)
        | Expr::ProcessEnv
        | Expr::GlobalThisExpr
        | Expr::ModuleTopThis
        | Expr::ProcessUptime
        | Expr::ProcessCwd
        | Expr::ProcessArgv
        | Expr::ProcessMemoryUsage
        | Expr::ProcessPid
        | Expr::ProcessPpid
        | Expr::ProcessVersion
        | Expr::ProcessVersions
        | Expr::ProcessHrtimeBigint
        | Expr::ProcessStdin
        | Expr::ProcessStdout
        | Expr::ProcessStderr
        | Expr::ProcessAbort
        | Expr::ProcessAvailableMemory
        | Expr::ProcessConstrainedMemory
        | Expr::ProcessPosixCredential(_)
        | Expr::ProcessResourceUsage
        | Expr::ProcessActiveResourcesInfo
        | Expr::ProcessTitle
        | Expr::ProcessStdinIsTTY
        | Expr::ProcessStdoutIsTTY
        | Expr::ProcessStderrIsTTY
        | Expr::ProcessStdoutColumns
        | Expr::ProcessStdoutRows
        | Expr::PathSep
        | Expr::PathDelimiter
        | Expr::PerformanceNow
        | Expr::IterResultGetValue
        | Expr::IterResultGetDone
        | Expr::TextEncoderNew
        | Expr::CryptoRandomUUID
        | Expr::CryptoRandomUUIDv7
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
        | Expr::OsCpus
        | Expr::OsNetworkInterfaces
        | Expr::OsUserInfo
        | Expr::OsUserInfoBuffer
        | Expr::OsEOL
        | Expr::OsDevNull
        | Expr::OsAvailableParallelism
        | Expr::OsEndianness
        | Expr::OsLoadavg
        | Expr::OsMachine
        | Expr::OsVersion
        | Expr::DateNow
        | Expr::MathRandom
        | Expr::MapNew
        | Expr::SetNew
        | Expr::RegExp { .. }
        | Expr::RegExpExecIndex
        | Expr::RegExpExecGroups
        | Expr::JsLoadModule { .. }
        | Expr::ImportMetaUrl(_)
        | Expr::ArrayPop(_)
        | Expr::ArrayShift(_) => {}

        // ─── Single-child wrappers (one Box<Expr> field) ──────────────────
        Expr::LocalSet(_, v)
        | Expr::GlobalSet(_, v)
        | Expr::TypeOf(v)
        | Expr::Void(v)
        | Expr::Await(v)
        | Expr::Delete(v)
        | Expr::Unary { operand: v, .. }
        | Expr::PropertyGet { object: v, .. }
        | Expr::PropertyUpdate { object: v, .. }
        | Expr::StaticFieldSet { value: v, .. }
        | Expr::EnvGetDynamic(v)
        | Expr::ProcessChdir(v)
        | Expr::ProcessStdinSetRawMode(v)
        | Expr::TtyIsAtty(v)
        | Expr::FsReadFileSync(v)
        | Expr::FsExistsSync(v)
        | Expr::FsMkdirSync(v)
        | Expr::FsUnlinkSync(v)
        | Expr::FsReadFileBinary(v)
        | Expr::FsRmRecursive(v)
        | Expr::PathDirname(v)
        | Expr::PathBasename(v)
        | Expr::PathExtname(v)
        | Expr::PathResolve(v)
        | Expr::PathIsAbsolute(v)
        | Expr::PathNormalize(v)
        | Expr::PathParse(v)
        | Expr::PathFormat(v)
        | Expr::PathToNamespacedPath(v)
        | Expr::FileURLToPath(v)
        | Expr::WeakRefNew(v)
        | Expr::WeakRefDeref(v)
        | Expr::FinalizationRegistryNew(v)
        | Expr::ObjectGetOwnPropertyNames(v)
        | Expr::ObjectGetOwnPropertyDescriptors(v)
        | Expr::ObjectFreeze(v)
        | Expr::ObjectSeal(v)
        | Expr::ObjectPreventExtensions(v)
        | Expr::ObjectIsFrozen(v)
        | Expr::ObjectIsSealed(v)
        | Expr::ObjectIsExtensible(v)
        | Expr::ObjectGetPrototypeOf(v)
        | Expr::ObjectGetOwnPropertySymbols(v)
        | Expr::ObjectKeys(v)
        | Expr::ForInKeys(v)
        | Expr::ObjectValues(v)
        | Expr::ObjectEntries(v)
        | Expr::ObjectFromEntries(v)
        | Expr::SymbolFor(v)
        | Expr::SymbolKeyFor(v)
        | Expr::SymbolDescription(v)
        | Expr::RegExpEscape(v)
        | Expr::SymbolToString(v)
        | Expr::RegExpSource(v)
        | Expr::RegExpFlags(v)
        | Expr::RegExpLastIndex(v)
        | Expr::JsonParse(v)
        | Expr::JsonStringify(v)
        | Expr::JsonRawJson(v)
        | Expr::JsonIsRawJson(v)
        | Expr::JsonParseTyped { text: v, .. }
        | Expr::MathFloor(v)
        | Expr::MathCeil(v)
        | Expr::MathRound(v)
        | Expr::MathTrunc(v)
        | Expr::MathSign(v)
        | Expr::MathAbs(v)
        | Expr::MathSqrt(v)
        | Expr::MathLog(v)
        | Expr::MathLog2(v)
        | Expr::MathLog10(v)
        | Expr::MathLog1p(v)
        | Expr::MathClz32(v)
        | Expr::MathSin(v)
        | Expr::MathCos(v)
        | Expr::MathTan(v)
        | Expr::MathAsin(v)
        | Expr::MathAcos(v)
        | Expr::MathAtan(v)
        | Expr::MathCbrt(v)
        | Expr::MathFround(v)
        | Expr::MathF16round(v)
        | Expr::MathExpm1(v)
        | Expr::MathSinh(v)
        | Expr::MathCosh(v)
        | Expr::MathTanh(v)
        | Expr::MathAsinh(v)
        | Expr::MathAcosh(v)
        | Expr::MathAtanh(v)
        | Expr::MathExp(v)
        | Expr::MathMinSpread(v)
        | Expr::MathMaxSpread(v)
        | Expr::WebAssemblyValidate(v)
        | Expr::WebAssemblyCompile(v)
        | Expr::WebAssemblyModuleNew(v)
        | Expr::WebAssemblyModuleExports(v)
        | Expr::WebAssemblyModuleImports(v)
        | Expr::WebAssemblyInstantiate(v)
        | Expr::Atob(v)
        | Expr::Btoa(v)
        | Expr::TextEncoderEncode(v)
        | Expr::TextDecoderEncoding(v)
        | Expr::TextDecoderFatal(v)
        | Expr::TextDecoderIgnoreBom(v)
        | Expr::EncodeURI(v)
        | Expr::DecodeURI(v)
        | Expr::EncodeURIComponent(v)
        | Expr::DecodeURIComponent(v)
        | Expr::QueueMicrotask(v)
        | Expr::IterResultSet(v, _)
        | Expr::CryptoRandomBytes(v)
        | Expr::CryptoSha256(v)
        | Expr::CryptoMd5(v)
        | Expr::BufferAllocUnsafe(v)
        | Expr::BufferConcat(v)
        | Expr::BufferIsBuffer(v)
        | Expr::BufferIsEncoding(v)
        | Expr::BufferLength(v)
        | Expr::NativeArenaAlloc(v)
        | Expr::NativeArenaDispose(v)
        | Expr::Uint8ArrayFrom(v)
        | Expr::Uint8ArrayLength(v)
        | Expr::ChildProcessGetProcessStatus(v)
        | Expr::ChildProcessKillProcess(v)
        | Expr::ParseFloat(v)
        | Expr::NumberCoerce(v)
        | Expr::BigIntCoerce(v)
        | Expr::StringCoerce(v)
        | Expr::ObjectCoerce(v)
        | Expr::BooleanCoerce(v)
        | Expr::IsNaN(v)
        | Expr::IsUndefinedOrBareNan(v)
        | Expr::IsFinite(v)
        | Expr::NumberIsNaN(v)
        | Expr::NumberIsFinite(v)
        | Expr::NumberIsInteger(v)
        | Expr::NumberIsSafeInteger(v)
        | Expr::StaticPluginResolve(v)
        | Expr::ArrayIsArray(v)
        | Expr::ArrayFrom(v)
        | Expr::ArrayFromArrayLikeHoley(v)
        | Expr::IteratorFrom(v)
        | Expr::IteratorToArray(v)
        | Expr::GetIterator(v)
        | Expr::GetAsyncIterator(v)
        | Expr::ForOfToArray(v)
        | Expr::ForAwaitToArray(v)
        | Expr::ObjectRest { object: v, .. }
        | Expr::ProxyRevoke(v)
        | Expr::ReflectOwnKeys(v)
        | Expr::ReflectGetPrototypeOf(v)
        | Expr::ReflectIsExtensible(v)
        | Expr::ReflectPreventExtensions(v)
        | Expr::DateGetTime(v)
        | Expr::DateToISOString(v)
        | Expr::DateGetFullYear(v)
        | Expr::DateGetMonth(v)
        | Expr::DateGetDate(v)
        | Expr::DateGetDay(v)
        | Expr::DateGetHours(v)
        | Expr::DateGetMinutes(v)
        | Expr::DateGetSeconds(v)
        | Expr::DateGetMilliseconds(v)
        | Expr::DateParse(v)
        | Expr::DateGetUtcDay(v)
        | Expr::DateGetUtcFullYear(v)
        | Expr::DateGetUtcMonth(v)
        | Expr::DateGetUtcDate(v)
        | Expr::DateGetUtcHours(v)
        | Expr::DateGetUtcMinutes(v)
        | Expr::DateGetUtcSeconds(v)
        | Expr::DateGetUtcMilliseconds(v)
        | Expr::DateValueOf(v)
        | Expr::DateToString(v)
        | Expr::DateToDateString(v)
        | Expr::DateToTimeString(v)
        | Expr::DateToUTCString(v)
        | Expr::DateToLocaleDateString(v)
        | Expr::DateToLocaleTimeString(v)
        | Expr::DateToLocaleString(v)
        | Expr::DateGetTimezoneOffset(v)
        | Expr::DateToJSON(v)
        | Expr::ErrorMessage(v)
        | Expr::TypeErrorNew(v)
        | Expr::RangeErrorNew(v)
        | Expr::ReferenceErrorNew(v)
        | Expr::SyntaxErrorNew(v)
        | Expr::UrlGetHref(v)
        | Expr::UrlGetPathname(v)
        | Expr::UrlGetProtocol(v)
        | Expr::UrlGetHost(v)
        | Expr::UrlGetHostname(v)
        | Expr::UrlGetPort(v)
        | Expr::UrlGetSearch(v)
        | Expr::UrlGetHash(v)
        | Expr::UrlGetOrigin(v)
        | Expr::UrlGetSearchParams(v)
        | Expr::UrlCanParse(v)
        | Expr::UrlParse(v)
        | Expr::UrlInstanceToString(v)
        | Expr::UrlInstanceToJSON(v)
        | Expr::UrlSearchParamsToString(v)
        | Expr::UrlSearchParamsEntries(v)
        | Expr::UrlSearchParamsKeys(v)
        | Expr::UrlSearchParamsValues(v)
        | Expr::UrlSearchParamsSort(v)
        | Expr::JsCreateCallback { closure: v, .. }
        | Expr::JsGetExport {
            module_handle: v, ..
        }
        | Expr::JsGetProperty { object: v, .. }
        | Expr::ArrayEntries(v)
        | Expr::ArrayKeys(v)
        | Expr::ArrayValues(v)
        | Expr::SetSize(v)
        | Expr::SetClear(v)
        | Expr::SetValues(v)
        | Expr::MapSize(v)
        | Expr::MapClear(v)
        | Expr::MapEntries(v)
        | Expr::MapKeys(v)
        | Expr::MapValues(v)
        | Expr::SetNewFromArray(v)
        | Expr::MapNewFromArray(v)
        | Expr::ArrayFlat { array: v }
        | Expr::ArrayToReversed { array: v }
        | Expr::TemplateRaw(v) => {
            f(v);
        }
        Expr::ObjectCreate(proto, props) => {
            f(proto);
            if let Some(props) = props {
                f(props);
            }
        }
        Expr::UrlSearchParamsMissingArgs { params, args, .. } => {
            f(params);
            for arg in args {
                f(arg);
            }
        }
        Expr::BufferConcatWithLength { list, total_length } => {
            f(list);
            f(total_length);
        }

        Expr::UrlCanParseWithBase { input, base } => {
            f(input);
            f(base);
        }

        Expr::UrlParseWithBase { input, base } => {
            f(input);
            f(base);
        }

        Expr::NativeArenaView {
            owner,
            byte_offset,
            length,
            ..
        } => {
            f(owner);
            f(byte_offset);
            f(length);
        }

        Expr::NativePodView {
            owner,
            byte_offset,
            count,
            ..
        } => {
            f(owner);
            f(byte_offset);
            f(count);
        }

        Expr::NativeMemoryFillU32 { view, value } => {
            f(view);
            f(value);
        }

        Expr::NativeMemoryCopy { dst, src } => {
            f(dst);
            f(src);
        }

        Expr::UrlSearchParamsForEach {
            params,
            callback,
            this_arg,
        } => {
            f(params);
            f(callback);
            if let Some(this_arg) = this_arg {
                f(this_arg);
            }
        }

        Expr::TaggedTemplateStrings { cooked, .. } => {
            for e in cooked {
                f(e);
            }
        }

        // v0.5.749: InstanceOf has TWO Expr children (expr + optional
        // ty_expr) so callers like the inliner / closure-conversion /
        // monomorph substitution see both. Without this, ty_expr was
        // invisible to the walker and dynamic-instanceof through inlined
        // function args returned false.
        Expr::InstanceOf { expr, ty_expr, .. } => {
            f(expr);
            if let Some(t) = ty_expr {
                f(t);
            }
        }

        // ─── Web Crypto (issue #561) ──────────────────────────────────────
        Expr::WebCryptoDigest { algo, data } => {
            f(algo);
            f(data);
        }
        Expr::WebCryptoImportKey {
            format,
            key,
            algorithm,
            extractable,
            usages,
        } => {
            f(format);
            f(key);
            f(algorithm);
            f(extractable);
            f(usages);
        }
        Expr::WebCryptoExportKey { format, key } => {
            f(format);
            f(key);
        }
        Expr::WebCryptoSign {
            algorithm,
            key,
            data,
        } => {
            f(algorithm);
            f(key);
            f(data);
        }
        Expr::WebCryptoVerify {
            algorithm,
            key,
            signature,
            data,
        } => {
            f(algorithm);
            f(key);
            f(signature);
            f(data);
        }
        Expr::WebCryptoDeriveBits {
            algorithm,
            base_key,
            length,
        } => {
            f(algorithm);
            f(base_key);
            f(length);
        }
        Expr::WebCryptoDeriveKey {
            algorithm,
            base_key,
            derived_key_algorithm,
            extractable,
            usages,
        } => {
            f(algorithm);
            f(base_key);
            f(derived_key_algorithm);
            f(extractable);
            f(usages);
        }
        Expr::WebCryptoEncrypt {
            algorithm,
            key,
            data,
        }
        | Expr::WebCryptoDecrypt {
            algorithm,
            key,
            data,
        } => {
            f(algorithm);
            f(key);
            f(data);
        }
        Expr::WebCryptoGenerateKey {
            algorithm,
            extractable,
            usages,
        } => {
            f(algorithm);
            f(extractable);
            f(usages);
        }
        Expr::WebCryptoWrapKey {
            format,
            key,
            wrapping_key,
            wrap_algorithm,
        } => {
            f(format);
            f(key);
            f(wrapping_key);
            f(wrap_algorithm);
        }
        Expr::WebCryptoUnwrapKey {
            format,
            wrapped_key,
            unwrapping_key,
            unwrap_algorithm,
            unwrapped_key_algorithm,
            extractable,
            usages,
        } => {
            f(format);
            f(wrapped_key);
            f(unwrapping_key);
            f(unwrap_algorithm);
            f(unwrapped_key_algorithm);
            f(extractable);
            f(usages);
        }
        Expr::CryptoRandomFillSync {
            buffer,
            offset,
            size,
        } => {
            f(buffer);
            f(offset);
            f(size);
        }

        // ─── Two-child variants ───────────────────────────────────────────
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            f(left);
            f(right);
        }
        Expr::TextEncoderEncodeInto { source, dest } => {
            f(source);
            f(dest);
        }
        Expr::TextDecoderNew {
            label,
            fatal,
            ignore_bom,
        } => {
            f(label);
            f(fatal);
            f(ignore_bom);
        }
        Expr::TextDecoderDecode { decoder, input } => {
            f(decoder);
            f(input);
        }
        Expr::PropertySet { object, value, .. } => {
            f(object);
            f(value);
        }
        Expr::ClassStaticSymbolSet { key, value, .. } => {
            f(key);
            f(value);
        }
        Expr::RegisterClassParentDynamic { parent_expr, .. } => {
            f(parent_expr);
        }
        Expr::RegisterClassCaptures { captures, .. } => {
            for c in captures {
                f(c);
            }
        }
        Expr::ClassCaptureValue { fallback, .. } => {
            if let Some(fb) = fallback {
                f(fb);
            }
        }
        Expr::RegisterClassStaticSymbol {
            key_expr,
            value_expr,
            ..
        } => {
            f(key_expr);
            f(value_expr);
        }
        Expr::RegisterClassComputedMethod { key_expr, .. }
        | Expr::RegisterClassComputedAccessor { key_expr, .. } => {
            f(key_expr);
        }
        Expr::ClassExprFresh {
            named_statics,
            symbol_statics,
            captured_args,
            ..
        } => {
            for (_, v) in named_statics.iter_mut() {
                f(v);
            }
            for (k, v) in symbol_statics.iter_mut() {
                f(k);
                f(v);
            }
            for a in captured_args.iter_mut() {
                f(a);
            }
        }
        Expr::SetFunctionPrototype { func, proto } => {
            f(func);
            f(proto);
        }
        Expr::RegisterPrototypeMethod { value, .. } => {
            f(value);
        }
        Expr::RegisterFunctionPrototypeMethod { func, value, .. } => {
            f(func);
            f(value);
        }
        Expr::GetFunctionPrototypeMethod { func, .. } => {
            f(func);
        }
        Expr::IndexGet { object, index } => {
            f(object);
            f(index);
        }
        Expr::MapEntryKeyAt { map, idx } | Expr::MapEntryValueAt { map, idx } => {
            f(map);
            f(idx);
        }
        Expr::SetValueAt { set, idx } => {
            f(set);
            f(idx);
        }
        Expr::IndexUpdate { object, index, .. } => {
            f(object);
            f(index);
        }
        Expr::In { property, object } => {
            f(property);
            f(object);
        }
        Expr::PrivateBrandCheck { object, .. } => {
            f(object);
        }
        Expr::PrivateGuard { object, .. } => {
            f(object);
        }
        Expr::FsWriteFileSync(a, b)
        | Expr::FsAppendFileSync(a, b)
        | Expr::PathJoin(a, b)
        | Expr::PathRelative(a, b)
        | Expr::PathBasenameExt(a, b)
        | Expr::PathMatchesGlob(a, b)
        | Expr::PathResolveJoin(a, b)
        | Expr::PathWin32Join(a, b)
        | Expr::ObjectGetOwnPropertyDescriptor(a, b)
        | Expr::ObjectIs(a, b)
        | Expr::ObjectHasOwn(a, b)
        | Expr::ObjectSetPrototypeOf(a, b)
        | Expr::ObjectDefineProperties(a, b)
        | Expr::JsonParseWithReviver(a, b)
        | Expr::MathPow(a, b)
        | Expr::MathImul(a, b)
        | Expr::MathAtan2(a, b)
        | Expr::StringSplit(a, b) => {
            f(a);
            f(b);
        }
        Expr::SymbolNew(opt) => {
            if let Some(v) = opt {
                f(v);
            }
        }
        Expr::StringFromCharCode(v)
        | Expr::StringFromCharCodeSpread(v)
        | Expr::StringFromCodePoint(v) => {
            f(v);
        }
        Expr::StringRaw {
            call_site,
            substitutions,
        } => {
            f(call_site);
            for s in substitutions {
                f(s);
            }
        }
        Expr::StringAt { string, index } | Expr::StringCodePointAt { string, index } => {
            f(string);
            f(index);
        }
        Expr::ParseInt { string, radix } => {
            f(string);
            if let Some(r) = radix {
                f(r);
            }
        }
        Expr::JsonParseReviver { text, reviver } => {
            f(text);
            f(reviver);
        }
        Expr::JsonStringifyPretty {
            value,
            replacer,
            space,
        } => {
            f(value);
            if let Some(r) = replacer {
                f(r);
            }
            f(space);
        }
        Expr::JsonStringifyFull(a, b, c) => {
            f(a);
            f(b);
            f(c);
        }
        Expr::ObjectDefineProperty(a, b, c) => {
            f(a);
            f(b);
            f(c);
        }
        Expr::ObjectGroupBy { items, key_fn } | Expr::MapGroupBy { items, key_fn } => {
            f(items);
            f(key_fn);
        }
        Expr::ArrayFromMapped {
            iterable,
            map_fn,
            this_arg,
        } => {
            f(iterable);
            f(map_fn);
            if let Some(t) = this_arg {
                f(t);
            }
        }

        // ─── Three-child variants ─────────────────────────────────────────
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            f(object);
            f(index);
            f(value);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            f(condition);
            f(then_expr);
            f(else_expr);
        }

        // ─── Vec<Expr> children ───────────────────────────────────────────
        Expr::Array(elements) | Expr::Sequence(elements) | Expr::SuperCall(elements) => {
            for e in elements {
                f(e);
            }
        }
        Expr::MathMin(elements) | Expr::MathMax(elements) | Expr::MathHypot(elements) => {
            for e in elements {
                f(e);
            }
        }
        Expr::PathWin32 { args, .. } => {
            for e in args {
                f(e);
            }
        }
        Expr::WebAssemblyCallExport {
            instance,
            name,
            args,
        } => {
            f(instance);
            f(name);
            for e in args {
                f(e);
            }
        }
        Expr::WebAssemblyModuleCustomSections { module, name } => {
            f(module);
            f(name);
        }
        Expr::DateUtc(elements) => {
            for e in elements {
                f(e);
            }
        }
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            f(home);
            f(key);
            f(receiver);
        }
        Expr::SuperPropertySet { key, value, .. } => {
            f(key);
            f(value);
        }
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            f(home);
            f(key);
            f(value);
            f(receiver);
        }
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            f(home);
            f(key);
            f(receiver);
            for a in args {
                f(a);
            }
        }
        Expr::SuperMethodCall { args, .. }
        | Expr::StaticMethodCall { args, .. }
        | Expr::New { args, .. } => {
            for a in args {
                f(a);
            }
        }
        Expr::Call { callee, args, .. } => {
            f(callee);
            for a in args {
                f(a);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            f(callee);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => f(e),
                }
            }
        }
        Expr::SuperCallSpread(args) | Expr::SuperMethodCallSpread { args, .. } => {
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => f(e),
                }
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    ArrayElement::Expr(e) | ArrayElement::Spread(e) => f(e),
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                f(v);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                f(v);
            }
        }
        Expr::ObjectAssign { target, sources } => {
            f(target);
            for s in sources {
                f(s);
            }
        }
        Expr::I18nString { params, .. } => {
            for (_, v) in params {
                f(v);
            }
        }
        Expr::NewDynamic { callee, args, .. } => {
            f(callee);
            for a in args {
                f(a);
            }
        }
        Expr::NewDynamicSpread { callee, args, .. } => {
            f(callee);
            for a in args {
                match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => f(e),
                }
            }
        }
        Expr::JsNew {
            module_handle,
            args,
            ..
        } => {
            f(module_handle);
            for a in args {
                f(a);
            }
        }
        Expr::JsNewFromHandle { constructor, args } => {
            f(constructor);
            for a in args {
                f(a);
            }
        }
        Expr::JsCallFunction {
            module_handle,
            args,
            ..
        } => {
            f(module_handle);
            for a in args {
                f(a);
            }
        }
        Expr::JsCallMethod { object, args, .. } => {
            f(object);
            for a in args {
                f(a);
            }
        }
        Expr::JsCallValue { callee, args } => {
            f(callee);
            for a in args {
                f(a);
            }
        }
        Expr::JsSetProperty { object, value, .. } => {
            f(object);
            f(value);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                f(o);
            }
            for a in args {
                f(a);
            }
        }

        // ─── Yield (optional value) ───────────────────────────────────────
        Expr::Yield { value, .. } => {
            if let Some(v) = value {
                f(v);
            }
        }

        // ─── Date constructors / setters ─────────────────────────────────
        Expr::DateNew(args) => {
            for a in args {
                f(a);
            }
        }
        Expr::BoxedPrimitiveNew { arg, .. } => f(arg),
        Expr::DateSetUtcFullYear { date, args }
        | Expr::DateSetUtcMonth { date, args }
        | Expr::DateSetUtcDate { date, args }
        | Expr::DateSetUtcHours { date, args }
        | Expr::DateSetUtcMinutes { date, args }
        | Expr::DateSetUtcSeconds { date, args }
        | Expr::DateSetUtcMilliseconds { date, args }
        | Expr::DateSetFullYear { date, args }
        | Expr::DateSetMonth { date, args }
        | Expr::DateSetDate { date, args }
        | Expr::DateSetHours { date, args }
        | Expr::DateSetMinutes { date, args }
        | Expr::DateSetSeconds { date, args }
        | Expr::DateSetMilliseconds { date, args }
        | Expr::DateSetTime { date, args } => {
            f(date);
            for a in args {
                f(a);
            }
        }

        // ─── Error constructors ───────────────────────────────────────────
        Expr::ErrorNew(opt) => {
            if let Some(v) = opt {
                f(v);
            }
        }
        Expr::ErrorNewWithCause { message, cause } => {
            f(message);
            f(cause);
        }
        Expr::ErrorNewWithOptions {
            message, options, ..
        } => {
            f(message);
            f(options);
        }
        Expr::AggregateErrorNew {
            errors,
            message,
            options,
        } => {
            f(errors);
            f(message);
            if let Some(o) = options {
                f(o);
            }
        }

        // ─── URL family ──────────────────────────────────────────────────
        Expr::UrlNew { url, base } => {
            f(url);
            if let Some(b) = base {
                f(b);
            }
        }
        Expr::UrlPatternNew { input, base } => {
            f(input);
            if let Some(b) = base {
                f(b);
            }
        }
        Expr::UrlSearchParamsNew(opt) => {
            if let Some(v) = opt {
                f(v);
            }
        }
        Expr::UrlSearchParamsGet { params, name }
        | Expr::UrlSearchParamsGetAll { params, name } => {
            f(params);
            f(name);
        }
        Expr::UrlSearchParamsHas {
            params,
            name,
            value,
        }
        | Expr::UrlSearchParamsDelete {
            params,
            name,
            value,
        } => {
            f(params);
            f(name);
            if let Some(v) = value {
                f(v);
            }
        }
        Expr::UrlSearchParamsSet {
            params,
            name,
            value,
        }
        | Expr::UrlSearchParamsAppend {
            params,
            name,
            value,
        } => {
            f(params);
            f(name);
            f(value);
        }

        // ─── RegExp ──────────────────────────────────────────────────────
        Expr::RegExpExec { regex, string }
        | Expr::RegExpTest { regex, string }
        | Expr::StringMatch { string, regex }
        | Expr::StringMatchAll { string, regex } => {
            f(regex);
            f(string);
        }
        Expr::RegExpDynamic {
            pattern,
            flags,
            is_call: _,
        } => {
            f(pattern);
            if let Some(flags_box) = flags {
                f(flags_box);
            }
        }
        Expr::RegExpSetLastIndex { regex, value } => {
            f(regex);
            f(value);
        }
        Expr::UrlSetPathname { url, value }
        | Expr::UrlSetSearch { url, value }
        | Expr::UrlSetHash { url, value }
        | Expr::UrlSetProtocol { url, value }
        | Expr::UrlSetHostname { url, value }
        | Expr::UrlSetPort { url, value }
        | Expr::UrlSetUsername { url, value }
        | Expr::UrlSetPassword { url, value }
        | Expr::UrlSetHref { url, value } => {
            f(url);
            f(value);
        }
        Expr::RegExpReplaceFn {
            string,
            regex,
            callback,
        } => {
            f(string);
            f(regex);
            f(callback);
        }
        Expr::StringReplace {
            string,
            pattern,
            replacement,
        } => {
            f(string);
            f(pattern);
            f(replacement);
        }
        Expr::AsyncStepChain {
            value,
            step_closure,
        } => {
            f(value);
            f(step_closure);
        }
        Expr::AsyncStepDone {
            value,
            step_closure,
        } => {
            f(value);
            f(step_closure);
        }
        Expr::CurrentStepClosure => {}
        Expr::AsyncFirstCall { step_closure } => {
            f(step_closure);
        }

        // ─── Buffer family ───────────────────────────────────────────────
        Expr::BufferFrom { data, encoding } => {
            f(data);
            if let Some(e) = encoding {
                f(e);
            }
        }
        Expr::StructuredClone { value, options } => {
            f(value);
            f(options);
        }
        Expr::LinkGeneratorPrototype { obj, .. } => {
            f(obj);
        }
        Expr::BufferFromArrayBuffer {
            data,
            byte_offset,
            length,
        } => {
            f(data);
            f(byte_offset);
            if let Some(e) = length {
                f(e);
            }
        }
        Expr::BufferAlloc {
            size,
            fill,
            encoding,
        } => {
            f(size);
            if let Some(v) = fill {
                f(v);
            }
            if let Some(v) = encoding {
                f(v);
            }
        }
        Expr::BufferByteLength { data, encoding } => {
            f(data);
            if let Some(e) = encoding {
                f(e);
            }
        }
        Expr::BufferToString { buffer, encoding } => {
            f(buffer);
            if let Some(e) = encoding {
                f(e);
            }
        }
        Expr::BufferSlice { buffer, start, end } => {
            f(buffer);
            if let Some(s) = start {
                f(s);
            }
            if let Some(e) = end {
                f(e);
            }
        }
        Expr::BufferCopy {
            source,
            target,
            target_start,
            source_start,
            source_end,
        } => {
            f(source);
            f(target);
            if let Some(v) = target_start {
                f(v);
            }
            if let Some(v) = source_start {
                f(v);
            }
            if let Some(v) = source_end {
                f(v);
            }
        }
        Expr::BufferWrite {
            buffer,
            string,
            offset,
            encoding,
        } => {
            f(buffer);
            f(string);
            if let Some(o) = offset {
                f(o);
            }
            if let Some(e) = encoding {
                f(e);
            }
        }
        Expr::BufferFill { buffer, value } => {
            f(buffer);
            f(value);
        }
        Expr::BufferEquals { buffer, other } => {
            f(buffer);
            f(other);
        }
        Expr::BufferIndexGet { buffer, index } => {
            f(buffer);
            f(index);
        }
        Expr::BufferIndexSet {
            buffer,
            index,
            value,
        } => {
            f(buffer);
            f(index);
            f(value);
        }

        // ─── Typed arrays ────────────────────────────────────────────────
        Expr::Uint8ArrayNew(opt) => {
            if let Some(v) = opt {
                f(v);
            }
        }
        Expr::TypedArrayNew { arg, .. } => {
            if let Some(v) = arg {
                f(v);
            }
        }
        Expr::Uint8ArrayGet { array, index } => {
            f(array);
            f(index);
        }
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => {
            f(array);
            f(index);
            f(value);
        }

        // ─── Process variants ────────────────────────────────────────────
        Expr::ProcessOn { event, handler } => {
            f(event);
            f(handler);
        }
        Expr::ProcessOnce { event, handler } => {
            f(event);
            f(handler);
        }
        Expr::ProcessStdinOn { event, handler } => {
            f(event);
            f(handler);
        }
        Expr::ProcessStdinRemoveListener { event, handler } => {
            f(event);
            f(handler);
        }
        Expr::ProcessStdinLifecycle(_) => {}
        Expr::ProcessStdoutOn { event, handler } => {
            f(event);
            f(handler);
        }
        Expr::ProcessKill { pid, signal } => {
            f(pid);
            if let Some(s) = signal {
                f(s);
            }
        }
        Expr::ProcessNextTick { callback, args } => {
            f(callback);
            for a in args {
                f(a);
            }
        }
        Expr::ProcessExit(opt) => {
            if let Some(v) = opt {
                f(v);
            }
        }
        Expr::ProcessUmask(opt) => {
            if let Some(v) = opt {
                f(v);
            }
        }
        Expr::ProcessThreadCpuUsage(opt) => {
            if let Some(v) = opt {
                f(v);
            }
        }
        Expr::ProcessEmitWarning(args) => {
            for a in args {
                f(a);
            }
        }
        Expr::ProcessCpuUsage(opt) => {
            if let Some(v) = opt {
                f(v);
            }
        }
        Expr::ProcessHrtime(opt) => {
            if let Some(v) = opt {
                f(v);
            }
        }
        Expr::ProcessSetTitle(v) => f(v),

        // ─── Child process ───────────────────────────────────────────────
        Expr::ChildProcessExecSync { command, options } => {
            f(command);
            if let Some(o) = options {
                f(o);
            }
        }
        Expr::ChildProcessSpawnSync {
            command,
            args,
            options,
        }
        | Expr::ChildProcessSpawn {
            command,
            args,
            options,
        } => {
            f(command);
            if let Some(a) = args {
                f(a);
            }
            if let Some(o) = options {
                f(o);
            }
        }
        Expr::ChildProcessFork {
            module,
            args,
            options,
        } => {
            f(module);
            if let Some(a) = args {
                f(a);
            }
            if let Some(o) = options {
                f(o);
            }
        }
        Expr::ChildProcessExec {
            command,
            options,
            callback,
        } => {
            f(command);
            if let Some(o) = options {
                f(o);
            }
            if let Some(c) = callback {
                f(c);
            }
        }
        Expr::ChildProcessExecFile {
            file,
            args,
            options,
            callback,
        } => {
            f(file);
            if let Some(a) = args {
                f(a);
            }
            if let Some(o) = options {
                f(o);
            }
            if let Some(c) = callback {
                f(c);
            }
        }
        Expr::ChildProcessExecFileSync {
            file,
            args,
            options,
        } => {
            f(file);
            if let Some(a) = args {
                f(a);
            }
            if let Some(o) = options {
                f(o);
            }
        }
        Expr::ChildProcessSpawnBackground {
            command,
            args,
            log_file,
            env_json,
        } => {
            f(command);
            if let Some(a) = args {
                f(a);
            }
            f(log_file);
            if let Some(e) = env_json {
                f(e);
            }
        }

        // ─── Fetch / Net ─────────────────────────────────────────────────
        Expr::FetchWithOptions {
            url,
            method,
            body,
            headers,
            headers_dynamic,
            signal,
        } => {
            f(url);
            f(method);
            f(body);
            for (_, v) in headers {
                f(v);
            }
            if let Some(hd) = headers_dynamic {
                f(hd);
            }
            if let Some(s) = signal {
                f(s);
            }
        }
        Expr::FetchGetWithAuth { url, auth_header } => {
            f(url);
            f(auth_header);
        }
        Expr::FetchPostWithAuth {
            url,
            auth_header,
            body,
        } => {
            f(url);
            f(auth_header);
            f(body);
        }
        Expr::NetCreateServer {
            options,
            connection_listener,
        } => {
            if let Some(o) = options {
                f(o);
            }
            if let Some(c) = connection_listener {
                f(c);
            }
        }
        Expr::NetCreateConnection {
            port,
            host,
            connect_listener,
        }
        | Expr::NetConnect {
            port,
            host,
            connect_listener,
        } => {
            f(port);
            if let Some(h) = host {
                f(h);
            }
            if let Some(c) = connect_listener {
                f(c);
            }
        }

        // ─── Array methods ───────────────────────────────────────────────
        Expr::ArrayPush { value, .. }
        | Expr::ArrayPushSpread { source: value, .. }
        | Expr::ArrayUnshift { value, .. }
        | Expr::SetAdd { value, .. } => {
            f(value);
        }
        Expr::ArrayIndexOf {
            array,
            value,
            from_index,
        }
        | Expr::ArrayIncludes {
            array,
            value,
            from_index,
        }
        | Expr::ArrayLastIndexOf {
            array,
            value,
            from_index,
        } => {
            f(array);
            f(value);
            if let Some(fi) = from_index {
                f(fi);
            }
        }
        Expr::ArraySlice { array, start, end } => {
            f(array);
            f(start);
            if let Some(e) = end {
                f(e);
            }
        }
        Expr::ArraySplice {
            array_id: _,
            start,
            delete_count,
            items,
        } => {
            f(start);
            if let Some(dc) = delete_count {
                f(dc);
            }
            for it in items {
                f(it);
            }
        }
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArrayFindLast { array, callback }
        | Expr::ArrayFindLastIndex { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFlatMap { array, callback }
        | Expr::ArraySort {
            array,
            comparator: callback,
        } => {
            f(array);
            f(callback);
        }
        Expr::ArrayAt { array, index } => {
            f(array);
            f(index);
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
            f(array);
            f(callback);
            if let Some(i) = initial {
                f(i);
            }
        }
        Expr::ArrayJoin { array, separator } => {
            f(array);
            if let Some(s) = separator {
                f(s);
            }
        }
        Expr::ArrayLikeMethod {
            method: _,
            receiver,
            args,
        } => {
            f(receiver);
            for a in args {
                f(a);
            }
        }
        Expr::ArrayToSorted { array, comparator } => {
            f(array);
            if let Some(c) = comparator {
                f(c);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            f(array);
            f(start);
            f(delete_count);
            for it in items {
                f(it);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            f(array);
            f(index);
            f(value);
        }
        Expr::ArrayReverseValue { receiver } => {
            f(receiver);
        }
        Expr::ArrayCopyWithin {
            array_id: _,
            target,
            start,
            end,
        } => {
            f(target);
            f(start);
            if let Some(e) = end {
                f(e);
            }
        }
        Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        } => {
            f(receiver);
            f(target);
            f(start);
            if let Some(e) = end {
                f(e);
            }
        }

        // ─── Map / Set methods (non-leaf) ────────────────────────────────
        Expr::MapSet { map, key, value } => {
            f(map);
            f(key);
            f(value);
        }
        Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
            f(map);
            f(key);
        }
        Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
            f(set);
            f(value);
        }

        // ─── Proxy / Reflect ─────────────────────────────────────────────
        Expr::ProxyNew { target, handler } | Expr::ProxyRevocable { target, handler } => {
            f(target);
            f(handler);
        }
        Expr::ProxyGet { proxy, key }
        | Expr::ProxyHas { proxy, key }
        | Expr::ProxyDelete { proxy, key } => {
            f(proxy);
            f(key);
        }
        Expr::ProxySet { proxy, key, value } => {
            f(proxy);
            f(key);
            f(value);
        }
        Expr::ProxyApply { proxy, args } | Expr::ProxyConstruct { proxy, args } => {
            f(proxy);
            for a in args {
                f(a);
            }
        }
        Expr::ReflectGet {
            target,
            key,
            receiver,
        } => {
            f(target);
            f(key);
            f(receiver);
        }
        Expr::ReflectHas { target, key } | Expr::ReflectDelete { target, key } => {
            f(target);
            f(key);
        }
        Expr::ReflectGetOwnPropertyDescriptor { target, key } => {
            f(target);
            f(key);
        }
        Expr::ReflectSet {
            target,
            key,
            value,
            receiver,
        } => {
            f(target);
            f(key);
            f(value);
            f(receiver);
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            f(target);
            f(key);
            f(value);
            f(receiver);
        }
        Expr::WithGet {
            object, fallback, ..
        } => {
            f(object);
            f(fallback);
        }
        Expr::WithSet { object, value, .. } => {
            f(object);
            f(value);
        }
        Expr::ReflectSetPrototypeOf { target, proto } => {
            f(target);
            f(proto);
        }
        Expr::ReflectApply {
            func,
            this_arg,
            args,
        } => {
            f(func);
            f(this_arg);
            f(args);
        }
        Expr::ReflectConstruct {
            target,
            args,
            new_target,
        } => {
            f(target);
            f(args);
            f(new_target);
        }
        Expr::ReflectDefineProperty {
            target,
            key,
            descriptor,
        } => {
            f(target);
            f(key);
            f(descriptor);
        }
        Expr::ReflectDefineMetadata {
            key,
            value,
            target,
            property_key,
        } => {
            f(key);
            f(value);
            f(target);
            if let Some(property_key) = property_key {
                f(property_key);
            }
        }
        Expr::ReflectGetMetadata {
            key,
            target,
            property_key,
        }
        | Expr::ReflectGetOwnMetadata {
            key,
            target,
            property_key,
        }
        | Expr::ReflectHasMetadata {
            key,
            target,
            property_key,
        }
        | Expr::ReflectHasOwnMetadata {
            key,
            target,
            property_key,
        }
        | Expr::ReflectDeleteMetadata {
            key,
            target,
            property_key,
        } => {
            f(key);
            f(target);
            if let Some(property_key) = property_key {
                f(property_key);
            }
        }
        Expr::ReflectGetMetadataKeys {
            target,
            property_key,
        }
        | Expr::ReflectGetOwnMetadataKeys {
            target,
            property_key,
        } => {
            f(target);
            if let Some(property_key) = property_key {
                f(property_key);
            }
        }

        // ─── FinalizationRegistry register/unregister ────────────────────
        Expr::FinalizationRegistryRegister {
            registry,
            target,
            held,
            token,
        } => {
            f(registry);
            f(target);
            f(held);
            if let Some(t) = token {
                f(t);
            }
        }
        Expr::FinalizationRegistryUnregister { registry, token } => {
            f(registry);
            f(token);
        }

        // ─── Closure: visit Param defaults only ──────────────────────────
        // The body (Vec<Stmt>) is intentionally not descended into here —
        // consumers handle closure body traversal themselves because they
        // often want different semantics (e.g. `replace_this_in_expr` skips
        // closures entirely, while `substitute_locals` calls its companion
        // `_in_stmts` helper). The `captures` / `mutable_captures` Vecs are
        // `LocalId`s, not `Expr`s, so they are not children either.
        Expr::Closure { params, .. } => {
            for p in params {
                if let Some(d) = &mut p.default {
                    f(d);
                }
            }
        }

        // Issue #100: dynamic import() — descend into the path arg.
        Expr::DynamicImport { arg, .. } => {
            f(arg);
        }
        Expr::WorkerNew {
            filename, options, ..
        } => {
            f(filename);
            if let Some(options) = options {
                f(options);
            }
        }
    }
}
