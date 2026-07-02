//! Issue #1098: extracted `lower_expr` dispatch table + `lower_math_operand`.
//!
//! Pure mechanical move out of `expr/mod.rs`. These `pub(crate)` free
//! functions are re-exported from the trunk so existing
//! `crate::expr::X` call paths resolve unchanged. The per-variant arm
//! bodies live in their own sibling modules (declared in the trunk); this
//! file only holds the outer dispatch `match`.
use super::*;

use anyhow::{bail, Result};
use perry_hir::{BinaryOp, Expr};
use perry_types::Type as HirType;

use crate::block::LlBlock;
use crate::codegen::AppMetadata;
use crate::collectors::NativeRegionFactGraph;
use crate::function::LlFunction;
use crate::native_value::{
    AliasState, BoundedBufferIndex, BoundsProof, BoundsState, BufferAccessFacts, BufferAccessMode,
    BufferViewSlot, GuardedBufferIndex, LoweredValue, MaterializationReason, NativeAbiTypeRecord,
    NativeFactUse, NativeRep, NativeRepRecord, NativeValueState, PodLayoutManifest,
    PodRecordViewManifest, ScalarConversionRecord,
};
use crate::strings::StringPool;
use crate::type_analysis::is_numeric_expr;
use crate::types::{DOUBLE, I32, I64, PTR};

