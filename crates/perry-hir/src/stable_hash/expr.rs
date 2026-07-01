//! `SH` impl for the giant `Expr` HIR enum. Split out of `stable_hash.rs`
//! with NO behavior change — every match arm hashes the exact same bytes
//! in the exact same order. Original file was multi-line per arm
//! (4-5 lines on average); this file is hand-compacted to a single line
//! per arm so the impl fits under the 2,000-line cap. Exhaustiveness is
//! preserved (no wildcards) so adding a new HIR variant still forces a
//! compile error here.

use super::primitives::{tag, SH};
use super::StableHasher;
use crate::ir::*;

fn hash_with_set_fallback<H: StableHasher>(h: &mut H, fallback: &WithSetFallback) {
    match fallback {
        WithSetFallback::Local(id) => {
            tag(h, 0);
            id.hash(h);
        }
        WithSetFallback::ThrowReferenceError => tag(h, 1),
        WithSetFallback::ThrowConstAssignment => tag(h, 2),
        WithSetFallback::Ignore => tag(h, 3),
        WithSetFallback::SloppyImplicit(id) => {
            tag(h, 4);
            id.hash(h);
        }
    }
}

#[rustfmt::skip]
impl SH for Expr {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            Expr::Undefined => tag(h, 0),
            Expr::Null => tag(h, 1),
            Expr::Bool(b) => { tag(h, 2); b.hash(h); }
            Expr::Number(n) => { tag(h, 3); n.hash(h); }
            Expr::Integer(n) => { tag(h, 4); n.hash(h); }
            Expr::BigInt(s) => { tag(h, 5); s.hash(h); }
            Expr::String(s) => { tag(h, 6); s.hash(h); }
            Expr::WtfString(b) => { tag(h, 7); b.hash(h); }
            Expr::I18nString { key, string_idx, params, plural_forms, plural_param, } => { tag(h, 8); key.hash(h); string_idx.hash(h); params.hash(h); plural_forms.hash(h); plural_param.hash(h); }
            Expr::LocalGet(id) => { tag(h, 9); id.hash(h); }
            Expr::LocalSet(id, e) => { tag(h, 10); id.hash(h); e.as_ref().hash(h); }
            Expr::GlobalGet(id) => { tag(h, 11); id.hash(h); }
            Expr::GlobalSet(id, e) => { tag(h, 12); id.hash(h); e.as_ref().hash(h); }
            Expr::Update { id, op, prefix } => { tag(h, 13); id.hash(h); op.hash(h); prefix.hash(h); }
            Expr::Binary { op, left, right } => { tag(h, 14); op.hash(h); left.as_ref().hash(h); right.as_ref().hash(h); }
            Expr::Unary { op, operand } => { tag(h, 15); op.hash(h); operand.as_ref().hash(h); }
            Expr::Compare { op, left, right } => { tag(h, 16); op.hash(h); left.as_ref().hash(h); right.as_ref().hash(h); }
            Expr::Logical { op, left, right } => { tag(h, 17); op.hash(h); left.as_ref().hash(h); right.as_ref().hash(h); }
            // #5247: `byte_offset` is diagnostic-only (source-location metadata
            // for runtime TypeErrors); deliberately excluded from the stable hash
            // so source whitespace edits that shift offsets don't bust the object
            // cache.
            Expr::Call { callee, args, type_args, .. } => { tag(h, 18); callee.as_ref().hash(h); args.hash(h); type_args.hash(h); }
            Expr::CallSpread { callee, args, type_args, } => { tag(h, 19); callee.as_ref().hash(h); args.hash(h); type_args.hash(h); }
            Expr::SuperCallSpread(args) => { tag(h, 12240); for a in args { match a { CallArg::Expr(e) | CallArg::Spread(e) => e.hash(h), } } }
            Expr::PodLayoutSizeOf { ty } => { tag(h, 12001); ty.hash(h); }
            Expr::PodLayoutAlignOf { ty } => { tag(h, 12002); ty.hash(h); }
            Expr::PodLayoutOffsetOf { ty, field_path } => { tag(h, 12003); ty.hash(h); field_path.hash(h); }
            Expr::FuncRef(id) => { tag(h, 20); id.hash(h); }
            Expr::ExternFuncRef { name, param_types, return_type, } => { tag(h, 21); name.hash(h); param_types.hash(h); return_type.hash(h); }
            Expr::NativeModuleRef(s) => { tag(h, 22); s.hash(h); }
            Expr::NativeMethodCall { module, class_name, object, method, args, } => { tag(h, 23); module.hash(h); class_name.hash(h); object.hash(h); method.hash(h); args.hash(h); }
            Expr::PropertyGet { object, property } => { tag(h, 24); object.as_ref().hash(h); property.hash(h); }
            Expr::PropertySet { object, property, value, } => { tag(h, 25); object.as_ref().hash(h); property.hash(h); value.as_ref().hash(h); }
            Expr::PropertyUpdate { object, property, op, prefix, } => { tag(h, 26); object.as_ref().hash(h); property.hash(h); op.hash(h); prefix.hash(h); }
            Expr::IndexGet { object, index } => { tag(h, 27); object.as_ref().hash(h); index.as_ref().hash(h); }
            Expr::IndexSet { object, index, value, } => { tag(h, 28); object.as_ref().hash(h); index.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::IndexUpdate { object, index, op, prefix, } => { tag(h, 29); object.as_ref().hash(h); index.as_ref().hash(h); op.hash(h); prefix.hash(h); }
            Expr::Object(fields) => { tag(h, 30); fields.hash(h); }
            Expr::ObjectSpread { parts } => { tag(h, 31); parts.hash(h); }
            Expr::ObjectAssign { target, sources } => { tag(h, 32); target.as_ref().hash(h); sources.hash(h); }
            Expr::Array(items) => { tag(h, 33); items.hash(h); }
            Expr::ArraySpread(items) => { tag(h, 34); items.hash(h); }
            Expr::Conditional { condition, then_expr, else_expr, } => { tag(h, 35); condition.as_ref().hash(h); then_expr.as_ref().hash(h); else_expr.as_ref().hash(h); }
            Expr::TypeOf(e) => { tag(h, 36); e.as_ref().hash(h); }
            Expr::Void(e) => { tag(h, 37); e.as_ref().hash(h); }
            Expr::InstanceOf { expr, ty, ty_expr } => { tag(h, 38); expr.as_ref().hash(h); ty.hash(h); ty_expr.hash(h); }
            Expr::In { property, object } => { tag(h, 39); property.as_ref().hash(h); object.as_ref().hash(h); }
            Expr::PrivateBrandCheck { class_name, field_name, object } => { tag(h, 12401); class_name.hash(h); field_name.hash(h); object.as_ref().hash(h); }
            Expr::PrivateGuard { class_name, field_name, kind, op, object } => { tag(h, 12402); class_name.hash(h); field_name.hash(h); kind.hash(h); op.hash(h); object.as_ref().hash(h); }
            Expr::Await(e) => { tag(h, 40); e.as_ref().hash(h); }
            Expr::Yield { value, delegate } => { tag(h, 41); value.hash(h); delegate.hash(h); }
            // #5253: `byte_offset` is diagnostic-only — excluded from the hash
            // for the same reason as `Call.byte_offset` (see #5247 above).
            Expr::New { class_name, args, type_args, .. } => { tag(h, 42); class_name.hash(h); args.hash(h); type_args.hash(h); }
            Expr::NewDynamic { callee, args, .. } => { tag(h, 43); callee.as_ref().hash(h); args.hash(h); }
            Expr::NewDynamicSpread { callee, args, .. } => { tag(h, 12507); callee.as_ref().hash(h); args.hash(h); }
            Expr::NewTarget => { tag(h, 12301); }
            Expr::ClassRef(s) => { tag(h, 44); s.hash(h); }
            Expr::EnumMember { enum_name, member_name, } => { tag(h, 45); enum_name.hash(h); member_name.hash(h); }
            Expr::StaticFieldGet { class_name, field_name, } => { tag(h, 46); class_name.hash(h); field_name.hash(h); }
            Expr::StaticFieldSet { class_name, field_name, value, } => { tag(h, 47); class_name.hash(h); field_name.hash(h); value.as_ref().hash(h); }
            Expr::ClassStaticSymbolSet { class_name, key, value, } => { tag(h, 48); class_name.hash(h); key.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::StaticMethodCall { class_name, method_name, args, } => { tag(h, 49); class_name.hash(h); method_name.hash(h); args.hash(h); }
            Expr::This => tag(h, 50),
            Expr::SuperCall(args) => { tag(h, 51); args.hash(h); }
            Expr::SuperMethodCall { method, args } => { tag(h, 52); method.hash(h); args.hash(h); }
            Expr::SuperMethodCallSpread { method, args } => { tag(h, 12509); method.hash(h); for a in args { match a { CallArg::Expr(e) | CallArg::Spread(e) => e.hash(h), } } }
            Expr::SuperPropertyGet { property } => { tag(h, 461); property.hash(h); }
            Expr::SuperPropertySet { parent_class_id, parent_class_name, key, value } => { tag(h, 12238); parent_class_id.hash(h); parent_class_name.hash(h); key.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::ObjectSuperPropertyGet { home, key, receiver } => { tag(h, 12231); home.as_ref().hash(h); key.as_ref().hash(h); receiver.as_ref().hash(h); }
            Expr::ObjectSuperPropertySet { home, key, value, receiver } => { tag(h, 12239); home.as_ref().hash(h); key.as_ref().hash(h); value.as_ref().hash(h); receiver.as_ref().hash(h); }
            Expr::ObjectSuperMethodCall { home, key, receiver, args } => { tag(h, 12232); home.as_ref().hash(h); key.as_ref().hash(h); receiver.as_ref().hash(h); args.hash(h); }
            Expr::EnvGet(s) => { tag(h, 53); s.hash(h); }
            Expr::EnvGetDynamic(e) => { tag(h, 54); e.as_ref().hash(h); }
            Expr::ProcessEnv => tag(h, 55),
            Expr::GlobalThisExpr => tag(h, 474),
            Expr::ModuleTopThis => tag(h, 4741),
            Expr::ProcessUptime => tag(h, 56),
            Expr::ProcessCwd => tag(h, 57),
            Expr::ProcessArgv => tag(h, 58),
            Expr::ProcessMemoryUsage => tag(h, 59),
            Expr::ProcessPid => tag(h, 60),
            Expr::ProcessPpid => tag(h, 61),
            Expr::ProcessVersion => tag(h, 62),
            Expr::ProcessVersions => tag(h, 63),
            Expr::ProcessHrtimeBigint => tag(h, 64),
            Expr::ProcessNextTick { callback, args } => { tag(h, 65); callback.as_ref().hash(h); for a in args { a.hash(h); } }
            Expr::ProcessOn { event, handler } => { tag(h, 66); event.as_ref().hash(h); handler.as_ref().hash(h); }
            Expr::ProcessOnce { event, handler } => { tag(h, 11223); event.as_ref().hash(h); handler.as_ref().hash(h); }
            Expr::ProcessChdir(e) => { tag(h, 67); e.as_ref().hash(h); }
            Expr::ProcessKill { pid, signal } => { tag(h, 68); pid.as_ref().hash(h); signal.hash(h); }
            Expr::ProcessExit(e) => { tag(h, 69); e.hash(h); }
            Expr::ProcessAbort => tag(h, 11224),
            Expr::ProcessUmask(e) => { tag(h, 11225); e.hash(h); }
            Expr::ProcessThreadCpuUsage(e) => { tag(h, 11226); e.hash(h); }
            Expr::ProcessAvailableMemory => tag(h, 11227),
            Expr::ProcessConstrainedMemory => tag(h, 11228),
            Expr::ProcessPosixCredential(k) => { tag(h, 11229); (*k as u8).hash(h); }
            Expr::ProcessEmitWarning(args) => { tag(h, 11230); for a in args { a.hash(h); } }
            Expr::ProcessCpuUsage(e) => { tag(h, 11231); e.hash(h); }
            Expr::ProcessResourceUsage => tag(h, 11232),
            Expr::ProcessActiveResourcesInfo => tag(h, 11233),
            Expr::ProcessHrtime(e) => { tag(h, 11234); e.hash(h); }
            Expr::ProcessTitle => tag(h, 11235),
            Expr::ProcessSetTitle(e) => { tag(h, 11236); e.as_ref().hash(h); }
            Expr::ProcessStdin => tag(h, 70),
            Expr::ProcessStdout => tag(h, 71),
            Expr::ProcessStderr => tag(h, 72),
            Expr::ProcessStdinSetRawMode(e) => { tag(h, 73); e.as_ref().hash(h); }
            Expr::ProcessStdinOn { event, handler } => { tag(h, 74); event.as_ref().hash(h); handler.as_ref().hash(h); }
            Expr::ProcessStdinRemoveListener { event, handler } => { tag(h, 11241); event.as_ref().hash(h); handler.as_ref().hash(h); }
            Expr::ProcessStdinLifecycle(method) => {
                tag(h, 11242);
                tag(h, match method {
                    ProcessStdinLifecycleMethod::Pause => 1,
                    ProcessStdinLifecycleMethod::Resume => 2,
                    ProcessStdinLifecycleMethod::Unref => 3,
                    ProcessStdinLifecycleMethod::Ref => 4,
                    ProcessStdinLifecycleMethod::Destroy => 5,
                });
            }
            Expr::ProcessStdoutOn { event, handler } => { tag(h, 75); event.as_ref().hash(h); handler.as_ref().hash(h); }
            Expr::ProcessStdinIsTTY => tag(h, 76),
            Expr::ProcessStdoutIsTTY => tag(h, 77),
            Expr::ProcessStderrIsTTY => tag(h, 78),
            Expr::ProcessStdoutColumns => tag(h, 79),
            Expr::ProcessStdoutRows => tag(h, 80),
            Expr::TtyIsAtty(e) => { tag(h, 81); e.as_ref().hash(h); }
            Expr::FsReadFileSync(e) => { tag(h, 82); e.as_ref().hash(h); }
            Expr::FsWriteFileSync(a, b) => { tag(h, 83); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::FsExistsSync(e) => { tag(h, 84); e.as_ref().hash(h); }
            Expr::FsMkdirSync(e) => { tag(h, 85); e.as_ref().hash(h); }
            Expr::FsUnlinkSync(e) => { tag(h, 86); e.as_ref().hash(h); }
            Expr::FsAppendFileSync(a, b) => { tag(h, 87); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::FsReadFileBinary(e) => { tag(h, 88); e.as_ref().hash(h); }
            Expr::FsRmRecursive(e) => { tag(h, 89); e.as_ref().hash(h); }
            Expr::PathJoin(a, b) => { tag(h, 90); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::PathWin32Join(a, b) => { tag(h, 462); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::PathWin32 { method, args } => { tag(h, 11222); (*method as u32).hash(h); for a in args { a.hash(h); } }
            Expr::PathDirname(e) => { tag(h, 91); e.as_ref().hash(h); }
            Expr::PathBasename(e) => { tag(h, 92); e.as_ref().hash(h); }
            Expr::PathBasenameExt(a, b) => { tag(h, 93); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::PathExtname(e) => { tag(h, 94); e.as_ref().hash(h); }
            Expr::PathResolve(e) => { tag(h, 95); e.as_ref().hash(h); }
            Expr::PathIsAbsolute(e) => { tag(h, 96); e.as_ref().hash(h); }
            Expr::PathRelative(a, b) => { tag(h, 97); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::PathNormalize(e) => { tag(h, 98); e.as_ref().hash(h); }
            Expr::PathParse(e) => { tag(h, 99); e.as_ref().hash(h); }
            Expr::PathFormat(e) => { tag(h, 100); e.as_ref().hash(h); }
            Expr::PathSep => tag(h, 101),
            Expr::PathDelimiter => tag(h, 102),
            Expr::PathToNamespacedPath(e) => { tag(h, 449); e.as_ref().hash(h); }
            Expr::PathMatchesGlob(a, b) => { tag(h, 450); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::PathResolveJoin(a, b) => { tag(h, 451); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::WeakRefNew(e) => { tag(h, 103); e.as_ref().hash(h); }
            Expr::WeakRefDeref(e) => { tag(h, 104); e.as_ref().hash(h); }
            Expr::FinalizationRegistryNew(e) => { tag(h, 105); e.as_ref().hash(h); }
            Expr::FinalizationRegistryRegister { registry, target, held, token, } => { tag(h, 106); registry.as_ref().hash(h); target.as_ref().hash(h); held.as_ref().hash(h); token.hash(h); }
            Expr::FinalizationRegistryUnregister { registry, token } => { tag(h, 107); registry.as_ref().hash(h); token.as_ref().hash(h); }
            Expr::ObjectDefineProperty(a, b, c) => { tag(h, 108); a.as_ref().hash(h); b.as_ref().hash(h); c.as_ref().hash(h); }
            Expr::ObjectGetOwnPropertyDescriptor(a, b) => { tag(h, 109); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::ObjectGetOwnPropertyDescriptors(e) => { tag(h, 11237); e.as_ref().hash(h); }
            Expr::ObjectGetOwnPropertyNames(e) => { tag(h, 110); e.as_ref().hash(h); }
            Expr::ObjectCreate(e, props) => { tag(h, 111); e.as_ref().hash(h); if let Some(p) = props { tag(h, 1); p.as_ref().hash(h); } else { tag(h, 0); } }
            Expr::ObjectFreeze(e) => { tag(h, 112); e.as_ref().hash(h); }
            Expr::ObjectSeal(e) => { tag(h, 113); e.as_ref().hash(h); }
            Expr::ObjectPreventExtensions(e) => { tag(h, 114); e.as_ref().hash(h); }
            Expr::ObjectIsFrozen(e) => { tag(h, 115); e.as_ref().hash(h); }
            Expr::ObjectIsSealed(e) => { tag(h, 116); e.as_ref().hash(h); }
            Expr::ObjectIsExtensible(e) => { tag(h, 117); e.as_ref().hash(h); }
            Expr::ObjectGetPrototypeOf(e) => { tag(h, 118); e.as_ref().hash(h); }
            Expr::ObjectSetPrototypeOf(a, b) => { tag(h, 453); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::ObjectDefineProperties(a, b) => { tag(h, 454); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::ObjectGetOwnPropertySymbols(e) => { tag(h, 119); e.as_ref().hash(h); }
            Expr::SymbolNew(e) => { tag(h, 120); e.hash(h); }
            Expr::SymbolFor(e) => { tag(h, 121); e.as_ref().hash(h); }
            Expr::RegExpEscape(e) => { tag(h, 12043); e.as_ref().hash(h); }
            Expr::SymbolKeyFor(e) => { tag(h, 122); e.as_ref().hash(h); }
            Expr::SymbolDescription(e) => { tag(h, 123); e.as_ref().hash(h); }
            Expr::SymbolToString(e) => { tag(h, 124); e.as_ref().hash(h); }
            Expr::FileURLToPath(e) => { tag(h, 125); e.as_ref().hash(h); }
            Expr::RegExpExec { regex, string } => { tag(h, 126); regex.as_ref().hash(h); string.as_ref().hash(h); }
            Expr::RegExpSource(e) => { tag(h, 127); e.as_ref().hash(h); }
            Expr::RegExpFlags(e) => { tag(h, 128); e.as_ref().hash(h); }
            Expr::RegExpLastIndex(e) => { tag(h, 129); e.as_ref().hash(h); }
            Expr::RegExpSetLastIndex { regex, value } => { tag(h, 130); regex.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::RegExpReplaceFn { string, regex, callback, } => { tag(h, 131); string.as_ref().hash(h); regex.as_ref().hash(h); callback.as_ref().hash(h); }
            Expr::RegExpExecIndex => tag(h, 132),
            Expr::RegExpExecGroups => tag(h, 133),
            Expr::JsonParse(e) => { tag(h, 134); e.as_ref().hash(h); }
            Expr::JsonParseTyped { text, ty, ordered_keys, } => { tag(h, 135); text.as_ref().hash(h); ty.hash(h); ordered_keys.hash(h); }
            Expr::JsonParseReviver { text, reviver } => { tag(h, 136); text.as_ref().hash(h); reviver.as_ref().hash(h); }
            Expr::JsonParseWithReviver(a, b) => { tag(h, 137); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::JsonStringify(e) => { tag(h, 138); e.as_ref().hash(h); }
            Expr::JsonStringifyPretty { value, replacer, space, } => { tag(h, 139); value.as_ref().hash(h); replacer.hash(h); space.as_ref().hash(h); }
            Expr::JsonStringifyFull(a, b, c) => { tag(h, 140); a.as_ref().hash(h); b.as_ref().hash(h); c.as_ref().hash(h); }
            Expr::JsonRawJson(e) => { tag(h, 12130); e.as_ref().hash(h); }
            Expr::JsonIsRawJson(e) => { tag(h, 12131); e.as_ref().hash(h); }
            Expr::MathFloor(e) => { tag(h, 141); e.as_ref().hash(h); }
            Expr::MathCeil(e) => { tag(h, 142); e.as_ref().hash(h); }
            Expr::MathRound(e) => { tag(h, 143); e.as_ref().hash(h); }
            Expr::MathTrunc(e) => { tag(h, 12062); e.as_ref().hash(h); }
            Expr::MathSign(e) => { tag(h, 12063); e.as_ref().hash(h); }
            Expr::MathAbs(e) => { tag(h, 144); e.as_ref().hash(h); }
            Expr::MathSqrt(e) => { tag(h, 145); e.as_ref().hash(h); }
            Expr::MathLog(e) => { tag(h, 146); e.as_ref().hash(h); }
            Expr::MathLog2(e) => { tag(h, 147); e.as_ref().hash(h); }
            Expr::MathLog10(e) => { tag(h, 148); e.as_ref().hash(h); }
            Expr::MathPow(a, b) => { tag(h, 149); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::MathMin(es) => { tag(h, 150); es.hash(h); }
            Expr::MathMax(es) => { tag(h, 151); es.hash(h); }
            Expr::MathMinSpread(e) => { tag(h, 152); e.as_ref().hash(h); }
            Expr::MathMaxSpread(e) => { tag(h, 153); e.as_ref().hash(h); }
            Expr::MathImul(a, b) => { tag(h, 154); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::MathRandom => tag(h, 155),
            Expr::MathSin(e) => { tag(h, 156); e.as_ref().hash(h); }
            Expr::MathCos(e) => { tag(h, 157); e.as_ref().hash(h); }
            Expr::MathTan(e) => { tag(h, 158); e.as_ref().hash(h); }
            Expr::MathAsin(e) => { tag(h, 159); e.as_ref().hash(h); }
            Expr::MathAcos(e) => { tag(h, 160); e.as_ref().hash(h); }
            Expr::MathAtan(e) => { tag(h, 161); e.as_ref().hash(h); }
            Expr::MathAtan2(a, b) => { tag(h, 162); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::MathCbrt(e) => { tag(h, 163); e.as_ref().hash(h); }
            Expr::MathHypot(es) => { tag(h, 164); es.hash(h); }
            Expr::MathFround(e) => { tag(h, 165); e.as_ref().hash(h); }
            Expr::MathF16round(e) => { tag(h, 12041); e.as_ref().hash(h); }
            Expr::MathClz32(e) => { tag(h, 166); e.as_ref().hash(h); }
            Expr::MathExpm1(e) => { tag(h, 167); e.as_ref().hash(h); }
            Expr::MathLog1p(e) => { tag(h, 168); e.as_ref().hash(h); }
            Expr::MathSinh(e) => { tag(h, 169); e.as_ref().hash(h); }
            Expr::MathCosh(e) => { tag(h, 170); e.as_ref().hash(h); }
            Expr::MathTanh(e) => { tag(h, 171); e.as_ref().hash(h); }
            Expr::MathAsinh(e) => { tag(h, 172); e.as_ref().hash(h); }
            Expr::MathAcosh(e) => { tag(h, 173); e.as_ref().hash(h); }
            Expr::MathAtanh(e) => { tag(h, 174); e.as_ref().hash(h); }
            Expr::MathExp(e) => { tag(h, 175); e.as_ref().hash(h); }
            Expr::PerformanceNow => tag(h, 176),
            Expr::Atob(e) => { tag(h, 177); e.as_ref().hash(h); }
            Expr::Btoa(e) => { tag(h, 178); e.as_ref().hash(h); }
            Expr::TextEncoderNew => tag(h, 179),
            Expr::TextEncoderEncode(e) => { tag(h, 180); e.as_ref().hash(h); }
            Expr::TextEncoderEncodeInto { source, dest } => { tag(h, 12040); source.as_ref().hash(h); dest.as_ref().hash(h); }
            Expr::TextDecoderNew { label, fatal, ignore_bom } => { tag(h, 12250); label.as_ref().hash(h); fatal.as_ref().hash(h); ignore_bom.as_ref().hash(h); }
            Expr::TextDecoderDecode { decoder, input } => { tag(h, 12251); decoder.as_ref().hash(h); input.as_ref().hash(h); }
            Expr::TextDecoderEncoding(e) => { tag(h, 12252); e.as_ref().hash(h); }
            Expr::TextDecoderFatal(e) => { tag(h, 12253); e.as_ref().hash(h); }
            Expr::TextDecoderIgnoreBom(e) => { tag(h, 12254); e.as_ref().hash(h); }
            Expr::EncodeURI(e) => { tag(h, 183); e.as_ref().hash(h); }
            Expr::DecodeURI(e) => { tag(h, 184); e.as_ref().hash(h); }
            Expr::EncodeURIComponent(e) => { tag(h, 185); e.as_ref().hash(h); }
            Expr::DecodeURIComponent(e) => { tag(h, 186); e.as_ref().hash(h); }
            Expr::StructuredClone { value, options } => { tag(h, 187); value.as_ref().hash(h); options.as_ref().hash(h); }
            Expr::QueueMicrotask(e) => { tag(h, 188); e.as_ref().hash(h); }
            Expr::LinkGeneratorPrototype { obj, is_async } => { tag(h, 4141); obj.as_ref().hash(h); is_async.hash(h); }
            Expr::IterResultSet(e, b) => { tag(h, 189); e.as_ref().hash(h); b.hash(h); }
            Expr::IterResultGetValue => tag(h, 190),
            Expr::IterResultGetDone => tag(h, 191),
            Expr::AsyncStepChain { value, step_closure, } => { tag(h, 192); value.as_ref().hash(h); step_closure.as_ref().hash(h); }
            Expr::CryptoRandomBytes(e) => { tag(h, 193); e.as_ref().hash(h); }
            Expr::CryptoRandomUUID => tag(h, 194),
            Expr::CryptoRandomUUIDv7 => tag(h, 12042),
            Expr::CryptoSha256(e) => { tag(h, 195); e.as_ref().hash(h); }
            Expr::CryptoMd5(e) => { tag(h, 196); e.as_ref().hash(h); }
            Expr::WebCryptoDigest { algo, data } => { tag(h, 197); algo.as_ref().hash(h); data.as_ref().hash(h); }
            Expr::WebCryptoImportKey { format, key, algorithm, extractable, usages, } => { tag(h, 198); format.as_ref().hash(h); key.as_ref().hash(h); algorithm.as_ref().hash(h); extractable.as_ref().hash(h); usages.as_ref().hash(h); }
            Expr::WebCryptoExportKey { format, key, } => { tag(h, 472); format.as_ref().hash(h); key.as_ref().hash(h); }
            Expr::WebCryptoSign { algorithm, key, data, } => { tag(h, 199); algorithm.as_ref().hash(h); key.as_ref().hash(h); data.as_ref().hash(h); }
            Expr::WebCryptoVerify { algorithm, key, signature, data, } => { tag(h, 200); algorithm.as_ref().hash(h); key.as_ref().hash(h); signature.as_ref().hash(h); data.as_ref().hash(h); }
            Expr::WebCryptoDeriveBits { algorithm, base_key, length, } => { tag(h, 473); algorithm.as_ref().hash(h); base_key.as_ref().hash(h); length.as_ref().hash(h); }
            Expr::WebCryptoDeriveKey { algorithm, base_key, derived_key_algorithm, extractable, usages, } => { tag(h, 12010); algorithm.as_ref().hash(h); base_key.as_ref().hash(h); derived_key_algorithm.as_ref().hash(h); extractable.as_ref().hash(h); usages.as_ref().hash(h); }
            Expr::WebCryptoEncrypt { algorithm, key, data, } => { tag(h, 466); algorithm.as_ref().hash(h); key.as_ref().hash(h); data.as_ref().hash(h); }
            Expr::WebCryptoDecrypt { algorithm, key, data, } => { tag(h, 467); algorithm.as_ref().hash(h); key.as_ref().hash(h); data.as_ref().hash(h); }
            Expr::WebCryptoGenerateKey { algorithm, extractable, usages, } => { tag(h, 469); algorithm.as_ref().hash(h); extractable.as_ref().hash(h); usages.as_ref().hash(h); }
            Expr::WebCryptoWrapKey { format, key, wrapping_key, wrap_algorithm, } => { tag(h, 470); format.as_ref().hash(h); key.as_ref().hash(h); wrapping_key.as_ref().hash(h); wrap_algorithm.as_ref().hash(h); }
            Expr::WebCryptoUnwrapKey { format, wrapped_key, unwrapping_key, unwrap_algorithm, unwrapped_key_algorithm, extractable, usages, } => { tag(h, 471); format.as_ref().hash(h); wrapped_key.as_ref().hash(h); unwrapping_key.as_ref().hash(h); unwrap_algorithm.as_ref().hash(h); unwrapped_key_algorithm.as_ref().hash(h); extractable.as_ref().hash(h); usages.as_ref().hash(h); }
            Expr::CryptoRandomFillSync { buffer, offset, size, } => { tag(h, 468); buffer.as_ref().hash(h); offset.as_ref().hash(h); size.as_ref().hash(h); }
            Expr::OsPlatform => tag(h, 201),
            Expr::OsArch => tag(h, 202),
            Expr::OsHostname => tag(h, 203),
            Expr::OsHomedir => tag(h, 204),
            Expr::OsTmpdir => tag(h, 205),
            Expr::OsTotalmem => tag(h, 206),
            Expr::OsFreemem => tag(h, 207),
            Expr::OsUptime => tag(h, 208),
            Expr::OsType => tag(h, 209),
            Expr::OsRelease => tag(h, 210),
            Expr::OsCpus => tag(h, 211),
            Expr::OsNetworkInterfaces => tag(h, 212),
            Expr::OsUserInfo => tag(h, 213),
            Expr::OsUserInfoBuffer => tag(h, 11221),
            Expr::OsEOL => tag(h, 214),
            Expr::OsDevNull => tag(h, 215),
            Expr::OsAvailableParallelism => tag(h, 216),
            Expr::OsEndianness => tag(h, 217),
            Expr::OsLoadavg => tag(h, 218),
            Expr::OsMachine => tag(h, 219),
            Expr::OsVersion => tag(h, 220),
            Expr::BufferFrom { data, encoding } => { tag(h, 12011); data.as_ref().hash(h); encoding.hash(h); }
            Expr::BufferFromArrayBuffer { data, byte_offset, length, } => { tag(h, 11220); data.as_ref().hash(h); byte_offset.as_ref().hash(h); length.hash(h); }
            Expr::BufferAlloc { size, fill, encoding } => { tag(h, 12012); size.as_ref().hash(h); fill.hash(h); encoding.hash(h); }
            Expr::BufferAllocUnsafe(e) => { tag(h, 12013); e.as_ref().hash(h); }
            Expr::BufferConcat(e) => { tag(h, 12014); e.as_ref().hash(h); }
            Expr::BufferConcatWithLength { list, total_length } => { tag(h, 12035); list.as_ref().hash(h); total_length.as_ref().hash(h); }
            Expr::BufferIsBuffer(e) => { tag(h, 12015); e.as_ref().hash(h); }
            Expr::BufferIsEncoding(e) => { tag(h, 11219); e.as_ref().hash(h); }
            Expr::BufferByteLength { data, encoding } => { tag(h, 12016); data.as_ref().hash(h); encoding.hash(h); }
            Expr::BufferToString { buffer, encoding } => { tag(h, 221); buffer.as_ref().hash(h); encoding.hash(h); }
            Expr::BufferLength(e) => { tag(h, 222); e.as_ref().hash(h); }
            Expr::BufferSlice { buffer, start, end } => { tag(h, 223); buffer.as_ref().hash(h); start.hash(h); end.hash(h); }
            Expr::BufferCopy { source, target, target_start, source_start, source_end, } => { tag(h, 224); source.as_ref().hash(h); target.as_ref().hash(h); target_start.hash(h); source_start.hash(h); source_end.hash(h); }
            Expr::BufferWrite { buffer, string, offset, encoding, } => { tag(h, 225); buffer.as_ref().hash(h); string.as_ref().hash(h); offset.hash(h); encoding.hash(h); }
            Expr::BufferFill { buffer, value } => { tag(h, 226); buffer.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::BufferEquals { buffer, other } => { tag(h, 227); buffer.as_ref().hash(h); other.as_ref().hash(h); }
            Expr::BufferIndexGet { buffer, index } => { tag(h, 228); buffer.as_ref().hash(h); index.as_ref().hash(h); }
            Expr::BufferIndexSet { buffer, index, value, } => { tag(h, 229); buffer.as_ref().hash(h); index.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::Uint8ArrayNew(e) => { tag(h, 230); e.hash(h); }
            Expr::Uint8ArrayFrom(e) => { tag(h, 231); e.as_ref().hash(h); }
            Expr::Uint8ArrayLength(e) => { tag(h, 232); e.as_ref().hash(h); }
            Expr::Uint8ArrayGet { array, index } => { tag(h, 233); array.as_ref().hash(h); index.as_ref().hash(h); }
            Expr::Uint8ArraySet { array, index, value, } => { tag(h, 234); array.as_ref().hash(h); index.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::TypedArrayNew { kind, arg } => { tag(h, 235); kind.hash(h); arg.hash(h); }
            Expr::NativeArenaAlloc(e) => { tag(h, 12031); e.as_ref().hash(h); }
            Expr::NativeArenaView { owner, kind, byte_offset, length } => { tag(h, 12032); owner.as_ref().hash(h); kind.hash(h); byte_offset.as_ref().hash(h); length.as_ref().hash(h); }
            Expr::NativePodView { owner, byte_offset, count, view_type } => { tag(h, 12037); owner.as_ref().hash(h); byte_offset.as_ref().hash(h); count.as_ref().hash(h); view_type.hash(h); }
            Expr::NativeArenaDispose(e) => { tag(h, 12038); e.as_ref().hash(h); }
            Expr::NativeMemoryFillU32 { view, value } => { tag(h, 12034); view.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::NativeMemoryCopy { dst, src } => { tag(h, 12039); dst.as_ref().hash(h); src.as_ref().hash(h); }
            Expr::ChildProcessExecSync { command, options } => { tag(h, 236); command.as_ref().hash(h); options.hash(h); }
            Expr::ChildProcessSpawnSync { command, args, options, } => { tag(h, 237); command.as_ref().hash(h); args.hash(h); options.hash(h); }
            Expr::ChildProcessSpawn { command, args, options, } => { tag(h, 238); command.as_ref().hash(h); args.hash(h); options.hash(h); }
            Expr::ChildProcessFork { module, args, options, } => { tag(h, 11240); module.as_ref().hash(h); args.hash(h); options.hash(h); }
            Expr::ChildProcessExec { command, options, callback, } => { tag(h, 239); command.as_ref().hash(h); options.hash(h); callback.hash(h); }
            Expr::ChildProcessExecFile { file, args, options, callback, } => { tag(h, 11239); file.as_ref().hash(h); args.hash(h); options.hash(h); callback.hash(h); }
            Expr::ChildProcessExecFileSync { file, args, options, } => { tag(h, 12033); file.as_ref().hash(h); args.hash(h); options.hash(h); }
            Expr::ChildProcessSpawnBackground { command, args, log_file, env_json, } => { tag(h, 240); command.as_ref().hash(h); args.hash(h); log_file.as_ref().hash(h); env_json.hash(h); }
            Expr::ChildProcessGetProcessStatus(e) => { tag(h, 241); e.as_ref().hash(h); }
            Expr::ChildProcessKillProcess(e) => { tag(h, 242); e.as_ref().hash(h); }
            Expr::FetchWithOptions { url, method, body, headers, headers_dynamic, signal, } => { tag(h, 243); url.as_ref().hash(h); method.as_ref().hash(h); body.as_ref().hash(h); headers.hash(h); if let Some(hd) = headers_dynamic { tag(h, 1); hd.as_ref().hash(h); } else { tag(h, 0); } if let Some(s) = signal { tag(h, 1); s.as_ref().hash(h); } else { tag(h, 0); } }
            Expr::FetchGetWithAuth { url, auth_header } => { tag(h, 244); url.as_ref().hash(h); auth_header.as_ref().hash(h); }
            Expr::FetchPostWithAuth { url, auth_header, body, } => { tag(h, 245); url.as_ref().hash(h); auth_header.as_ref().hash(h); body.as_ref().hash(h); }
            Expr::NetCreateServer { options, connection_listener, } => { tag(h, 246); options.hash(h); connection_listener.hash(h); }
            Expr::NetCreateConnection { port, host, connect_listener, } => { tag(h, 247); port.as_ref().hash(h); host.hash(h); connect_listener.hash(h); }
            Expr::NetConnect { port, host, connect_listener, } => { tag(h, 248); port.as_ref().hash(h); host.hash(h); connect_listener.hash(h); }
            Expr::ArrayPush { array_id, value } => { tag(h, 249); array_id.hash(h); value.as_ref().hash(h); }
            Expr::ArrayPushSpread { array_id, source } => { tag(h, 250); array_id.hash(h); source.as_ref().hash(h); }
            Expr::ArrayPop(id) => { tag(h, 251); id.hash(h); }
            Expr::ArrayShift(id) => { tag(h, 252); id.hash(h); }
            Expr::ArrayUnshift { array_id, value } => { tag(h, 253); array_id.hash(h); value.as_ref().hash(h); }
            Expr::ArrayIndexOf { array, value, from_index } => { tag(h, 254); array.as_ref().hash(h); value.as_ref().hash(h); if let Some(fi) = from_index { tag(h, 1); fi.as_ref().hash(h); } else { tag(h, 0); } }
            Expr::ArrayLastIndexOf { array, value, from_index } => { tag(h, 11244); array.as_ref().hash(h); value.as_ref().hash(h); if let Some(fi) = from_index { tag(h, 1); fi.as_ref().hash(h); } else { tag(h, 0); } }
            Expr::ArrayIncludes { array, value, from_index } => { tag(h, 255); array.as_ref().hash(h); value.as_ref().hash(h); if let Some(fi) = from_index { tag(h, 1); fi.as_ref().hash(h); } else { tag(h, 0); } }
            Expr::ArraySlice { array, start, end } => { tag(h, 256); array.as_ref().hash(h); start.as_ref().hash(h); end.hash(h); }
            Expr::ArraySplice { array_id, start, delete_count, items, } => { tag(h, 257); array_id.hash(h); start.as_ref().hash(h); delete_count.hash(h); items.hash(h); }
            Expr::ArrayForEach { array, callback } => { tag(h, 258); array.as_ref().hash(h); callback.as_ref().hash(h); }
            Expr::ArrayMap { array, callback } => { tag(h, 259); array.as_ref().hash(h); callback.as_ref().hash(h); }
            Expr::ArrayFilter { array, callback } => { tag(h, 260); array.as_ref().hash(h); callback.as_ref().hash(h); }
            Expr::ArrayFind { array, callback } => { tag(h, 261); array.as_ref().hash(h); callback.as_ref().hash(h); }
            Expr::ArrayFindIndex { array, callback } => { tag(h, 262); array.as_ref().hash(h); callback.as_ref().hash(h); }
            Expr::ArrayFindLast { array, callback } => { tag(h, 263); array.as_ref().hash(h); callback.as_ref().hash(h); }
            Expr::ArrayFindLastIndex { array, callback } => { tag(h, 264); array.as_ref().hash(h); callback.as_ref().hash(h); }
            Expr::ArrayAt { array, index } => { tag(h, 265); array.as_ref().hash(h); index.as_ref().hash(h); }
            Expr::ArraySome { array, callback } => { tag(h, 266); array.as_ref().hash(h); callback.as_ref().hash(h); }
            Expr::ArrayEvery { array, callback } => { tag(h, 267); array.as_ref().hash(h); callback.as_ref().hash(h); }
            Expr::ArrayFlatMap { array, callback } => { tag(h, 268); array.as_ref().hash(h); callback.as_ref().hash(h); }
            Expr::ArraySort { array, comparator } => { tag(h, 269); array.as_ref().hash(h); comparator.as_ref().hash(h); }
            Expr::ArrayReduce { array, callback, initial, } => { tag(h, 270); array.as_ref().hash(h); callback.as_ref().hash(h); initial.hash(h); }
            Expr::ArrayReduceRight { array, callback, initial, } => { tag(h, 271); array.as_ref().hash(h); callback.as_ref().hash(h); initial.hash(h); }
            Expr::ArrayJoin { array, separator } => { tag(h, 272); array.as_ref().hash(h); separator.hash(h); }
            Expr::ArrayFlat { array } => { tag(h, 273); array.as_ref().hash(h); }
            Expr::ArrayToReversed { array } => { tag(h, 274); array.as_ref().hash(h); }
            Expr::ArrayToSorted { array, comparator } => { tag(h, 275); array.as_ref().hash(h); comparator.hash(h); }
            Expr::ArrayToSpliced { array, start, delete_count, items, } => { tag(h, 276); array.as_ref().hash(h); start.as_ref().hash(h); delete_count.as_ref().hash(h); items.hash(h); }
            Expr::ArrayWith { array, index, value, } => { tag(h, 277); array.as_ref().hash(h); index.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::ArrayReverseValue { receiver } => { tag(h, 12092); receiver.as_ref().hash(h); }
            Expr::ArrayCopyWithin { array_id, target, start, end, } => { tag(h, 278); array_id.hash(h); target.as_ref().hash(h); start.as_ref().hash(h); end.hash(h); }
            Expr::ArrayCopyWithinValue { receiver, target, start, end, } => { tag(h, 12091); receiver.as_ref().hash(h); target.as_ref().hash(h); start.as_ref().hash(h); end.hash(h); }
            Expr::ArrayEntries(e) => { tag(h, 279); e.as_ref().hash(h); }
            Expr::ArrayKeys(e) => { tag(h, 280); e.as_ref().hash(h); }
            Expr::ArrayValues(e) => { tag(h, 281); e.as_ref().hash(h); }
            Expr::ArrayLikeMethod { method, receiver, args } => { tag(h, 12500); method.hash(h); receiver.as_ref().hash(h); args.hash(h); }
            Expr::StringSplit(a, b) => { tag(h, 282); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::StringFromCharCode(e) => { tag(h, 283); e.as_ref().hash(h); }
            Expr::StringFromCharCodeSpread(e) => { tag(h, 12045); e.as_ref().hash(h); }
            Expr::StringFromCodePoint(e) => { tag(h, 284); e.as_ref().hash(h); }
            Expr::StringRaw { call_site, substitutions } => { tag(h, 12047); call_site.as_ref().hash(h); substitutions.hash(h); }
            Expr::StringAt { string, index } => { tag(h, 285); string.as_ref().hash(h); index.as_ref().hash(h); }
            Expr::StringCodePointAt { string, index } => { tag(h, 286); string.as_ref().hash(h); index.as_ref().hash(h); }
            Expr::MapNew => tag(h, 287),
            Expr::MapNewFromArray(e) => { tag(h, 288); e.as_ref().hash(h); }
            Expr::MapSet { map, key, value } => { tag(h, 289); map.as_ref().hash(h); key.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::MapGet { map, key } => { tag(h, 290); map.as_ref().hash(h); key.as_ref().hash(h); }
            Expr::MapHas { map, key } => { tag(h, 291); map.as_ref().hash(h); key.as_ref().hash(h); }
            Expr::MapDelete { map, key } => { tag(h, 292); map.as_ref().hash(h); key.as_ref().hash(h); }
            Expr::MapSize(e) => { tag(h, 293); e.as_ref().hash(h); }
            Expr::MapClear(e) => { tag(h, 294); e.as_ref().hash(h); }
            Expr::MapEntries(e) => { tag(h, 295); e.as_ref().hash(h); }
            Expr::MapKeys(e) => { tag(h, 296); e.as_ref().hash(h); }
            Expr::MapValues(e) => { tag(h, 297); e.as_ref().hash(h); }
            Expr::MapEntryKeyAt { map, idx } => { tag(h, 298); map.as_ref().hash(h); idx.as_ref().hash(h); }
            Expr::MapEntryValueAt { map, idx } => { tag(h, 299); map.as_ref().hash(h); idx.as_ref().hash(h); }
            Expr::SetNew => tag(h, 300),
            Expr::SetNewFromArray(e) => { tag(h, 301); e.as_ref().hash(h); }
            Expr::SetAdd { set_id, value } => { tag(h, 302); set_id.hash(h); value.as_ref().hash(h); }
            Expr::SetHas { set, value } => { tag(h, 303); set.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::SetDelete { set, value } => { tag(h, 304); set.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::SetSize(e) => { tag(h, 305); e.as_ref().hash(h); }
            Expr::SetClear(e) => { tag(h, 306); e.as_ref().hash(h); }
            Expr::SetValues(e) => { tag(h, 307); e.as_ref().hash(h); }
            Expr::SetValueAt { set, idx } => { tag(h, 308); set.as_ref().hash(h); idx.as_ref().hash(h); }
            Expr::Sequence(es) => { tag(h, 309); es.hash(h); }
            Expr::DateNow => tag(h, 310),
            Expr::DateNew(es) => { tag(h, 311); es.hash(h); }
            Expr::BoxedPrimitiveNew {
                kind,
                arg,
                arg_present,
            } => {
                tag(h, 12017);
                (*kind as u8).hash(h);
                arg.as_ref().hash(h);
                arg_present.hash(h);
            }
            Expr::DateGetTime(e) => { tag(h, 312); e.as_ref().hash(h); }
            Expr::DateToISOString(e) => { tag(h, 313); e.as_ref().hash(h); }
            Expr::DateGetFullYear(e) => { tag(h, 314); e.as_ref().hash(h); }
            Expr::DateGetMonth(e) => { tag(h, 315); e.as_ref().hash(h); }
            Expr::DateGetDate(e) => { tag(h, 316); e.as_ref().hash(h); }
            Expr::DateGetDay(e) => { tag(h, 12018); e.as_ref().hash(h); }
            Expr::DateGetHours(e) => { tag(h, 317); e.as_ref().hash(h); }
            Expr::DateGetMinutes(e) => { tag(h, 318); e.as_ref().hash(h); }
            Expr::DateGetSeconds(e) => { tag(h, 319); e.as_ref().hash(h); }
            Expr::DateGetMilliseconds(e) => { tag(h, 320); e.as_ref().hash(h); }
            Expr::DateParse(e) => { tag(h, 321); e.as_ref().hash(h); }
            Expr::DateUtc(es) => { tag(h, 322); es.hash(h); }
            Expr::DateGetUtcDay(e) => { tag(h, 323); e.as_ref().hash(h); }
            Expr::DateGetUtcFullYear(e) => { tag(h, 324); e.as_ref().hash(h); }
            Expr::DateGetUtcMonth(e) => { tag(h, 325); e.as_ref().hash(h); }
            Expr::DateGetUtcDate(e) => { tag(h, 326); e.as_ref().hash(h); }
            Expr::DateGetUtcHours(e) => { tag(h, 327); e.as_ref().hash(h); }
            Expr::DateGetUtcMinutes(e) => { tag(h, 328); e.as_ref().hash(h); }
            Expr::DateGetUtcSeconds(e) => { tag(h, 329); e.as_ref().hash(h); }
            Expr::DateGetUtcMilliseconds(e) => { tag(h, 330); e.as_ref().hash(h); }
            Expr::DateSetUtcFullYear { date, args } => { tag(h, 331); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetUtcMonth { date, args } => { tag(h, 332); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetUtcDate { date, args } => { tag(h, 333); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetUtcHours { date, args } => { tag(h, 334); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetUtcMinutes { date, args } => { tag(h, 335); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetUtcSeconds { date, args } => { tag(h, 336); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetUtcMilliseconds { date, args } => { tag(h, 337); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetFullYear { date, args } => { tag(h, 478); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetMonth { date, args } => { tag(h, 479); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetDate { date, args } => { tag(h, 480); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetHours { date, args } => { tag(h, 481); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetMinutes { date, args } => { tag(h, 482); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetSeconds { date, args } => { tag(h, 483); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetMilliseconds { date, args } => { tag(h, 484); date.as_ref().hash(h); args.hash(h); }
            Expr::DateSetTime { date, args } => { tag(h, 485); date.as_ref().hash(h); args.hash(h); }
            Expr::DateValueOf(e) => { tag(h, 338); e.as_ref().hash(h); }
            Expr::DateToString(e) => { tag(h, 1339); e.as_ref().hash(h); }
            Expr::DateToDateString(e) => { tag(h, 339); e.as_ref().hash(h); }
            Expr::DateToTimeString(e) => { tag(h, 340); e.as_ref().hash(h); }
            Expr::DateToUTCString(e) => { tag(h, 12508); e.as_ref().hash(h); }
            Expr::DateToLocaleDateString(e) => { tag(h, 341); e.as_ref().hash(h); }
            Expr::DateToLocaleTimeString(e) => { tag(h, 342); e.as_ref().hash(h); }
            Expr::DateToLocaleString(e) => { tag(h, 343); e.as_ref().hash(h); }
            Expr::DateGetTimezoneOffset(e) => { tag(h, 344); e.as_ref().hash(h); }
            Expr::DateToJSON(e) => { tag(h, 345); e.as_ref().hash(h); }
            Expr::ErrorNew(e) => { tag(h, 346); e.hash(h); }
            Expr::ErrorMessage(e) => { tag(h, 347); e.as_ref().hash(h); }
            Expr::ErrorNewWithCause { message, cause } => { tag(h, 348); message.as_ref().hash(h); cause.as_ref().hash(h); }
            Expr::ErrorNewWithOptions { kind, message, options } => { tag(h, 12060); kind.hash(h); message.as_ref().hash(h); options.as_ref().hash(h); }
            Expr::TypeErrorNew(e) => { tag(h, 349); e.as_ref().hash(h); }
            Expr::RangeErrorNew(e) => { tag(h, 350); e.as_ref().hash(h); }
            Expr::ReferenceErrorNew(e) => { tag(h, 351); e.as_ref().hash(h); }
            Expr::SyntaxErrorNew(e) => { tag(h, 352); e.as_ref().hash(h); }
            Expr::AggregateErrorNew { errors, message, options } => { tag(h, 353); errors.as_ref().hash(h); message.as_ref().hash(h); options.hash(h); }
            Expr::UrlNew { url, base } => { tag(h, 354); url.as_ref().hash(h); base.hash(h); }
            Expr::UrlPatternNew { input, base } => { tag(h, 12061); input.as_ref().hash(h); base.hash(h); }
            Expr::UrlGetHref(e) => { tag(h, 355); e.as_ref().hash(h); }
            Expr::UrlGetPathname(e) => { tag(h, 356); e.as_ref().hash(h); }
            Expr::UrlGetProtocol(e) => { tag(h, 357); e.as_ref().hash(h); }
            Expr::UrlGetHost(e) => { tag(h, 358); e.as_ref().hash(h); }
            Expr::UrlGetHostname(e) => { tag(h, 359); e.as_ref().hash(h); }
            Expr::UrlGetPort(e) => { tag(h, 360); e.as_ref().hash(h); }
            Expr::UrlGetSearch(e) => { tag(h, 361); e.as_ref().hash(h); }
            Expr::UrlGetHash(e) => { tag(h, 362); e.as_ref().hash(h); }
            Expr::UrlGetOrigin(e) => { tag(h, 363); e.as_ref().hash(h); }
            Expr::UrlGetSearchParams(e) => { tag(h, 364); e.as_ref().hash(h); }
            Expr::UrlCanParse(e) => { tag(h, 365); e.as_ref().hash(h); }
            Expr::UrlCanParseWithBase { input, base } => { tag(h, 700); input.as_ref().hash(h); base.as_ref().hash(h); }
            Expr::UrlParse(e) => { tag(h, 366); e.as_ref().hash(h); }
            Expr::UrlParseWithBase { input, base } => { tag(h, 705); input.as_ref().hash(h); base.as_ref().hash(h); }
            Expr::UrlInstanceToString(e) => { tag(h, 367); e.as_ref().hash(h); }
            Expr::UrlInstanceToJSON(e) => { tag(h, 368); e.as_ref().hash(h); }
            Expr::UrlSetPathname { url, value } => { tag(h, 369); url.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::UrlSetSearch { url, value } => { tag(h, 370); url.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::UrlSetHash { url, value } => { tag(h, 371); url.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::UrlSetProtocol { url, value } => { tag(h, 465); url.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::UrlSetHostname { url, value } => { tag(h, 12019); url.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::UrlSetPort { url, value } => { tag(h, 12020); url.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::UrlSetUsername { url, value } => { tag(h, 12021); url.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::UrlSetPassword { url, value } => { tag(h, 12022); url.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::UrlSetHref { url, value } => { tag(h, 12036); url.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::UrlSearchParamsNew(e) => { tag(h, 372); e.hash(h); }
            Expr::UrlSearchParamsMissingArgs { params, args, name_and_value } => { tag(h, 12049); params.as_ref().hash(h); args.hash(h); name_and_value.hash(h); }
            Expr::UrlSearchParamsGet { params, name } => { tag(h, 373); params.as_ref().hash(h); name.as_ref().hash(h); }
            Expr::UrlSearchParamsHas { params, name, value, } => { tag(h, 374); params.as_ref().hash(h); name.as_ref().hash(h); match value { Some(v) => { tag(h, 1); v.as_ref().hash(h); } None => tag(h, 0), } }
            Expr::UrlSearchParamsSet { params, name, value, } => { tag(h, 375); params.as_ref().hash(h); name.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::UrlSearchParamsAppend { params, name, value, } => { tag(h, 376); params.as_ref().hash(h); name.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::UrlSearchParamsDelete { params, name, value, } => { tag(h, 377); params.as_ref().hash(h); name.as_ref().hash(h); match value { Some(v) => { tag(h, 1); v.as_ref().hash(h); } None => tag(h, 0), } }
            Expr::UrlSearchParamsToString(e) => { tag(h, 378); e.as_ref().hash(h); }
            Expr::UrlSearchParamsGetAll { params, name } => { tag(h, 379); params.as_ref().hash(h); name.as_ref().hash(h); }
            Expr::UrlSearchParamsEntries(e) => { tag(h, 380); e.as_ref().hash(h); }
            Expr::UrlSearchParamsKeys(e) => { tag(h, 701); e.as_ref().hash(h); }
            Expr::UrlSearchParamsValues(e) => { tag(h, 702); e.as_ref().hash(h); }
            Expr::UrlSearchParamsSort(e) => { tag(h, 703); e.as_ref().hash(h); }
            Expr::UrlSearchParamsForEach { params, callback, this_arg } => { tag(h, 704); params.as_ref().hash(h); callback.as_ref().hash(h); match this_arg { Some(v) => { tag(h, 1); v.as_ref().hash(h); } None => tag(h, 0), } }
            Expr::Delete(e) => { tag(h, 381); e.as_ref().hash(h); }
            Expr::NewTarget => { tag(h, 12271); }
            Expr::Closure { func_id, params, return_type, body, captures, mutable_captures, captures_this, captures_new_target, enclosing_class, is_arrow, is_async, is_generator, is_strict, } => { tag(h, 382); func_id.hash(h); params.hash(h); return_type.hash(h); body.hash(h); captures.hash(h); mutable_captures.hash(h); captures_this.hash(h); captures_new_target.hash(h); enclosing_class.hash(h); is_arrow.hash(h); is_async.hash(h); is_generator.hash(h); is_strict.hash(h); }
            Expr::RegExp { pattern, flags } => { tag(h, 383); pattern.hash(h); flags.hash(h); }
            Expr::RegExpDynamic { pattern, flags, is_call } => { tag(h, 475); pattern.as_ref().hash(h); if let Some(f_box) = flags { tag(h, 476); f_box.as_ref().hash(h); } else { tag(h, 477); } tag(h, if *is_call { 478 } else { 479 }); }
            Expr::RegExpTest { regex, string } => { tag(h, 384); regex.as_ref().hash(h); string.as_ref().hash(h); }
            Expr::StringMatch { string, regex } => { tag(h, 385); string.as_ref().hash(h); regex.as_ref().hash(h); }
            Expr::StringMatchAll { string, regex } => { tag(h, 386); string.as_ref().hash(h); regex.as_ref().hash(h); }
            Expr::StringReplace { string, pattern, replacement, } => { tag(h, 387); string.as_ref().hash(h); pattern.as_ref().hash(h); replacement.as_ref().hash(h); }
            Expr::ObjectFromEntries(e) => { tag(h, 388); e.as_ref().hash(h); }
            Expr::ObjectIs(a, b) => { tag(h, 389); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::ObjectHasOwn(a, b) => { tag(h, 390); a.as_ref().hash(h); b.as_ref().hash(h); }
            Expr::ObjectKeys(e) => { tag(h, 391); e.as_ref().hash(h); }
            Expr::ForInKeys(e) => { tag(h, 12506); e.as_ref().hash(h); }
            Expr::ObjectValues(e) => { tag(h, 392); e.as_ref().hash(h); }
            Expr::ObjectEntries(e) => { tag(h, 393); e.as_ref().hash(h); }
            Expr::ObjectGroupBy { items, key_fn } => { tag(h, 394); items.as_ref().hash(h); key_fn.as_ref().hash(h); }
            Expr::MapGroupBy { items, key_fn } => { tag(h, 12044); items.as_ref().hash(h); key_fn.as_ref().hash(h); }
            Expr::ObjectRest { object, exclude_keys, } => { tag(h, 395); object.as_ref().hash(h); exclude_keys.hash(h); }
            Expr::ArrayIsArray(e) => { tag(h, 396); e.as_ref().hash(h); }
            Expr::ArrayFrom(e) => { tag(h, 397); e.as_ref().hash(h); }
            Expr::ArrayFromArrayLikeHoley(e) => { tag(h, 12320); e.as_ref().hash(h); }
            Expr::IteratorFrom(e) => { tag(h, 12270); e.as_ref().hash(h); }
            Expr::IteratorToArray(e) => { tag(h, 398); e.as_ref().hash(h); }
            Expr::GetIterator(e) => { tag(h, 11238); e.as_ref().hash(h); }
            Expr::GetAsyncIterator(e) => { tag(h, 12502); e.as_ref().hash(h); }
            Expr::ForOfToArray(e) => { tag(h, 11243); e.as_ref().hash(h); }
            Expr::ForAwaitToArray(e) => { tag(h, 12501); e.as_ref().hash(h); }
            Expr::ArrayFromMapped { iterable, map_fn, this_arg } => { tag(h, 399); iterable.as_ref().hash(h); map_fn.as_ref().hash(h); this_arg.is_some().hash(h); if let Some(t) = this_arg { t.as_ref().hash(h); } }
            Expr::ParseInt { string, radix } => { tag(h, 400); string.as_ref().hash(h); radix.hash(h); }
            Expr::ParseFloat(e) => { tag(h, 401); e.as_ref().hash(h); }
            Expr::NumberCoerce(e) => { tag(h, 402); e.as_ref().hash(h); }
            Expr::BigIntCoerce(e) => { tag(h, 403); e.as_ref().hash(h); }
            Expr::StringCoerce(e) => { tag(h, 404); e.as_ref().hash(h); }
            Expr::ObjectCoerce(e) => { tag(h, 906); e.as_ref().hash(h); }
            Expr::BooleanCoerce(e) => { tag(h, 405); e.as_ref().hash(h); }
            Expr::IsNaN(e) => { tag(h, 406); e.as_ref().hash(h); }
            Expr::IsUndefinedOrBareNan(e) => { tag(h, 407); e.as_ref().hash(h); }
            Expr::IsFinite(e) => { tag(h, 408); e.as_ref().hash(h); }
            Expr::NumberIsNaN(e) => { tag(h, 409); e.as_ref().hash(h); }
            Expr::NumberIsFinite(e) => { tag(h, 410); e.as_ref().hash(h); }
            Expr::NumberIsInteger(e) => { tag(h, 411); e.as_ref().hash(h); }
            Expr::NumberIsSafeInteger(e) => { tag(h, 412); e.as_ref().hash(h); }
            Expr::StaticPluginResolve(e) => { tag(h, 413); e.as_ref().hash(h); }
            Expr::JsLoadModule { path } => { tag(h, 414); path.hash(h); }
            Expr::JsGetExport { module_handle, export_name, } => { tag(h, 415); module_handle.as_ref().hash(h); export_name.hash(h); }
            Expr::JsCallFunction { module_handle, func_name, args, } => { tag(h, 416); module_handle.as_ref().hash(h); func_name.hash(h); args.hash(h); }
            Expr::JsCallMethod { object, method_name, args, } => { tag(h, 417); object.as_ref().hash(h); method_name.hash(h); args.hash(h); }
            Expr::JsCallValue { callee, args } => { tag(h, 452); callee.as_ref().hash(h); args.hash(h); }
            Expr::JsGetProperty { object, property_name, } => { tag(h, 418); object.as_ref().hash(h); property_name.hash(h); }
            Expr::JsSetProperty { object, property_name, value, } => { tag(h, 419); object.as_ref().hash(h); property_name.hash(h); value.as_ref().hash(h); }
            Expr::JsNew { module_handle, class_name, args, } => { tag(h, 420); module_handle.as_ref().hash(h); class_name.hash(h); args.hash(h); }
            Expr::JsNewFromHandle { constructor, args } => { tag(h, 421); constructor.as_ref().hash(h); args.hash(h); }
            Expr::JsCreateCallback { closure, param_count, } => { tag(h, 422); closure.as_ref().hash(h); param_count.hash(h); }
            Expr::ImportMetaUrl(s) => { tag(h, 423); s.hash(h); }
            Expr::ProxyNew { target, handler } => { tag(h, 424); target.as_ref().hash(h); handler.as_ref().hash(h); }
            Expr::ProxyGet { proxy, key } => { tag(h, 425); proxy.as_ref().hash(h); key.as_ref().hash(h); }
            Expr::ProxySet { proxy, key, value } => { tag(h, 426); proxy.as_ref().hash(h); key.as_ref().hash(h); value.as_ref().hash(h); }
            Expr::ProxyHas { proxy, key } => { tag(h, 427); proxy.as_ref().hash(h); key.as_ref().hash(h); }
            Expr::ProxyDelete { proxy, key } => { tag(h, 428); proxy.as_ref().hash(h); key.as_ref().hash(h); }
            Expr::ProxyApply { proxy, args } => { tag(h, 429); proxy.as_ref().hash(h); args.hash(h); }
            Expr::ProxyConstruct { proxy, args } => { tag(h, 430); proxy.as_ref().hash(h); args.hash(h); }
            Expr::ProxyRevocable { target, handler } => { tag(h, 431); target.as_ref().hash(h); handler.as_ref().hash(h); }
            Expr::ProxyRevoke(e) => { tag(h, 432); e.as_ref().hash(h); }
            Expr::ReflectGet { target, key, receiver } => { tag(h, 433); target.as_ref().hash(h); key.as_ref().hash(h); receiver.as_ref().hash(h); }
            Expr::ReflectSet { target, key, value, receiver } => { tag(h, 434); target.as_ref().hash(h); key.as_ref().hash(h); value.as_ref().hash(h); receiver.as_ref().hash(h); }
            Expr::PutValueSet { target, key, value, receiver, strict } => { tag(h, 12235); target.as_ref().hash(h); key.as_ref().hash(h); value.as_ref().hash(h); receiver.as_ref().hash(h); strict.hash(h); }
            Expr::WithGet { object, property, fallback } => { tag(h, 12236); object.as_ref().hash(h); property.hash(h); fallback.as_ref().hash(h); }
            Expr::WithSet { object, property, value, fallback, strict } => { tag(h, 12237); object.as_ref().hash(h); property.hash(h); value.as_ref().hash(h); hash_with_set_fallback(h, fallback); strict.hash(h); }
            Expr::ReflectHas { target, key } => { tag(h, 435); target.as_ref().hash(h); key.as_ref().hash(h); }
            Expr::ReflectDelete { target, key } => { tag(h, 436); target.as_ref().hash(h); key.as_ref().hash(h); }
            Expr::ReflectOwnKeys(e) => { tag(h, 437); e.as_ref().hash(h); }
            Expr::ReflectApply { func, this_arg, args, } => { tag(h, 438); func.as_ref().hash(h); this_arg.as_ref().hash(h); args.as_ref().hash(h); }
            Expr::ReflectConstruct { target, args, new_target } => { tag(h, 439); target.as_ref().hash(h); args.as_ref().hash(h); new_target.as_ref().hash(h); }
            Expr::ReflectDefineProperty { target, key, descriptor, } => { tag(h, 440); target.as_ref().hash(h); key.as_ref().hash(h); descriptor.as_ref().hash(h); }
            Expr::ReflectGetOwnPropertyDescriptor { target, key } => { tag(h, 12505); target.as_ref().hash(h); key.as_ref().hash(h); }
            Expr::ReflectGetPrototypeOf(e) => { tag(h, 441); e.as_ref().hash(h); }
            Expr::ReflectSetPrototypeOf { target, proto } => { tag(h, 12230); target.as_ref().hash(h); proto.as_ref().hash(h); }
            Expr::ReflectIsExtensible(e) => { tag(h, 12048); e.as_ref().hash(h); }
            Expr::ReflectPreventExtensions(e) => { tag(h, 12046); e.as_ref().hash(h); }
            Expr::ReflectDefineMetadata { key, value, target, property_key, } => { tag(h, 12023); key.as_ref().hash(h); value.as_ref().hash(h); target.as_ref().hash(h); property_key.hash(h); }
            Expr::ReflectGetMetadata { key, target, property_key, } => { tag(h, 12024); key.as_ref().hash(h); target.as_ref().hash(h); property_key.hash(h); }
            Expr::ReflectGetOwnMetadata { key, target, property_key, } => { tag(h, 455); key.as_ref().hash(h); target.as_ref().hash(h); property_key.hash(h); }
            Expr::ReflectHasMetadata { key, target, property_key, } => { tag(h, 456); key.as_ref().hash(h); target.as_ref().hash(h); property_key.hash(h); }
            Expr::ReflectHasOwnMetadata { key, target, property_key, } => { tag(h, 457); key.as_ref().hash(h); target.as_ref().hash(h); property_key.hash(h); }
            Expr::ReflectGetMetadataKeys { target, property_key, } => { tag(h, 458); target.as_ref().hash(h); property_key.hash(h); }
            Expr::ReflectGetOwnMetadataKeys { target, property_key, } => { tag(h, 459); target.as_ref().hash(h); property_key.hash(h); }
            Expr::ReflectDeleteMetadata { key, target, property_key, } => { tag(h, 460); key.as_ref().hash(h); target.as_ref().hash(h); property_key.hash(h); }
            Expr::AsyncStepDone { value, step_closure, } => { tag(h, 442); value.as_ref().hash(h); step_closure.as_ref().hash(h); }
            Expr::CurrentStepClosure => tag(h, 443),
            Expr::AsyncFirstCall { step_closure } => { tag(h, 444); step_closure.as_ref().hash(h); }
            Expr::TaggedTemplateStrings { site_id, cooked, raw } => { tag(h, 445); site_id.hash(h); cooked.hash(h); raw.hash(h); }
            Expr::TemplateRaw(e) => { tag(h, 446); e.as_ref().hash(h); }
            Expr::RegisterClassParentDynamic { class_name, parent_expr, } => { tag(h, 447); class_name.hash(h); parent_expr.as_ref().hash(h); }
            Expr::RegisterClassCaptures { class_name, captures } => { tag(h, 12241); class_name.hash(h); for c in captures { c.hash(h); } }
            Expr::ClassCaptureValue { class_name, index, fallback, prefer_fallback } => { tag(h, 12242); class_name.hash(h); index.hash(h); fallback.hash(h); prefer_fallback.hash(h); }
            Expr::RegisterClassStaticSymbol { class_name, key_expr, value_expr, } => { tag(h, 12025); class_name.hash(h); key_expr.as_ref().hash(h); value_expr.as_ref().hash(h); }
            Expr::RegisterClassComputedMethod { class_name, key_expr, method_name, is_static, param_count, has_rest } => { tag(h, 12233); class_name.hash(h); key_expr.as_ref().hash(h); method_name.hash(h); is_static.hash(h); param_count.hash(h); has_rest.hash(h); }
            Expr::RegisterClassComputedAccessor { class_name, key_expr, getter_name, setter_name, is_static } => { tag(h, 12234); class_name.hash(h); key_expr.as_ref().hash(h); getter_name.hash(h); setter_name.hash(h); is_static.hash(h); }
            Expr::ClassExprFresh { template, named_statics, symbol_statics, captured_args, } => { tag(h, 12026); template.hash(h); for (n, v) in named_statics { n.hash(h); v.hash(h); } for (k, v) in symbol_statics { k.hash(h); v.hash(h); } for a in captured_args { a.hash(h); } }
            Expr::SetFunctionPrototype { func, proto } => { tag(h, 448); func.as_ref().hash(h); proto.as_ref().hash(h); }
            Expr::RegisterPrototypeMethod { class_name, method_name, value, } => { tag(h, 463); class_name.hash(h); method_name.hash(h); value.as_ref().hash(h); }
            Expr::RegisterFunctionPrototypeMethod { func, method_name, value, } => { tag(h, 464); func.as_ref().hash(h); method_name.hash(h); value.as_ref().hash(h); }
            Expr::GetFunctionPrototypeMethod { func, method_name } => { tag(h, 1465); func.as_ref().hash(h); method_name.hash(h); }
            Expr::WebAssemblyValidate(bytes) => { tag(h, 12027); bytes.as_ref().hash(h); }
            Expr::WebAssemblyCompile(bytes) => { tag(h, 12050); bytes.as_ref().hash(h); }
            Expr::WebAssemblyModuleNew(bytes) => { tag(h, 12051); bytes.as_ref().hash(h); }
            Expr::WebAssemblyModuleExports(module) => { tag(h, 12052); module.as_ref().hash(h); }
            Expr::WebAssemblyModuleImports(module) => { tag(h, 12053); module.as_ref().hash(h); }
            Expr::WebAssemblyModuleCustomSections { module, name } => { tag(h, 12054); module.as_ref().hash(h); name.as_ref().hash(h); }
            Expr::WebAssemblyInstantiate(bytes) => { tag(h, 12028); bytes.as_ref().hash(h); }
            Expr::WebAssemblyCallExport { instance, name, args, } => { tag(h, 12029); instance.as_ref().hash(h); name.as_ref().hash(h); args.hash(h); }
            Expr::DynamicImport { paths, arg, byte_offset, deferred_error, synchronous } => { tag(h, 12030); for p in paths { p.hash(h); } arg.as_ref().hash(h); byte_offset.hash(h); deferred_error.hash(h); synchronous.hash(h); }
            Expr::WorkerNew { paths, filename, options, is_eval } => {
                tag(h, 12055);
                for p in paths { p.hash(h); }
                filename.as_ref().hash(h);
                match options {
                    Some(e) => {
                        true.hash(h);
                        e.as_ref().hash(h);
                    }
                    None => false.hash(h),
                }
                is_eval.hash(h);
            }
        }
    }
}
