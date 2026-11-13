//! What a lowered HIR node names by itself (the node part of the text
//! rule): the families of the realm entry it is the intrinsic of.
//!
//! The match has no wildcard arm on purpose: a new HIR node does not
//! compile until it is classified here (DESIGN-family-reach §2.4).

use perry_hir::Expr;

/// The class of one node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    /// Names no family (its operands are visited on their own).
    Core,
    /// Reads or writes a property under a key that is a value: in a body
    /// generation does not walk it may name any member of a prototype.
    Computed,
    /// Every family (the reason).
    All(&'static str),
    /// These families.
    Fam(&'static [&'static str]),
    /// The intrinsic of this realm holder: everything the holder reaches.
    Entry(&'static str),
    /// A lowered member of this module.
    Spec(&'static str),
    /// The global object as a value.
    Global,
    /// Carries a module specifier (`text` reads it).
    Module,
}

#[rustfmt::skip]
pub(crate) fn class(e: &Expr) -> Node {
    match e {
        Expr::Undefined { .. } | Expr::Null { .. } | Expr::Bool { .. } | Expr::Number { .. }
        | Expr::Integer { .. } | Expr::String { .. } | Expr::WtfString { .. }
        | Expr::I18nString { .. } | Expr::LocalGet { .. } | Expr::LocalSet { .. }
        | Expr::ScopedTemp { .. } | Expr::GlobalGet { .. } | Expr::GlobalSet { .. }
        | Expr::Update { .. } | Expr::Binary { .. } | Expr::Unary { .. } | Expr::Compare { .. }
        | Expr::Logical { .. } | Expr::Call { .. } | Expr::CallSpread { .. }
        | Expr::SuperCallSpread { .. } | Expr::FuncRef { .. } | Expr::ExternFuncRef { .. }
        | Expr::PropertyGet { .. } | Expr::PropertySet { .. } | Expr::PropertyUpdate { .. }
        | Expr::Object { .. } | Expr::LiteralChunks { .. } | Expr::LiteralChunkObject { .. }
        | Expr::LiteralSlotInit { .. } | Expr::Array { .. } | Expr::ArraySpread { .. }
        | Expr::Conditional { .. } | Expr::TypeOf { .. } | Expr::Void { .. }
        | Expr::InstanceOf { .. } | Expr::PrivateLexicalBrand { .. }
        | Expr::PrivateLexicalBrandPush { .. } | Expr::PrivateLexicalBrandPop { .. }
        | Expr::ThisProvenClass { .. } | Expr::PrivateBrandCheck { .. }
        | Expr::PrivateGuard { .. } | Expr::Await { .. } | Expr::Yield { .. }
        | Expr::New { .. } | Expr::NewDynamic { .. } | Expr::NewDynamicSpread { .. }
        | Expr::NewTarget { .. } | Expr::ClassRef { .. } | Expr::ClassMethodHome { .. }
        | Expr::EnumMember { .. } | Expr::EnumImage { .. } | Expr::StaticFieldGet { .. }
        | Expr::StaticFieldSet { .. } | Expr::ClassStaticSymbolSet { .. }
        | Expr::RegisterClassParentDynamic { .. } | Expr::RegisterClassCaptures { .. }
        | Expr::RefreshClassExprCaptures { .. } | Expr::ClassEnvGet { .. }
        | Expr::ClassEnvSet { .. } | Expr::ClassEnvStamp { .. } | Expr::ClassEnvCurrent { .. }
        | Expr::ClassCaptureValue { .. } | Expr::RegisterClassStaticSymbol { .. }
        | Expr::RegisterClassComputedMethod { .. } | Expr::RegisterClassComputedAccessor { .. }
        | Expr::ClassExprFresh { .. } | Expr::ClassIsFirstEvaluation { .. }
        | Expr::SetFunctionPrototype { .. } | Expr::RegisterPrototypeMethod { .. }
        | Expr::RegisterFunctionPrototypeMethod { .. }
        | Expr::GetFunctionPrototypeMethod { .. } | Expr::StaticMethodCall { .. }
        | Expr::This { .. } | Expr::SuperCall { .. } | Expr::SuperMethodCall { .. }
        | Expr::SuperMethodCallSpread { .. } | Expr::SuperPropertyGet { .. }
        | Expr::SuperPropertySet { .. } | Expr::ObjectSuperPropertyGet { .. }
        | Expr::ObjectSuperPropertySet { .. } | Expr::ObjectSuperMethodCall { .. }
        | Expr::EnvGet { .. } | Expr::ProcessEnv { .. } | Expr::NamespacePatched { .. }
        | Expr::ModuleTopThis { .. } | Expr::ProcessUptime { .. } | Expr::ProcessCwd { .. }
        | Expr::ProcessArgv { .. } | Expr::ProcessMemoryUsage { .. } | Expr::ProcessPid { .. }
        | Expr::ProcessPpid { .. } | Expr::ProcessVersion { .. } | Expr::ProcessVersions { .. }
        | Expr::ProcessHrtime { .. } | Expr::ProcessNextTick { .. } | Expr::ProcessChdir { .. }
        | Expr::ProcessExit { .. } | Expr::ProcessAbort { .. } | Expr::ProcessUmask { .. }
        | Expr::ProcessThreadCpuUsage { .. } | Expr::ProcessAvailableMemory { .. }
        | Expr::ProcessConstrainedMemory { .. } | Expr::ProcessPosixCredential { .. }
        | Expr::ProcessEmitWarning { .. } | Expr::ProcessCpuUsage { .. }
        | Expr::ProcessResourceUsage { .. } | Expr::ProcessActiveResourcesInfo { .. }
        | Expr::ProcessTitle { .. } | Expr::ProcessSetTitle { .. } | Expr::PathJoin { .. }
        | Expr::PathDirname { .. } | Expr::PathBasename { .. } | Expr::PathBasenameExt { .. }
        | Expr::PathExtname { .. } | Expr::PathResolve { .. } | Expr::PathIsAbsolute { .. }
        | Expr::PathRelative { .. } | Expr::PathNormalize { .. } | Expr::PathParse { .. }
        | Expr::PathFormat { .. } | Expr::PathSep { .. } | Expr::PathDelimiter { .. }
        | Expr::PathToNamespacedPath { .. } | Expr::PathMatchesGlob { .. }
        | Expr::PathResolveJoin { .. } | Expr::PathWin32Join { .. } | Expr::PathWin32 { .. }
        | Expr::WeakRefNew { .. } | Expr::WeakRefDeref { .. }
        | Expr::FinalizationRegistryNew { .. } | Expr::FinalizationRegistryRegister { .. }
        | Expr::FinalizationRegistryUnregister { .. } | Expr::ObjectGetOwnPropertyNames { .. }
        | Expr::ObjectCreate { .. } | Expr::ObjectFreeze { .. } | Expr::ObjectSeal { .. }
        | Expr::ObjectPreventExtensions { .. } | Expr::ObjectIsFrozen { .. }
        | Expr::ObjectIsSealed { .. } | Expr::ObjectIsExtensible { .. }
        | Expr::ObjectGetPrototypeOf { .. } | Expr::ObjectSetPrototypeOf { .. }
        | Expr::ObjectGetOwnPropertySymbols { .. } | Expr::SymbolNew { .. }
        | Expr::SymbolFor { .. } | Expr::SymbolKeyFor { .. } | Expr::SymbolDescription { .. }
        | Expr::SymbolToString { .. } | Expr::MathFloor { .. } | Expr::MathCeil { .. }
        | Expr::MathRound { .. } | Expr::MathTrunc { .. } | Expr::MathSign { .. }
        | Expr::MathAbs { .. } | Expr::MathSqrt { .. } | Expr::MathLog { .. }
        | Expr::MathLog2 { .. } | Expr::MathLog10 { .. } | Expr::MathPow { .. }
        | Expr::MathMin { .. } | Expr::MathMax { .. } | Expr::MathMinSpread { .. }
        | Expr::MathMaxSpread { .. } | Expr::MathImul { .. } | Expr::MathRandom { .. }
        | Expr::MathSin { .. } | Expr::MathCos { .. } | Expr::MathTan { .. }
        | Expr::MathAsin { .. } | Expr::MathAcos { .. } | Expr::MathAtan { .. }
        | Expr::MathAtan2 { .. } | Expr::MathCbrt { .. } | Expr::MathHypot { .. }
        | Expr::MathFround { .. } | Expr::MathF16round { .. } | Expr::MathClz32 { .. }
        | Expr::MathExpm1 { .. } | Expr::MathLog1p { .. } | Expr::MathSinh { .. }
        | Expr::MathCosh { .. } | Expr::MathTanh { .. } | Expr::MathAsinh { .. }
        | Expr::MathAcosh { .. } | Expr::MathAtanh { .. } | Expr::MathExp { .. }
        | Expr::EncodeURI { .. } | Expr::DecodeURI { .. } | Expr::EncodeURIComponent { .. }
        | Expr::DecodeURIComponent { .. } | Expr::LinkGeneratorPrototype { .. }
        | Expr::QueueMicrotask { .. } | Expr::IterResultSet { .. }
        | Expr::IterResultGetValue { .. } | Expr::IterResultGetDone { .. }
        | Expr::AsyncStepChain { .. } | Expr::AsyncStepResumeNow { .. }
        | Expr::AsyncStepDeclined { .. } | Expr::AsyncStepDone { .. }
        | Expr::CurrentStepClosure { .. } | Expr::AsyncFirstCall { .. }
        | Expr::AsyncGenResume { .. } | Expr::ArrayPush { .. } | Expr::ArrayPushSpread { .. }
        | Expr::ArrayPop { .. } | Expr::ArrayShift { .. } | Expr::ArrayUnshift { .. }
        | Expr::ArrayIndexOf { .. } | Expr::ArrayLastIndexOf { .. }
        | Expr::ArrayIncludes { .. } | Expr::ArraySlice { .. } | Expr::ArraySplice { .. }
        | Expr::ArrayForEach { .. } | Expr::ArrayMap { .. } | Expr::ArrayFilter { .. }
        | Expr::ArrayFind { .. } | Expr::ArrayFindIndex { .. } | Expr::ArrayFindLast { .. }
        | Expr::ArrayFindLastIndex { .. } | Expr::ArrayAt { .. } | Expr::ArraySome { .. }
        | Expr::ArrayEvery { .. } | Expr::ArrayFlatMap { .. } | Expr::ArraySort { .. }
        | Expr::ArrayReduce { .. } | Expr::ArrayReduceRight { .. } | Expr::ArrayJoin { .. }
        | Expr::ArrayFlat { .. } | Expr::ArrayToReversed { .. } | Expr::ArrayToSorted { .. }
        | Expr::ArrayToSpliced { .. } | Expr::ArrayWith { .. } | Expr::ArrayReverseValue { .. }
        | Expr::ArrayCopyWithin { .. } | Expr::ArrayCopyWithinValue { .. }
        | Expr::ArrayEntries { .. } | Expr::ArrayKeys { .. } | Expr::ArrayValues { .. }
        | Expr::StringSplit { .. } | Expr::StringFromCharCode { .. }
        | Expr::StringFromCharCodeSpread { .. } | Expr::StringFromCodePoint { .. }
        | Expr::StringRaw { .. } | Expr::StringAt { .. } | Expr::StringCodePointAt { .. }
        | Expr::MapNew { .. } | Expr::MapNewFromArray { .. } | Expr::MapSet { .. }
        | Expr::MapGet { .. } | Expr::MapHas { .. } | Expr::MapDelete { .. }
        | Expr::MapSize { .. } | Expr::MapClear { .. } | Expr::MapEntries { .. }
        | Expr::MapKeys { .. } | Expr::MapValues { .. } | Expr::MapEntryKeyAt { .. }
        | Expr::MapEntryValueAt { .. } | Expr::SetNew { .. } | Expr::SetNewFromArray { .. }
        | Expr::SetAdd { .. } | Expr::SetHas { .. } | Expr::SetDelete { .. }
        | Expr::SetSize { .. } | Expr::SetClear { .. } | Expr::SetValues { .. }
        | Expr::SetValueAt { .. } | Expr::Sequence { .. } | Expr::BoxedPrimitiveNew { .. }
        | Expr::ErrorNew { .. } | Expr::ErrorMessage { .. } | Expr::ErrorNewWithCause { .. }
        | Expr::ErrorNewWithOptions { .. } | Expr::TypeErrorNew { .. }
        | Expr::RangeErrorNew { .. } | Expr::ReferenceErrorNew { .. }
        | Expr::SyntaxErrorNew { .. } | Expr::AggregateErrorNew { .. } | Expr::Closure { .. }
        | Expr::StringReplace { .. } | Expr::ObjectFromEntries { .. } | Expr::ObjectIs { .. }
        | Expr::ObjectHasOwn { .. } | Expr::ObjectKeys { .. } | Expr::ForInKeys { .. }
        | Expr::ObjectGroupBy { .. } | Expr::MapGroupBy { .. } | Expr::ArrayIsArray { .. }
        | Expr::ArrayFrom { .. } | Expr::ArrayFromArrayLikeHoley { .. }
        | Expr::IteratorFrom { .. } | Expr::TaggedTemplateStrings { .. }
        | Expr::TemplateRaw { .. } | Expr::IteratorToArray { .. } | Expr::GetIterator { .. }
        | Expr::ArrayIterationPatched { .. } | Expr::GetAsyncIterator { .. }
        | Expr::ForOfToArray { .. } | Expr::ForAwaitToArray { .. }
        | Expr::ArrayFromMapped { .. } | Expr::ParseInt { .. } | Expr::ParseFloat { .. }
        | Expr::NumberCoerce { .. } | Expr::StringCoerce { .. }
        | Expr::TemplateStringCoerce { .. } | Expr::ObjectCoerce { .. }
        | Expr::BooleanCoerce { .. } | Expr::IsNaN { .. } | Expr::IsUndefinedOrBareNan { .. }
        | Expr::IsFinite { .. } | Expr::NumberIsNaN { .. } | Expr::NumberIsFinite { .. }
        | Expr::NumberIsInteger { .. } | Expr::NumberIsSafeInteger { .. }
        | Expr::StaticPluginResolve { .. } | Expr::ImportMetaUrl { .. }
        | Expr::ReflectOwnKeys { .. } | Expr::ReflectGetPrototypeOf { .. }
        | Expr::ReflectSetPrototypeOf { .. } | Expr::ReflectIsExtensible { .. }
        | Expr::ReflectPreventExtensions { .. } | Expr::ReflectDefineMetadata { .. }
        | Expr::ReflectGetMetadata { .. } | Expr::ReflectGetOwnMetadata { .. }
        | Expr::ReflectHasMetadata { .. } | Expr::ReflectHasOwnMetadata { .. }
        | Expr::ReflectGetMetadataKeys { .. } | Expr::ReflectGetOwnMetadataKeys { .. }
        | Expr::ReflectDeleteMetadata { .. } => Node::Core,
        Expr::BigInt { .. } | Expr::ProcessHrtimeBigint { .. } | Expr::BigIntCoerce { .. } => Node::Fam(&["bigint"]),
        Expr::WithGet { .. } | Expr::WithSet { .. } => Node::All("with"),
        Expr::NativeModuleRef { .. } | Expr::NativeMethodCall { .. }
        | Expr::DynamicImport { .. } => Node::Module,
        Expr::IndexGet { .. } | Expr::IndexSet { .. } | Expr::IndexUpdate { .. }
        | Expr::ObjectSpread { .. } | Expr::ObjectAssign { .. } | Expr::In { .. }
        | Expr::EnvGetDynamic { .. } | Expr::ObjectDefineProperty { .. }
        | Expr::ObjectGetOwnPropertyDescriptor { .. }
        | Expr::ObjectGetOwnPropertyDescriptors { .. } | Expr::ObjectDefineProperties { .. }
        | Expr::ArrayLikeMethod { .. } | Expr::Delete { .. } | Expr::ObjectValues { .. }
        | Expr::ObjectEntries { .. } | Expr::ObjectRest { .. } | Expr::ReflectGet { .. }
        | Expr::ReflectSet { .. } | Expr::PutValueSet { .. } | Expr::ReflectHas { .. }
        | Expr::ReflectDelete { .. } | Expr::ReflectApply { .. }
        | Expr::ReflectConstruct { .. } | Expr::ReflectDefineProperty { .. }
        | Expr::ReflectGetOwnPropertyDescriptor { .. } => Node::Computed,
        Expr::GlobalThisExpr { .. } => Node::Global,
        Expr::ProcessOn { .. } | Expr::ProcessOnce { .. } | Expr::ProcessKill { .. }
        | Expr::ProcessStdout { .. } | Expr::ProcessStderr { .. }
        | Expr::ProcessStdoutOn { .. } | Expr::ProcessStdoutIsTTY { .. }
        | Expr::ProcessStderrIsTTY { .. } | Expr::ProcessStdoutColumns { .. }
        | Expr::ProcessStdoutRows { .. } | Expr::TtyIsAtty { .. } => Node::Fam(&["os"]),
        Expr::ProcessStdin { .. } | Expr::ProcessStdinSetRawMode { .. }
        | Expr::ProcessStdinOn { .. } | Expr::ProcessStdinRemoveListener { .. }
        | Expr::ProcessStdinLifecycle { .. } | Expr::ProcessStdinIsTTY { .. } => Node::Fam(&["os", "typedarray"]),
        Expr::FsReadFileSync { .. } | Expr::FsWriteFileSync { .. } | Expr::FsExistsSync { .. }
        | Expr::FsMkdirSync { .. } | Expr::FsUnlinkSync { .. } | Expr::FsAppendFileSync { .. }
        | Expr::FsReadFileBinary { .. } | Expr::FsRmRecursive { .. } => Node::Spec("fs"),
        Expr::RegExpEscape { .. } | Expr::RegExpExecIndex { .. }
        | Expr::RegExpExecGroups { .. } | Expr::RegExp { .. } | Expr::RegExpDynamic { .. }
        | Expr::StringMatch { .. } | Expr::StringMatchAll { .. } => Node::Entry("RegExp"),
        Expr::FileURLToPath { .. } | Expr::UrlNew { .. } | Expr::UrlPatternNew { .. }
        | Expr::UrlGetHref { .. } | Expr::UrlGetPathname { .. } | Expr::UrlGetProtocol { .. }
        | Expr::UrlGetHost { .. } | Expr::UrlGetHostname { .. } | Expr::UrlGetPort { .. }
        | Expr::UrlGetSearch { .. } | Expr::UrlGetHash { .. } | Expr::UrlGetOrigin { .. }
        | Expr::UrlGetSearchParams { .. } | Expr::UrlCanParse { .. }
        | Expr::UrlCanParseWithBase { .. } | Expr::UrlParse { .. }
        | Expr::UrlParseWithBase { .. } | Expr::UrlInstanceToString { .. }
        | Expr::UrlInstanceToJSON { .. } | Expr::UrlSetPathname { .. }
        | Expr::UrlSetSearch { .. } | Expr::UrlSetHash { .. } | Expr::UrlSetProtocol { .. }
        | Expr::UrlSetHostname { .. } | Expr::UrlSetPort { .. } | Expr::UrlSetUsername { .. }
        | Expr::UrlSetPassword { .. } | Expr::UrlSetHref { .. }
        | Expr::UrlSearchParamsNew { .. } | Expr::UrlSearchParamsMissingArgs { .. }
        | Expr::UrlSearchParamsGet { .. } | Expr::UrlSearchParamsHas { .. }
        | Expr::UrlSearchParamsSet { .. } | Expr::UrlSearchParamsAppend { .. }
        | Expr::UrlSearchParamsDelete { .. } | Expr::UrlSearchParamsToString { .. }
        | Expr::UrlSearchParamsGetAll { .. } | Expr::UrlSearchParamsEntries { .. }
        | Expr::UrlSearchParamsKeys { .. } | Expr::UrlSearchParamsValues { .. }
        | Expr::UrlSearchParamsSort { .. } | Expr::UrlSearchParamsForEach { .. } => Node::Entry("URL"),
        Expr::JsonParse { .. } | Expr::JsonParseTyped { .. } | Expr::JsonParseReviver { .. }
        | Expr::JsonParseWithReviver { .. } | Expr::JsonStringify { .. }
        | Expr::JsonStringifyPretty { .. } | Expr::JsonStringifyFull { .. }
        | Expr::JsonRawJson { .. } | Expr::JsonIsRawJson { .. } => Node::Fam(&["json_engine"]),
        Expr::PerformanceNow { .. } => Node::Entry("performance"),
        Expr::WebAssemblyValidate { .. } | Expr::WebAssemblyCompile { .. }
        | Expr::WebAssemblyModuleNew { .. } | Expr::WebAssemblyModuleExports { .. }
        | Expr::WebAssemblyModuleImports { .. } | Expr::WebAssemblyModuleCustomSections { .. }
        | Expr::WebAssemblyInstantiate { .. } | Expr::WebAssemblyCallExport { .. } => Node::Entry("WebAssembly"),
        Expr::Atob { .. } | Expr::Btoa { .. } => Node::Fam(&["event_target"]),
        Expr::TextEncoderNew { .. } | Expr::TextEncoderEncode { .. }
        | Expr::TextEncoderEncodeInto { .. } | Expr::TextDecoderNew { .. }
        | Expr::TextDecoderDecode { .. } | Expr::TextDecoderEncoding { .. }
        | Expr::TextDecoderFatal { .. } | Expr::TextDecoderIgnoreBom { .. } => Node::Entry("TextEncoder"),
        Expr::StructuredClone { .. } => Node::Entry("structuredClone"),
        Expr::CryptoRandomBytes { .. } | Expr::CryptoRandomUUID { .. }
        | Expr::CryptoRandomUUIDv7 { .. } | Expr::CryptoSha256 { .. } | Expr::CryptoMd5 { .. }
        | Expr::WebCryptoDigest { .. } | Expr::WebCryptoImportKey { .. }
        | Expr::WebCryptoExportKey { .. } | Expr::WebCryptoSign { .. }
        | Expr::WebCryptoVerify { .. } | Expr::WebCryptoDeriveBits { .. }
        | Expr::WebCryptoDeriveKey { .. } | Expr::WebCryptoEncrypt { .. }
        | Expr::WebCryptoDecrypt { .. } | Expr::WebCryptoGenerateKey { .. }
        | Expr::WebCryptoWrapKey { .. } | Expr::WebCryptoUnwrapKey { .. }
        | Expr::CryptoRandomFillSync { .. } => Node::Fam(&["global_webcrypto", "event_target", "typedarray", "turnloop", "bigint", "date"]),
        Expr::OsPlatform { .. } | Expr::OsArch { .. } | Expr::OsHostname { .. }
        | Expr::OsHomedir { .. } | Expr::OsTmpdir { .. } | Expr::OsTotalmem { .. }
        | Expr::OsFreemem { .. } | Expr::OsUptime { .. } | Expr::OsType { .. }
        | Expr::OsRelease { .. } | Expr::OsCpus { .. } | Expr::OsNetworkInterfaces { .. }
        | Expr::OsUserInfo { .. } | Expr::OsUserInfoBuffer { .. } | Expr::OsEOL { .. }
        | Expr::OsDevNull { .. } | Expr::OsAvailableParallelism { .. }
        | Expr::OsEndianness { .. } | Expr::OsLoadavg { .. } | Expr::OsMachine { .. }
        | Expr::OsVersion { .. } => Node::Spec("os"),
        Expr::BufferFrom { .. } | Expr::BufferFromArrayBuffer { .. } | Expr::BufferAlloc { .. }
        | Expr::BufferAllocUnsafe { .. } | Expr::BufferConcat { .. }
        | Expr::BufferConcatWithLength { .. } | Expr::BufferIsBuffer { .. }
        | Expr::BufferIsEncoding { .. } | Expr::BufferByteLength { .. }
        | Expr::BufferToString { .. } | Expr::BufferLength { .. } | Expr::BufferSlice { .. }
        | Expr::BufferCopy { .. } | Expr::BufferWrite { .. } | Expr::BufferFill { .. }
        | Expr::BufferEquals { .. } | Expr::BufferIndexGet { .. } | Expr::BufferIndexSet { .. } => Node::Entry("Buffer"),
        Expr::Uint8ArrayNew { .. } | Expr::Uint8ArrayFrom { .. }
        | Expr::Uint8ArrayLength { .. } | Expr::Uint8ArrayGet { .. }
        | Expr::Uint8ArraySet { .. } | Expr::TypedArrayNew { .. }
        | Expr::NativeArenaAlloc { .. } | Expr::NativeArenaView { .. }
        | Expr::NativePodView { .. } | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. } | Expr::PodLayoutOffsetOf { .. }
        | Expr::NativeArenaDispose { .. } | Expr::NativeMemoryFillU32 { .. }
        | Expr::NativeMemoryCopy { .. } => Node::Entry("Uint8Array"),
        Expr::ChildProcessExecSync { .. } | Expr::ChildProcessSpawnSync { .. }
        | Expr::ChildProcessSpawn { .. } | Expr::ChildProcessFork { .. }
        | Expr::ChildProcessExec { .. } | Expr::ChildProcessExecFile { .. }
        | Expr::ChildProcessExecFileSync { .. } | Expr::ChildProcessSpawnBackground { .. }
        | Expr::ChildProcessGetProcessStatus { .. } | Expr::ChildProcessKillProcess { .. } => Node::Spec("child_process"),
        Expr::FetchWithOptions { .. } | Expr::FetchGetWithAuth { .. }
        | Expr::FetchPostWithAuth { .. } => Node::Entry("fetch"),
        Expr::NetCreateServer { .. } | Expr::NetCreateConnection { .. }
        | Expr::NetConnect { .. } => Node::Spec("net"),
        Expr::DateNow { .. } | Expr::DateNew { .. } | Expr::DateGetTime { .. }
        | Expr::DateToISOString { .. } | Expr::DateGetFullYear { .. }
        | Expr::DateGetMonth { .. } | Expr::DateGetDate { .. } | Expr::DateGetDay { .. }
        | Expr::DateGetHours { .. } | Expr::DateGetMinutes { .. } | Expr::DateGetSeconds { .. }
        | Expr::DateGetMilliseconds { .. } | Expr::DateParse { .. } | Expr::DateUtc { .. }
        | Expr::DateGetUtcDay { .. } | Expr::DateGetUtcFullYear { .. }
        | Expr::DateGetUtcMonth { .. } | Expr::DateGetUtcDate { .. }
        | Expr::DateGetUtcHours { .. } | Expr::DateGetUtcMinutes { .. }
        | Expr::DateGetUtcSeconds { .. } | Expr::DateGetUtcMilliseconds { .. }
        | Expr::DateSetUtcFullYear { .. } | Expr::DateSetUtcMonth { .. }
        | Expr::DateSetUtcDate { .. } | Expr::DateSetUtcHours { .. }
        | Expr::DateSetUtcMinutes { .. } | Expr::DateSetUtcSeconds { .. }
        | Expr::DateSetUtcMilliseconds { .. } | Expr::DateSetFullYear { .. }
        | Expr::DateSetMonth { .. } | Expr::DateSetDate { .. } | Expr::DateSetHours { .. }
        | Expr::DateSetMinutes { .. } | Expr::DateSetSeconds { .. }
        | Expr::DateSetMilliseconds { .. } | Expr::DateSetTime { .. }
        | Expr::DateValueOf { .. } | Expr::DateToString { .. } | Expr::DateToDateString { .. }
        | Expr::DateToTimeString { .. } | Expr::DateToUTCString { .. }
        | Expr::DateGetTimezoneOffset { .. } | Expr::DateToJSON { .. } => Node::Entry("Date"),
        Expr::DateToLocaleDateString { .. } | Expr::DateToLocaleTimeString { .. }
        | Expr::DateToLocaleString { .. } => Node::Fam(&["date", "intl_namespace"]),
        Expr::JsLoadModule { .. } | Expr::JsGetExport { .. } | Expr::JsCallFunction { .. }
        | Expr::JsCallMethod { .. } | Expr::JsCallValue { .. } | Expr::JsGetProperty { .. }
        | Expr::JsSetProperty { .. } | Expr::JsNew { .. } | Expr::JsNewFromHandle { .. }
        | Expr::JsCreateCallback { .. } => Node::All("a module loaded as JavaScript"),
        Expr::ProxyNew { .. } | Expr::ProxyGet { .. } | Expr::ProxySet { .. }
        | Expr::ProxyHas { .. } | Expr::ProxyDelete { .. } | Expr::ProxyApply { .. }
        | Expr::ProxyConstruct { .. } | Expr::ProxyRevocable { .. } | Expr::ProxyRevoke { .. } => Node::Entry("Proxy"),
        Expr::WorkerNew { .. } => Node::Spec("worker_threads"),
    }
}