/// Lower an expression to a raw LLVM `double` value. Returns the string form
/// of the value (either a `%rN` register or a literal like `42.0`).
///
/// Issue #1098: split into per-chunk sibling modules. The outer match
/// here is a dispatch table; each module's `lower(ctx, expr)` contains the
/// original arm bodies verbatim.
pub(crate) fn lower_expr(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    if let Some(lowered) = lower_expr_value(ctx, expr)? {
        if ctx.discard_expr_value {
            return Ok(materialize_js_value_without_record(ctx, lowered));
        }
        return Ok(materialize_js_value(
            ctx,
            lowered,
            MaterializationReason::RuntimeApi,
        ));
    }
    match expr {
        Expr::Integer(..)
        | Expr::Number(..)
        | Expr::Bool(..)
        | Expr::Undefined
        | Expr::Null
        | Expr::Void(..)
        | Expr::TypeOf(..)
        | Expr::String(..)
        | Expr::WtfString(..)
        | Expr::LocalGet(..)
        | Expr::LocalSet(..)
        | Expr::Update { .. }
        | Expr::DateNow => super::literals_vars::lower(ctx, expr),
        Expr::Binary { .. } => super::binary::lower(ctx, expr),
        Expr::Unary { .. } => super::unary::lower(ctx, expr),
        Expr::Compare { .. } => super::compare::lower(ctx, expr),
        Expr::Object(..) | Expr::Array(..) | Expr::ArraySpread(..) => {
            super::objects_arrays_lit::lower(ctx, expr)
        }
        Expr::IndexGet { .. } => super::index_get::lower(ctx, expr),
        Expr::IndexSet { .. } => super::index_set::lower(ctx, expr),
        Expr::PropertySet { .. } => super::property_set::lower(ctx, expr),
        Expr::PropertyGet { .. } => super::property_get::lower(ctx, expr),
        Expr::Conditional { .. } => super::conditional::lower(ctx, expr),
        Expr::ArrayPush { .. } | Expr::ArrayPushSpread { .. } => {
            super::array_push::lower(ctx, expr)
        }
        Expr::Closure { .. } => super::closure::lower(ctx, expr),
        Expr::New { .. } | Expr::NewDynamic { .. } | Expr::NewDynamicSpread { .. } => {
            super::new_dynamic::lower(ctx, expr)
        }
        Expr::This | Expr::NewTarget | Expr::SuperCall(..) | Expr::SuperCallSpread(..) => {
            super::this_super_call::lower(ctx, expr)
        }
        Expr::IsNaN(..)
        | Expr::MathPow(..)
        | Expr::MathImul(..)
        | Expr::ErrorNew(..)
        | Expr::ArrayPop(..)
        | Expr::ArrayMap { .. }
        | Expr::MapSet { .. }
        | Expr::MapGet { .. }
        | Expr::MapHas { .. }
        | Expr::MathSqrt(..)
        | Expr::MathFloor(..)
        | Expr::MathCeil(..)
        | Expr::MathRound(..)
        | Expr::MathTrunc(..)
        | Expr::MathSign(..)
        | Expr::MathAbs(..)
        | Expr::MathLog(..)
        | Expr::MathLog2(..)
        | Expr::MathLog10(..)
        | Expr::MathLog1p(..)
        | Expr::MathRandom
        | Expr::WebAssemblyValidate(..)
        | Expr::WebAssemblyCompile(..)
        | Expr::WebAssemblyModuleNew(..)
        | Expr::WebAssemblyModuleExports(..)
        | Expr::WebAssemblyModuleImports(..)
        | Expr::WebAssemblyModuleCustomSections { .. }
        | Expr::WebAssemblyInstantiate(..)
        | Expr::WebAssemblyCallExport { .. }
        | Expr::JsonStringifyFull(..)
        | Expr::MapNew => super::math_simple::lower(ctx, expr),
        Expr::Logical { .. }
        | Expr::ArrayFilter { .. }
        | Expr::FetchWithOptions { .. }
        | Expr::ArraySome { .. }
        | Expr::ArrayEvery { .. }
        | Expr::ArrayJoin { .. }
        | Expr::MapDelete { .. }
        | Expr::ObjectKeys(..)
        | Expr::ForInKeys(..)
        | Expr::IsFinite(..)
        | Expr::NumberIsFinite(..)
        | Expr::IsUndefinedOrBareNan(..)
        | Expr::MathMin(..)
        | Expr::MathMinSpread(..)
        | Expr::MathMax(..)
        | Expr::MathMaxSpread(..)
        | Expr::StringCoerce(..)
        | Expr::ObjectCoerce(..)
        | Expr::BooleanCoerce(..)
        | Expr::ArraySlice { .. }
        | Expr::ArrayShift(..)
        | Expr::ArrayLikeMethod { .. }
        | Expr::SetNew
        | Expr::In { .. }
        | Expr::PrivateBrandCheck { .. }
        | Expr::PrivateGuard { .. }
        | Expr::ParseInt { .. }
        | Expr::ParseFloat(..)
        | Expr::RegExp { .. }
        | Expr::RegExpDynamic { .. }
        | Expr::ObjectSpread { .. }
        | Expr::ObjectAssign { .. }
        | Expr::SetNewFromArray(..) => super::logical_collections::lower(ctx, expr),
        Expr::StaticMethodCall { .. } => super::static_method::lower(ctx, expr),
        Expr::SuperMethodCall { .. }
        | Expr::SuperMethodCallSpread { .. }
        | Expr::SuperPropertyGet { .. }
        | Expr::SuperPropertySet { .. }
        | Expr::ObjectSuperPropertyGet { .. }
        | Expr::ObjectSuperPropertySet { .. }
        | Expr::ObjectSuperMethodCall { .. }
        | Expr::FsReadFileBinary(..) => super::super_method::lower(ctx, expr),
        Expr::WithGet { .. }
        | Expr::WithSet { .. }
        | Expr::InstanceOf { .. }
        | Expr::Delete(..)
        | Expr::Sequence(..)
        | Expr::ArrayFrom(..)
        | Expr::ArrayFromArrayLikeHoley(..)
        | Expr::IteratorFrom(..)
        | Expr::TaggedTemplateStrings { .. }
        | Expr::TemplateRaw(..)
        | Expr::ArrayFromMapped { .. }
        | Expr::Uint8ArrayFrom(..)
        | Expr::ObjectValues(..)
        | Expr::ObjectEntries(..)
        | Expr::PathJoin(..)
        | Expr::PathWin32Join(..)
        | Expr::PathWin32 { .. }
        | Expr::QueueMicrotask(..)
        | Expr::ProcessNextTick { .. }
        | Expr::RegExpTest { .. }
        | Expr::RegExpExec { .. }
        | Expr::GlobalGet(..)
        | Expr::PathDirname(..)
        | Expr::PathRelative(..)
        | Expr::ArrayIncludes { .. }
        | Expr::ArraySplice { .. }
        | Expr::ObjectFromEntries(..)
        | Expr::ObjectGroupBy { .. }
        | Expr::MapGroupBy { .. }
        | Expr::StringMatch { .. }
        | Expr::StringMatchAll { .. }
        | Expr::PropertyUpdate { .. }
        | Expr::IndexUpdate { .. }
        | Expr::PathBasename(..)
        | Expr::PathBasenameExt(..)
        | Expr::PathParse(..)
        | Expr::JsonParse(..)
        | Expr::JsonRawJson(..)
        | Expr::JsonIsRawJson(..)
        | Expr::JsonParseTyped { .. }
        | Expr::JsonParseReviver { .. }
        | Expr::JsonParseWithReviver(..) => super::instance_misc1::lower(ctx, expr),
        Expr::DateNew(..)
        | Expr::BoxedPrimitiveNew { .. }
        | Expr::ArrayFind { .. }
        | Expr::ArrayFindIndex { .. }
        | Expr::ArrayFindLast { .. }
        | Expr::ArrayFindLastIndex { .. }
        | Expr::ObjectIs(..)
        | Expr::NumberIsInteger(..)
        | Expr::MapClear(..)
        | Expr::MapEntries(..)
        | Expr::MapKeys(..)
        | Expr::MapValues(..)
        | Expr::MapEntryKeyAt { .. }
        | Expr::MapEntryValueAt { .. }
        | Expr::SetValueAt { .. }
        | Expr::SetValues(..)
        | Expr::ObjectIsFrozen(..)
        | Expr::ObjectIsSealed(..)
        | Expr::ObjectIsExtensible(..)
        | Expr::FuncRef(..)
        | Expr::PathExtname(..)
        | Expr::PathSep
        | Expr::PathDelimiter
        | Expr::PathFormat(..)
        | Expr::PathToNamespacedPath(..)
        | Expr::PathMatchesGlob(..)
        | Expr::PathResolveJoin(..)
        | Expr::ProcessVersion
        | Expr::ObjectHasOwn(..)
        | Expr::NumberIsNaN(..)
        | Expr::FsMkdirSync(..)
        | Expr::IteratorToArray(..)
        | Expr::GetIterator(..)
        | Expr::GetAsyncIterator(..)
        | Expr::ForOfToArray(..)
        | Expr::ForAwaitToArray(..)
        | Expr::WeakRefDeref(..)
        | Expr::Uint8ArrayNew(..)
        | Expr::Uint8ArrayLength(..)
        | Expr::Uint8ArrayGet { .. }
        | Expr::Uint8ArraySet { .. }
        | Expr::BufferIndexGet { .. }
        | Expr::BufferIndexSet { .. }
        | Expr::TypedArrayNew { .. }
        | Expr::NativeArenaAlloc(..)
        | Expr::NativeArenaView { .. }
        | Expr::NativePodView { .. }
        | Expr::NativeArenaDispose(..)
        | Expr::ArrayUnshift { .. }
        | Expr::ArrayEntries(..)
        | Expr::ArrayKeys(..)
        | Expr::ArrayValues(..)
        | Expr::ClassRef(..) => super::arrays_finds::lower(ctx, expr),
        Expr::NativeMemoryFillU32 { .. } | Expr::NativeMemoryCopy { .. } => {
            super::native_memory::lower(ctx, expr)
        }
        Expr::CallSpread { .. } => super::call_spread::lower(ctx, expr),
        Expr::MathFround(..)
        | Expr::MathF16round(..)
        | Expr::MapNewFromArray(..)
        | Expr::DateGetTime(..)
        | Expr::DateGetTimezoneOffset(..)
        | Expr::DateUtc(..)
        | Expr::ObjectDefineProperty(..)
        | Expr::PathIsAbsolute(..)
        | Expr::ProcessHrtimeBigint
        | Expr::ProcessHrtime(..)
        | Expr::ProcessTitle
        | Expr::ProcessSetTitle(..)
        | Expr::RegExpExecIndex
        | Expr::CryptoRandomUUID
        | Expr::CryptoRandomUUIDv7
        | Expr::CryptoRandomBytes(..)
        | Expr::CryptoSha256(..)
        | Expr::CryptoMd5(..)
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
        | Expr::ArrayIndexOf { .. }
        | Expr::ArrayLastIndexOf { .. }
        | Expr::ArrayForEach { .. }
        | Expr::ObjectGetOwnPropertyDescriptor(..)
        | Expr::ObjectGetOwnPropertyDescriptors(..)
        | Expr::MathCbrt(..)
        | Expr::DateGetFullYear(..)
        | Expr::DateGetMonth(..)
        | Expr::DateGetUtcDay(..)
        | Expr::DateValueOf(..)
        | Expr::ProcessOn { .. }
        | Expr::ProcessOnce { .. }
        | Expr::ProcessStdinSetRawMode(..)
        | Expr::ProcessStdinOn { .. }
        | Expr::ProcessStdinRemoveListener { .. }
        | Expr::ProcessStdinLifecycle(..)
        | Expr::ProcessStdoutOn { .. }
        | Expr::TtyIsAtty(..)
        | Expr::ProcessStdinIsTTY
        | Expr::ProcessStdoutIsTTY
        | Expr::ProcessStderrIsTTY
        | Expr::ProcessStdoutColumns
        | Expr::ProcessStdoutRows
        | Expr::PerformanceNow
        | Expr::IterResultSet(..)
        | Expr::IterResultGetValue
        | Expr::IterResultGetDone
        | Expr::AsyncStepChain { .. }
        | Expr::AsyncStepDone { .. }
        | Expr::CurrentStepClosure
        | Expr::AsyncFirstCall { .. }
        | Expr::ObjectGetOwnPropertyNames(..)
        | Expr::MathHypot(..)
        | Expr::RegExpExecGroups => super::misc_methods::lower(ctx, expr),
        Expr::SetClear(..)
        | Expr::StringFromCodePoint(..)
        | Expr::StringFromCharCodeSpread(..)
        | Expr::StringRaw { .. }
        | Expr::StringAt { .. }
        | Expr::StringCodePointAt { .. }
        | Expr::RegExpSource(..)
        | Expr::RegExpFlags(..)
        | Expr::ProcessChdir(..)
        | Expr::ProcessExit(..)
        | Expr::ProcessAbort
        | Expr::ProcessUmask(..)
        | Expr::ObjectGetPrototypeOf(..)
        | Expr::ObjectDefineProperties(..)
        | Expr::ObjectSetPrototypeOf(..)
        | Expr::MathExpm1(..)
        | Expr::MathExp(..)
        | Expr::DateSetUtcFullYear { .. }
        | Expr::DateGetDate(..)
        | Expr::DateGetDay(..)
        | Expr::DateGetUtcDate(..)
        | Expr::DateGetUtcFullYear(..)
        | Expr::DateGetUtcMonth(..)
        | Expr::DateGetHours(..)
        | Expr::DateGetMinutes(..)
        | Expr::DateGetSeconds(..)
        | Expr::DateGetMilliseconds(..)
        | Expr::DateGetUtcHours(..)
        | Expr::DateGetUtcMinutes(..)
        | Expr::DateGetUtcSeconds(..)
        | Expr::DateGetUtcMilliseconds(..)
        | Expr::Atob(..)
        | Expr::Btoa(..)
        | Expr::ArrayFlat { .. }
        | Expr::ArrayFlatMap { .. }
        | Expr::MathSin(..)
        | Expr::MathCos(..)
        | Expr::MathSinh(..)
        | Expr::MathCosh(..)
        | Expr::MathTanh(..)
        | Expr::MathTan(..)
        | Expr::MathAsin(..)
        | Expr::MathAcos(..)
        | Expr::MathAtan(..)
        | Expr::MathAtan2(..)
        | Expr::StringFromCharCode(..)
        | Expr::RegExpSetLastIndex { .. }
        | Expr::ProcessStdin
        | Expr::ProcessStdout
        | Expr::ProcessStderr
        | Expr::MathAsinh(..)
        | Expr::MathAcosh(..)
        | Expr::MathAtanh(..)
        | Expr::DateSetUtcDate { .. }
        | Expr::DateSetUtcHours { .. }
        | Expr::ProcessKill { .. }
        | Expr::SymbolNew(..)
        | Expr::SymbolFor(..)
        | Expr::SymbolKeyFor(..)
        | Expr::SymbolDescription(..)
        | Expr::RegExpEscape(..)
        | Expr::SymbolToString(..)
        | Expr::ObjectGetOwnPropertySymbols(..)
        | Expr::TextEncoderNew
        | Expr::TextDecoderNew { .. }
        | Expr::TextEncoderEncode(..)
        | Expr::TextEncoderEncodeInto { .. }
        | Expr::TextDecoderDecode { .. }
        | Expr::TextDecoderEncoding(..)
        | Expr::TextDecoderFatal(..)
        | Expr::TextDecoderIgnoreBom(..)
        | Expr::OsArch
        | Expr::OsType
        | Expr::OsPlatform
        | Expr::OsRelease
        | Expr::OsHostname
        | Expr::OsHomedir
        | Expr::OsTmpdir
        | Expr::OsTotalmem
        | Expr::OsFreemem
        | Expr::OsUptime
        | Expr::OsCpus
        | Expr::OsNetworkInterfaces
        | Expr::OsUserInfo
        | Expr::OsUserInfoBuffer
        | Expr::OsDevNull
        | Expr::OsAvailableParallelism
        | Expr::OsEndianness
        | Expr::OsLoadavg
        | Expr::OsMachine => super::string_regex_proc::lower(ctx, expr),
        Expr::OsVersion
        | Expr::ProcessMemoryUsage
        | Expr::ProcessThreadCpuUsage(..)
        | Expr::ProcessAvailableMemory
        | Expr::ProcessConstrainedMemory
        | Expr::ProcessPosixCredential(..)
        | Expr::ProcessEmitWarning(..)
        | Expr::ProcessCpuUsage(..)
        | Expr::ProcessResourceUsage
        | Expr::ProcessActiveResourcesInfo
        | Expr::EncodeURI(..)
        | Expr::DecodeURI(..)
        | Expr::EncodeURIComponent(..)
        | Expr::DecodeURIComponent(..)
        | Expr::DateToString(..)
        | Expr::DateToDateString(..)
        | Expr::DateToTimeString(..)
        | Expr::DateToUTCString(..)
        | Expr::DateToLocaleDateString(..)
        | Expr::DateToLocaleTimeString(..)
        | Expr::DateToJSON(..)
        | Expr::ArrayReverseValue { .. }
        | Expr::ArrayWith { .. }
        | Expr::ArrayCopyWithin { .. }
        | Expr::ArrayCopyWithinValue { .. }
        | Expr::ArrayToReversed { .. }
        | Expr::ArrayToSorted { .. }
        | Expr::ArrayToSpliced { .. }
        | Expr::ArrayAt { .. }
        | Expr::DateSetUtcMinutes { .. }
        | Expr::DateSetUtcSeconds { .. }
        | Expr::DateSetUtcMilliseconds { .. }
        | Expr::Yield { .. }
        | Expr::TypeErrorNew(..)
        | Expr::RangeErrorNew(..)
        | Expr::SyntaxErrorNew(..)
        | Expr::ReferenceErrorNew(..)
        | Expr::NumberIsSafeInteger(..)
        | Expr::ObjectFreeze(..)
        | Expr::ObjectSeal(..)
        | Expr::ObjectPreventExtensions(..)
        | Expr::DateSetUtcMonth { .. }
        | Expr::DateSetFullYear { .. }
        | Expr::DateSetMonth { .. }
        | Expr::DateSetDate { .. }
        | Expr::DateSetHours { .. }
        | Expr::DateSetMinutes { .. }
        | Expr::DateSetSeconds { .. }
        | Expr::DateSetMilliseconds { .. }
        | Expr::DateSetTime { .. } => super::os_uri_dates::lower(ctx, expr),
        Expr::ArrayIsArray(..)
        | Expr::AggregateErrorNew { .. }
        | Expr::RegExpLastIndex(..)
        | Expr::BufferConcat(..)
        | Expr::BufferConcatWithLength { .. }
        | Expr::BufferSlice { .. }
        | Expr::BufferIsBuffer(..)
        | Expr::BufferIsEncoding(..)
        | Expr::StaticPluginResolve(..)
        | Expr::PathNormalize(..)
        | Expr::PathResolve(..)
        | Expr::ObjectCreate(..)
        | Expr::MathClz32(..)
        | Expr::FsReadFileSync(..)
        | Expr::FinalizationRegistryNew(..)
        | Expr::FinalizationRegistryRegister { .. }
        | Expr::FinalizationRegistryUnregister { .. }
        | Expr::ErrorNewWithCause { .. }
        | Expr::ErrorNewWithOptions { .. }
        | Expr::EnvGet(..)
        | Expr::EnvGetDynamic(..)
        | Expr::ProcessEnv => super::array_methods::lower(ctx, expr),
        Expr::GlobalThisExpr
        | Expr::ModuleTopThis
        | Expr::DateToISOString(..)
        | Expr::DateToLocaleString(..)
        | Expr::FetchGetWithAuth { .. }
        | Expr::FetchPostWithAuth { .. }
        | Expr::NetCreateServer { .. }
        | Expr::DateParse(..)
        | Expr::ProcessVersions
        | Expr::ProcessUptime
        | Expr::ProcessCwd
        | Expr::OsEOL
        | Expr::BufferFrom { .. }
        | Expr::BufferFromArrayBuffer { .. }
        | Expr::BufferAllocUnsafe(..)
        | Expr::BufferByteLength { .. }
        | Expr::BufferAlloc { .. }
        | Expr::ProcessPid
        | Expr::ProcessPpid
        | Expr::ProcessArgv
        | Expr::StructuredClone { .. }
        | Expr::WeakRefNew(..) => super::env_clones::lower(ctx, expr),
        Expr::FsUnlinkSync(..) | Expr::Await(..) => super::fs_await::lower(ctx, expr),
        Expr::StaticFieldGet { .. }
        | Expr::StaticFieldSet { .. }
        | Expr::RegisterClassParentDynamic { .. }
        | Expr::RegisterClassCaptures { .. }
        | Expr::ClassCaptureValue { .. }
        | Expr::RegisterClassStaticSymbol { .. }
        | Expr::RegisterClassComputedMethod { .. }
        | Expr::RegisterClassComputedAccessor { .. }
        | Expr::ClassExprFresh { .. }
        | Expr::SetFunctionPrototype { .. }
        | Expr::RegisterPrototypeMethod { .. }
        | Expr::RegisterFunctionPrototypeMethod { .. }
        | Expr::GetFunctionPrototypeMethod { .. }
        | Expr::ClassStaticSymbolSet { .. }
        | Expr::LinkGeneratorPrototype { .. }
        | Expr::NativeModuleRef(..) => super::static_field_meta::lower(ctx, expr),
        Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. } => super::pod_layout_constants::lower(ctx, expr),
        Expr::ObjectRest { .. }
        | Expr::BigInt(..)
        | Expr::BigIntCoerce(..)
        | Expr::ArraySort { .. }
        | Expr::ArrayReduce { .. }
        | Expr::ArrayReduceRight { .. }
        | Expr::EnumMember { .. }
        | Expr::FsExistsSync(..)
        | Expr::NumberCoerce(..)
        | Expr::SetAdd { .. }
        | Expr::SetHas { .. }
        | Expr::SetDelete { .. }
        | Expr::SetSize(..)
        | Expr::FsWriteFileSync(..)
        | Expr::FsAppendFileSync(..) => super::bigint_set::lower(ctx, expr),
        Expr::NativeMethodCall { .. } | Expr::Call { .. } => super::calls::lower(ctx, expr),
        Expr::ProxyNew { .. }
        | Expr::ProxyGet { .. }
        | Expr::ProxySet { .. }
        | Expr::ProxyHas { .. }
        | Expr::ProxyDelete { .. }
        | Expr::ProxyApply { .. }
        | Expr::ProxyConstruct { .. }
        | Expr::ProxyRevocable { .. }
        | Expr::ProxyRevoke(..)
        | Expr::ReflectGet { .. }
        | Expr::ReflectSet { .. }
        | Expr::PutValueSet { .. }
        | Expr::ReflectHas { .. }
        | Expr::ReflectDelete { .. }
        | Expr::ReflectOwnKeys(..)
        | Expr::ReflectApply { .. }
        | Expr::ReflectConstruct { .. }
        | Expr::ReflectDefineProperty { .. }
        | Expr::ReflectGetOwnPropertyDescriptor { .. }
        | Expr::ReflectGetPrototypeOf(..)
        | Expr::ReflectSetPrototypeOf { .. }
        | Expr::ReflectIsExtensible(..)
        | Expr::ReflectPreventExtensions(..)
        | Expr::ReflectDefineMetadata { .. }
        | Expr::ReflectGetMetadata { .. }
        | Expr::ReflectGetOwnMetadata { .. }
        | Expr::ReflectHasMetadata { .. }
        | Expr::ReflectHasOwnMetadata { .. }
        | Expr::ReflectGetMetadataKeys { .. }
        | Expr::ReflectGetOwnMetadataKeys { .. }
        | Expr::ReflectDeleteMetadata { .. } => super::proxy_reflect::lower(ctx, expr),
        Expr::DynamicImport { .. }
        | Expr::WorkerNew { .. }
        | Expr::ExternFuncRef { .. }
        | Expr::I18nString { .. } => super::dyn_extern_i18n::lower(ctx, expr),
        Expr::ChildProcessExecSync { .. }
        | Expr::ChildProcessSpawnSync { .. }
        | Expr::ChildProcessSpawnBackground { .. }
        | Expr::ChildProcessSpawn { .. }
        | Expr::ChildProcessFork { .. }
        | Expr::ChildProcessExec { .. }
        | Expr::ChildProcessExecFile { .. }
        | Expr::ChildProcessExecFileSync { .. }
        | Expr::ChildProcessGetProcessStatus(..)
        | Expr::ChildProcessKillProcess(..) => super::child_proc::lower(ctx, expr),
        Expr::FileURLToPath(..)
        | Expr::UrlNew { .. }
        | Expr::UrlPatternNew { .. }
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
        | Expr::UrlCanParse(..)
        | Expr::UrlCanParseWithBase { .. }
        | Expr::UrlParse(..)
        | Expr::UrlParseWithBase { .. }
        | Expr::UrlSearchParamsNew(..)
        | Expr::UrlSearchParamsMissingArgs { .. }
        | Expr::UrlSearchParamsGet { .. }
        | Expr::UrlSearchParamsHas { .. }
        | Expr::UrlSearchParamsSet { .. }
        | Expr::UrlSearchParamsAppend { .. }
        | Expr::UrlSearchParamsDelete { .. }
        | Expr::UrlSearchParamsToString(..)
        | Expr::UrlSearchParamsEntries(..)
        | Expr::UrlSearchParamsKeys(..)
        | Expr::UrlSearchParamsValues(..)
        | Expr::UrlSearchParamsSort(..)
        | Expr::UrlSearchParamsForEach { .. }
        | Expr::UrlSearchParamsGetAll { .. }
        | Expr::FsRmRecursive(..) => super::url_main::lower(ctx, expr),
        Expr::JsLoadModule { .. }
        | Expr::JsGetExport { .. }
        | Expr::JsCallFunction { .. }
        | Expr::JsCallMethod { .. }
        | Expr::JsCallValue { .. }
        | Expr::JsGetProperty { .. }
        | Expr::JsSetProperty { .. }
        | Expr::JsNew { .. }
        | Expr::JsNewFromHandle { .. }
        | Expr::JsCreateCallback { .. } => super::js_runtime::lower(ctx, expr),
        // -------- Unsupported (clear error) --------
        other => bail!(
            "perry-codegen Phase 2: expression {} not yet supported",
            variant_name(other)
        ),
    }
}

pub(crate) fn lower_math_operand(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    let raw = lower_expr(ctx, expr)?;
    if is_numeric_expr(ctx, expr)
        && !crate::type_analysis::expr_may_return_boxed_value_from_raw_f64_fallback(ctx, expr)
    {
        Ok(raw)
    } else {
        Ok(ctx
            .block()
            .call(DOUBLE, "js_math_to_number", &[(DOUBLE, &raw)]))
    }
}
