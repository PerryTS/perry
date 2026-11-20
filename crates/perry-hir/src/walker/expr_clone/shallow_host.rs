//! The shallow copies of the process, fs, path, os, buffer, crypto, math and global-function nodes (see [`super::shallow`]).

use super::*;

/// The copy of `e` with a hole (counted in `n`) at each sub-expression, for the
/// variants of this group; any other variant goes on to the next group.
#[allow(clippy::clone_on_copy)]
pub(super) fn shallow(e: &Expr, n: &mut usize) -> Expr {
    match e {
        Expr::ProcessArgv => Expr::ProcessArgv,
        Expr::ProcessMemoryUsage => Expr::ProcessMemoryUsage,
        Expr::ProcessPid => Expr::ProcessPid,
        Expr::ProcessPpid => Expr::ProcessPpid,
        Expr::ProcessVersion => Expr::ProcessVersion,
        Expr::ProcessVersions => Expr::ProcessVersions,
        Expr::ProcessHrtimeBigint => Expr::ProcessHrtimeBigint,
        Expr::ProcessHrtime(a0) => Expr::ProcessHrtime(Shallow::shallow(a0, n)),
        Expr::ProcessNextTick { callback, args } => Expr::ProcessNextTick {
            callback: Shallow::shallow(callback, n),
            args: Shallow::shallow(args, n),
        },
        Expr::ProcessOn { event, handler } => Expr::ProcessOn {
            event: Shallow::shallow(event, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProcessOnce { event, handler } => Expr::ProcessOnce {
            event: Shallow::shallow(event, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProcessChdir(a0) => Expr::ProcessChdir(Shallow::shallow(a0, n)),
        Expr::ProcessKill { pid, signal } => Expr::ProcessKill {
            pid: Shallow::shallow(pid, n),
            signal: Shallow::shallow(signal, n),
        },
        Expr::ProcessExit(a0) => Expr::ProcessExit(Shallow::shallow(a0, n)),
        Expr::ProcessAbort => Expr::ProcessAbort,
        Expr::ProcessUmask(a0) => Expr::ProcessUmask(Shallow::shallow(a0, n)),
        Expr::ProcessThreadCpuUsage(a0) => Expr::ProcessThreadCpuUsage(Shallow::shallow(a0, n)),
        Expr::ProcessAvailableMemory => Expr::ProcessAvailableMemory,
        Expr::ProcessConstrainedMemory => Expr::ProcessConstrainedMemory,
        Expr::ProcessPosixCredential(a0) => Expr::ProcessPosixCredential(a0.clone()),
        Expr::ProcessEmitWarning(a0) => Expr::ProcessEmitWarning(Shallow::shallow(a0, n)),
        Expr::ProcessCpuUsage(a0) => Expr::ProcessCpuUsage(Shallow::shallow(a0, n)),
        Expr::ProcessResourceUsage => Expr::ProcessResourceUsage,
        Expr::ProcessActiveResourcesInfo => Expr::ProcessActiveResourcesInfo,
        Expr::ProcessTitle => Expr::ProcessTitle,
        Expr::ProcessSetTitle(a0) => Expr::ProcessSetTitle(Shallow::shallow(a0, n)),
        Expr::ProcessStdin => Expr::ProcessStdin,
        Expr::ProcessStdout => Expr::ProcessStdout,
        Expr::ProcessStderr => Expr::ProcessStderr,
        Expr::ProcessStdinSetRawMode(a0) => Expr::ProcessStdinSetRawMode(Shallow::shallow(a0, n)),
        Expr::ProcessStdinOn { event, handler } => Expr::ProcessStdinOn {
            event: Shallow::shallow(event, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProcessStdinRemoveListener { event, handler } => Expr::ProcessStdinRemoveListener {
            event: Shallow::shallow(event, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProcessStdinLifecycle(a0) => Expr::ProcessStdinLifecycle(a0.clone()),
        Expr::ProcessStdoutOn { event, handler } => Expr::ProcessStdoutOn {
            event: Shallow::shallow(event, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProcessStdinIsTTY => Expr::ProcessStdinIsTTY,
        Expr::ProcessStdoutIsTTY => Expr::ProcessStdoutIsTTY,
        Expr::ProcessStderrIsTTY => Expr::ProcessStderrIsTTY,
        Expr::ProcessStdoutColumns => Expr::ProcessStdoutColumns,
        Expr::ProcessStdoutRows => Expr::ProcessStdoutRows,
        Expr::TtyIsAtty(a0) => Expr::TtyIsAtty(Shallow::shallow(a0, n)),
        Expr::FsReadFileSync(a0) => Expr::FsReadFileSync(Shallow::shallow(a0, n)),
        Expr::FsWriteFileSync(a0, a1) => {
            Expr::FsWriteFileSync(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::FsExistsSync(a0) => Expr::FsExistsSync(Shallow::shallow(a0, n)),
        Expr::FsMkdirSync(a0) => Expr::FsMkdirSync(Shallow::shallow(a0, n)),
        Expr::FsUnlinkSync(a0) => Expr::FsUnlinkSync(Shallow::shallow(a0, n)),
        Expr::FsAppendFileSync(a0, a1) => {
            Expr::FsAppendFileSync(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::FsReadFileBinary(a0) => Expr::FsReadFileBinary(Shallow::shallow(a0, n)),
        Expr::FsRmRecursive(a0) => Expr::FsRmRecursive(Shallow::shallow(a0, n)),
        Expr::PathJoin(a0, a1) => Expr::PathJoin(Shallow::shallow(a0, n), Shallow::shallow(a1, n)),
        Expr::PathDirname(a0) => Expr::PathDirname(Shallow::shallow(a0, n)),
        Expr::PathBasename(a0) => Expr::PathBasename(Shallow::shallow(a0, n)),
        Expr::PathBasenameExt(a0, a1) => {
            Expr::PathBasenameExt(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::PathExtname(a0) => Expr::PathExtname(Shallow::shallow(a0, n)),
        Expr::PathResolve(a0) => Expr::PathResolve(Shallow::shallow(a0, n)),
        Expr::PathIsAbsolute(a0) => Expr::PathIsAbsolute(Shallow::shallow(a0, n)),
        Expr::PathRelative(a0, a1) => {
            Expr::PathRelative(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::PathNormalize(a0) => Expr::PathNormalize(Shallow::shallow(a0, n)),
        Expr::PathParse(a0) => Expr::PathParse(Shallow::shallow(a0, n)),
        Expr::PathFormat(a0) => Expr::PathFormat(Shallow::shallow(a0, n)),
        Expr::PathSep => Expr::PathSep,
        Expr::PathDelimiter => Expr::PathDelimiter,
        Expr::PathToNamespacedPath(a0) => Expr::PathToNamespacedPath(Shallow::shallow(a0, n)),
        Expr::PathMatchesGlob(a0, a1) => {
            Expr::PathMatchesGlob(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::PathResolveJoin(a0, a1) => {
            Expr::PathResolveJoin(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::PathWin32Join(a0, a1) => {
            Expr::PathWin32Join(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::PathWin32 { method, args } => Expr::PathWin32 {
            method: method.clone(),
            args: Shallow::shallow(args, n),
        },
        Expr::WeakRefNew(a0) => Expr::WeakRefNew(Shallow::shallow(a0, n)),
        Expr::WeakRefDeref(a0) => Expr::WeakRefDeref(Shallow::shallow(a0, n)),
        Expr::FinalizationRegistryNew(a0) => Expr::FinalizationRegistryNew(Shallow::shallow(a0, n)),
        Expr::FinalizationRegistryRegister {
            registry,
            target,
            held,
            token,
        } => Expr::FinalizationRegistryRegister {
            registry: Shallow::shallow(registry, n),
            target: Shallow::shallow(target, n),
            held: Shallow::shallow(held, n),
            token: Shallow::shallow(token, n),
        },
        Expr::FinalizationRegistryUnregister { registry, token } => {
            Expr::FinalizationRegistryUnregister {
                registry: Shallow::shallow(registry, n),
                token: Shallow::shallow(token, n),
            }
        }
        Expr::ObjectDefineProperty(a0, a1, a2) => Expr::ObjectDefineProperty(
            Shallow::shallow(a0, n),
            Shallow::shallow(a1, n),
            Shallow::shallow(a2, n),
        ),
        Expr::ObjectGetOwnPropertyDescriptor(a0, a1) => {
            Expr::ObjectGetOwnPropertyDescriptor(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::ObjectGetOwnPropertyDescriptors(a0) => {
            Expr::ObjectGetOwnPropertyDescriptors(Shallow::shallow(a0, n))
        }
        Expr::ObjectGetOwnPropertyNames(a0) => {
            Expr::ObjectGetOwnPropertyNames(Shallow::shallow(a0, n))
        }
        Expr::ObjectCreate(a0, a1) => {
            Expr::ObjectCreate(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::ObjectFreeze(a0) => Expr::ObjectFreeze(Shallow::shallow(a0, n)),
        Expr::ObjectSeal(a0) => Expr::ObjectSeal(Shallow::shallow(a0, n)),
        Expr::ObjectPreventExtensions(a0) => Expr::ObjectPreventExtensions(Shallow::shallow(a0, n)),
        Expr::ObjectIsFrozen(a0) => Expr::ObjectIsFrozen(Shallow::shallow(a0, n)),
        Expr::ObjectIsSealed(a0) => Expr::ObjectIsSealed(Shallow::shallow(a0, n)),
        Expr::ObjectIsExtensible(a0) => Expr::ObjectIsExtensible(Shallow::shallow(a0, n)),
        Expr::ObjectGetPrototypeOf(a0) => Expr::ObjectGetPrototypeOf(Shallow::shallow(a0, n)),
        Expr::ObjectSetPrototypeOf(a0, a1) => {
            Expr::ObjectSetPrototypeOf(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::ObjectDefineProperties(a0, a1) => {
            Expr::ObjectDefineProperties(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::ObjectGetOwnPropertySymbols(a0) => {
            Expr::ObjectGetOwnPropertySymbols(Shallow::shallow(a0, n))
        }
        Expr::SymbolNew(a0) => Expr::SymbolNew(Shallow::shallow(a0, n)),
        Expr::SymbolFor(a0) => Expr::SymbolFor(Shallow::shallow(a0, n)),
        Expr::SymbolKeyFor(a0) => Expr::SymbolKeyFor(Shallow::shallow(a0, n)),
        Expr::SymbolDescription(a0) => Expr::SymbolDescription(Shallow::shallow(a0, n)),
        Expr::RegExpEscape(a0) => Expr::RegExpEscape(Shallow::shallow(a0, n)),
        Expr::SymbolToString(a0) => Expr::SymbolToString(Shallow::shallow(a0, n)),
        Expr::FileURLToPath(a0) => Expr::FileURLToPath(Shallow::shallow(a0, n)),
        Expr::RegExpExecIndex => Expr::RegExpExecIndex,
        Expr::RegExpExecGroups => Expr::RegExpExecGroups,
        Expr::JsonParse(a0) => Expr::JsonParse(Shallow::shallow(a0, n)),
        Expr::JsonParseTyped {
            text,
            ty,
            ordered_keys,
        } => Expr::JsonParseTyped {
            text: Shallow::shallow(text, n),
            ty: ty.clone(),
            ordered_keys: ordered_keys.clone(),
        },
        Expr::JsonParseReviver { text, reviver } => Expr::JsonParseReviver {
            text: Shallow::shallow(text, n),
            reviver: Shallow::shallow(reviver, n),
        },
        Expr::JsonParseWithReviver(a0, a1) => {
            Expr::JsonParseWithReviver(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::JsonStringify(a0) => Expr::JsonStringify(Shallow::shallow(a0, n)),
        Expr::JsonStringifyPretty {
            value,
            replacer,
            space,
        } => Expr::JsonStringifyPretty {
            value: Shallow::shallow(value, n),
            replacer: Shallow::shallow(replacer, n),
            space: Shallow::shallow(space, n),
        },
        Expr::JsonStringifyFull(a0, a1, a2) => Expr::JsonStringifyFull(
            Shallow::shallow(a0, n),
            Shallow::shallow(a1, n),
            Shallow::shallow(a2, n),
        ),
        Expr::JsonRawJson(a0) => Expr::JsonRawJson(Shallow::shallow(a0, n)),
        Expr::JsonIsRawJson(a0) => Expr::JsonIsRawJson(Shallow::shallow(a0, n)),
        Expr::MathFloor(a0) => Expr::MathFloor(Shallow::shallow(a0, n)),
        Expr::MathCeil(a0) => Expr::MathCeil(Shallow::shallow(a0, n)),
        Expr::MathRound(a0) => Expr::MathRound(Shallow::shallow(a0, n)),
        Expr::MathTrunc(a0) => Expr::MathTrunc(Shallow::shallow(a0, n)),
        Expr::MathSign(a0) => Expr::MathSign(Shallow::shallow(a0, n)),
        Expr::MathAbs(a0) => Expr::MathAbs(Shallow::shallow(a0, n)),
        Expr::MathSqrt(a0) => Expr::MathSqrt(Shallow::shallow(a0, n)),
        Expr::MathLog(a0) => Expr::MathLog(Shallow::shallow(a0, n)),
        Expr::MathLog2(a0) => Expr::MathLog2(Shallow::shallow(a0, n)),
        Expr::MathLog10(a0) => Expr::MathLog10(Shallow::shallow(a0, n)),
        Expr::MathPow(a0, a1) => Expr::MathPow(Shallow::shallow(a0, n), Shallow::shallow(a1, n)),
        Expr::MathMin(a0) => Expr::MathMin(Shallow::shallow(a0, n)),
        Expr::MathMax(a0) => Expr::MathMax(Shallow::shallow(a0, n)),
        Expr::MathMinSpread(a0) => Expr::MathMinSpread(Shallow::shallow(a0, n)),
        Expr::MathMaxSpread(a0) => Expr::MathMaxSpread(Shallow::shallow(a0, n)),
        Expr::MathImul(a0, a1) => Expr::MathImul(Shallow::shallow(a0, n), Shallow::shallow(a1, n)),
        Expr::MathRandom => Expr::MathRandom,
        Expr::MathSin(a0) => Expr::MathSin(Shallow::shallow(a0, n)),
        Expr::MathCos(a0) => Expr::MathCos(Shallow::shallow(a0, n)),
        Expr::MathTan(a0) => Expr::MathTan(Shallow::shallow(a0, n)),
        Expr::MathAsin(a0) => Expr::MathAsin(Shallow::shallow(a0, n)),
        Expr::MathAcos(a0) => Expr::MathAcos(Shallow::shallow(a0, n)),
        Expr::MathAtan(a0) => Expr::MathAtan(Shallow::shallow(a0, n)),
        Expr::MathAtan2(a0, a1) => {
            Expr::MathAtan2(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::MathCbrt(a0) => Expr::MathCbrt(Shallow::shallow(a0, n)),
        Expr::MathHypot(a0) => Expr::MathHypot(Shallow::shallow(a0, n)),
        Expr::MathFround(a0) => Expr::MathFround(Shallow::shallow(a0, n)),
        Expr::MathF16round(a0) => Expr::MathF16round(Shallow::shallow(a0, n)),
        Expr::MathClz32(a0) => Expr::MathClz32(Shallow::shallow(a0, n)),
        Expr::MathExpm1(a0) => Expr::MathExpm1(Shallow::shallow(a0, n)),
        Expr::MathLog1p(a0) => Expr::MathLog1p(Shallow::shallow(a0, n)),
        Expr::MathSinh(a0) => Expr::MathSinh(Shallow::shallow(a0, n)),
        Expr::MathCosh(a0) => Expr::MathCosh(Shallow::shallow(a0, n)),
        Expr::MathTanh(a0) => Expr::MathTanh(Shallow::shallow(a0, n)),
        Expr::MathAsinh(a0) => Expr::MathAsinh(Shallow::shallow(a0, n)),
        Expr::MathAcosh(a0) => Expr::MathAcosh(Shallow::shallow(a0, n)),
        Expr::MathAtanh(a0) => Expr::MathAtanh(Shallow::shallow(a0, n)),
        Expr::MathExp(a0) => Expr::MathExp(Shallow::shallow(a0, n)),
        Expr::PerformanceNow => Expr::PerformanceNow,
        Expr::WebAssemblyValidate(a0) => Expr::WebAssemblyValidate(Shallow::shallow(a0, n)),
        Expr::WebAssemblyCompile(a0) => Expr::WebAssemblyCompile(Shallow::shallow(a0, n)),
        Expr::WebAssemblyModuleNew(a0) => Expr::WebAssemblyModuleNew(Shallow::shallow(a0, n)),
        Expr::WebAssemblyModuleExports(a0) => {
            Expr::WebAssemblyModuleExports(Shallow::shallow(a0, n))
        }
        Expr::WebAssemblyModuleImports(a0) => {
            Expr::WebAssemblyModuleImports(Shallow::shallow(a0, n))
        }
        Expr::WebAssemblyModuleCustomSections { module, name } => {
            Expr::WebAssemblyModuleCustomSections {
                module: Shallow::shallow(module, n),
                name: Shallow::shallow(name, n),
            }
        }
        Expr::WebAssemblyInstantiate { bytes, imports } => Expr::WebAssemblyInstantiate {
            bytes: Shallow::shallow(bytes, n),
            imports: Shallow::shallow(imports, n),
        },
        Expr::WebAssemblyCallExport {
            instance,
            name,
            args,
        } => Expr::WebAssemblyCallExport {
            instance: Shallow::shallow(instance, n),
            name: Shallow::shallow(name, n),
            args: Shallow::shallow(args, n),
        },
        Expr::Atob(a0) => Expr::Atob(Shallow::shallow(a0, n)),
        Expr::Btoa(a0) => Expr::Btoa(Shallow::shallow(a0, n)),
        Expr::TextEncoderNew => Expr::TextEncoderNew,
        Expr::TextEncoderEncode(a0) => Expr::TextEncoderEncode(Shallow::shallow(a0, n)),
        Expr::TextEncoderEncodeInto { source, dest } => Expr::TextEncoderEncodeInto {
            source: Shallow::shallow(source, n),
            dest: Shallow::shallow(dest, n),
        },
        Expr::TextDecoderNew {
            label,
            fatal,
            ignore_bom,
        } => Expr::TextDecoderNew {
            label: Shallow::shallow(label, n),
            fatal: Shallow::shallow(fatal, n),
            ignore_bom: Shallow::shallow(ignore_bom, n),
        },
        Expr::TextDecoderDecode { decoder, input } => Expr::TextDecoderDecode {
            decoder: Shallow::shallow(decoder, n),
            input: Shallow::shallow(input, n),
        },
        Expr::TextDecoderEncoding(a0) => Expr::TextDecoderEncoding(Shallow::shallow(a0, n)),
        Expr::TextDecoderFatal(a0) => Expr::TextDecoderFatal(Shallow::shallow(a0, n)),
        Expr::TextDecoderIgnoreBom(a0) => Expr::TextDecoderIgnoreBom(Shallow::shallow(a0, n)),
        Expr::EncodeURI(a0) => Expr::EncodeURI(Shallow::shallow(a0, n)),
        Expr::DecodeURI(a0) => Expr::DecodeURI(Shallow::shallow(a0, n)),
        Expr::EncodeURIComponent(a0) => Expr::EncodeURIComponent(Shallow::shallow(a0, n)),
        Expr::DecodeURIComponent(a0) => Expr::DecodeURIComponent(Shallow::shallow(a0, n)),
        Expr::StructuredClone { value, options } => Expr::StructuredClone {
            value: Shallow::shallow(value, n),
            options: Shallow::shallow(options, n),
        },
        Expr::LinkGeneratorPrototype {
            obj,
            is_async,
            fixed_state,
            i32_controls,
            i1_controls,
        } => Expr::LinkGeneratorPrototype {
            obj: Shallow::shallow(obj, n),
            is_async: is_async.clone(),
            fixed_state: fixed_state.clone(),
            i32_controls: i32_controls.clone(),
            i1_controls: i1_controls.clone(),
        },
        Expr::QueueMicrotask(a0) => Expr::QueueMicrotask(Shallow::shallow(a0, n)),
        Expr::IterResultSet(a0, a1) => Expr::IterResultSet(Shallow::shallow(a0, n), a1.clone()),
        Expr::IterResultGetValue => Expr::IterResultGetValue,
        Expr::IterResultGetDone => Expr::IterResultGetDone,
        Expr::AsyncStepChain {
            value,
            step_closure,
        } => Expr::AsyncStepChain {
            value: Shallow::shallow(value, n),
            step_closure: Shallow::shallow(step_closure, n),
        },
        Expr::AsyncStepResumeNow {
            value,
            step_closure,
        } => Expr::AsyncStepResumeNow {
            value: Shallow::shallow(value, n),
            step_closure: Shallow::shallow(step_closure, n),
        },
        Expr::AsyncStepDeclined(a0) => Expr::AsyncStepDeclined(Shallow::shallow(a0, n)),
        Expr::AsyncStepDone {
            value,
            step_closure,
        } => Expr::AsyncStepDone {
            value: Shallow::shallow(value, n),
            step_closure: Shallow::shallow(step_closure, n),
        },
        Expr::CurrentStepClosure => Expr::CurrentStepClosure,
        Expr::AsyncFirstCall { step_closure } => Expr::AsyncFirstCall {
            step_closure: Shallow::shallow(step_closure, n),
        },
        Expr::AsyncGenResume {
            step_closure,
            value,
            is_error,
        } => Expr::AsyncGenResume {
            step_closure: Shallow::shallow(step_closure, n),
            value: Shallow::shallow(value, n),
            is_error: is_error.clone(),
        },
        Expr::CryptoRandomBytes(a0) => Expr::CryptoRandomBytes(Shallow::shallow(a0, n)),
        Expr::CryptoRandomUUID => Expr::CryptoRandomUUID,
        Expr::CryptoRandomUUIDv7 => Expr::CryptoRandomUUIDv7,
        Expr::CryptoSha256(a0) => Expr::CryptoSha256(Shallow::shallow(a0, n)),
        Expr::CryptoMd5(a0) => Expr::CryptoMd5(Shallow::shallow(a0, n)),
        Expr::WebCryptoDigest { algo, data } => Expr::WebCryptoDigest {
            algo: Shallow::shallow(algo, n),
            data: Shallow::shallow(data, n),
        },
        Expr::WebCryptoImportKey {
            format,
            key,
            algorithm,
            extractable,
            usages,
        } => Expr::WebCryptoImportKey {
            format: Shallow::shallow(format, n),
            key: Shallow::shallow(key, n),
            algorithm: Shallow::shallow(algorithm, n),
            extractable: Shallow::shallow(extractable, n),
            usages: Shallow::shallow(usages, n),
        },
        Expr::WebCryptoExportKey { format, key } => Expr::WebCryptoExportKey {
            format: Shallow::shallow(format, n),
            key: Shallow::shallow(key, n),
        },
        Expr::WebCryptoSign {
            algorithm,
            key,
            data,
        } => Expr::WebCryptoSign {
            algorithm: Shallow::shallow(algorithm, n),
            key: Shallow::shallow(key, n),
            data: Shallow::shallow(data, n),
        },
        Expr::WebCryptoVerify {
            algorithm,
            key,
            signature,
            data,
        } => Expr::WebCryptoVerify {
            algorithm: Shallow::shallow(algorithm, n),
            key: Shallow::shallow(key, n),
            signature: Shallow::shallow(signature, n),
            data: Shallow::shallow(data, n),
        },
        Expr::WebCryptoDeriveBits {
            algorithm,
            base_key,
            length,
        } => Expr::WebCryptoDeriveBits {
            algorithm: Shallow::shallow(algorithm, n),
            base_key: Shallow::shallow(base_key, n),
            length: Shallow::shallow(length, n),
        },
        Expr::WebCryptoDeriveKey {
            algorithm,
            base_key,
            derived_key_algorithm,
            extractable,
            usages,
        } => Expr::WebCryptoDeriveKey {
            algorithm: Shallow::shallow(algorithm, n),
            base_key: Shallow::shallow(base_key, n),
            derived_key_algorithm: Shallow::shallow(derived_key_algorithm, n),
            extractable: Shallow::shallow(extractable, n),
            usages: Shallow::shallow(usages, n),
        },
        Expr::WebCryptoEncrypt {
            algorithm,
            key,
            data,
        } => Expr::WebCryptoEncrypt {
            algorithm: Shallow::shallow(algorithm, n),
            key: Shallow::shallow(key, n),
            data: Shallow::shallow(data, n),
        },
        Expr::WebCryptoDecrypt {
            algorithm,
            key,
            data,
        } => Expr::WebCryptoDecrypt {
            algorithm: Shallow::shallow(algorithm, n),
            key: Shallow::shallow(key, n),
            data: Shallow::shallow(data, n),
        },
        Expr::WebCryptoGenerateKey {
            algorithm,
            extractable,
            usages,
        } => Expr::WebCryptoGenerateKey {
            algorithm: Shallow::shallow(algorithm, n),
            extractable: Shallow::shallow(extractable, n),
            usages: Shallow::shallow(usages, n),
        },
        Expr::WebCryptoWrapKey {
            format,
            key,
            wrapping_key,
            wrap_algorithm,
        } => Expr::WebCryptoWrapKey {
            format: Shallow::shallow(format, n),
            key: Shallow::shallow(key, n),
            wrapping_key: Shallow::shallow(wrapping_key, n),
            wrap_algorithm: Shallow::shallow(wrap_algorithm, n),
        },
        Expr::WebCryptoUnwrapKey {
            format,
            wrapped_key,
            unwrapping_key,
            unwrap_algorithm,
            unwrapped_key_algorithm,
            extractable,
            usages,
        } => Expr::WebCryptoUnwrapKey {
            format: Shallow::shallow(format, n),
            wrapped_key: Shallow::shallow(wrapped_key, n),
            unwrapping_key: Shallow::shallow(unwrapping_key, n),
            unwrap_algorithm: Shallow::shallow(unwrap_algorithm, n),
            unwrapped_key_algorithm: Shallow::shallow(unwrapped_key_algorithm, n),
            extractable: Shallow::shallow(extractable, n),
            usages: Shallow::shallow(usages, n),
        },
        Expr::CryptoRandomFillSync {
            buffer,
            offset,
            size,
        } => Expr::CryptoRandomFillSync {
            buffer: Shallow::shallow(buffer, n),
            offset: Shallow::shallow(offset, n),
            size: Shallow::shallow(size, n),
        },
        Expr::OsPlatform => Expr::OsPlatform,
        Expr::OsArch => Expr::OsArch,
        Expr::OsHostname => Expr::OsHostname,
        Expr::OsHomedir => Expr::OsHomedir,
        Expr::OsTmpdir => Expr::OsTmpdir,
        Expr::OsTotalmem => Expr::OsTotalmem,
        Expr::OsFreemem => Expr::OsFreemem,
        Expr::OsUptime => Expr::OsUptime,
        Expr::OsType => Expr::OsType,
        Expr::OsRelease => Expr::OsRelease,
        Expr::OsCpus => Expr::OsCpus,
        Expr::OsNetworkInterfaces => Expr::OsNetworkInterfaces,
        Expr::OsUserInfo => Expr::OsUserInfo,
        Expr::OsUserInfoBuffer => Expr::OsUserInfoBuffer,
        Expr::OsEOL => Expr::OsEOL,
        Expr::OsDevNull => Expr::OsDevNull,
        Expr::OsAvailableParallelism => Expr::OsAvailableParallelism,
        Expr::OsEndianness => Expr::OsEndianness,
        Expr::OsLoadavg => Expr::OsLoadavg,
        Expr::OsMachine => Expr::OsMachine,
        Expr::OsVersion => Expr::OsVersion,
        Expr::BufferFrom { data, encoding } => Expr::BufferFrom {
            data: Shallow::shallow(data, n),
            encoding: Shallow::shallow(encoding, n),
        },
        Expr::BufferFromArrayBuffer {
            data,
            byte_offset,
            length,
        } => Expr::BufferFromArrayBuffer {
            data: Shallow::shallow(data, n),
            byte_offset: Shallow::shallow(byte_offset, n),
            length: Shallow::shallow(length, n),
        },
        Expr::BufferAlloc {
            size,
            fill,
            encoding,
        } => Expr::BufferAlloc {
            size: Shallow::shallow(size, n),
            fill: Shallow::shallow(fill, n),
            encoding: Shallow::shallow(encoding, n),
        },
        Expr::BufferAllocUnsafe(a0) => Expr::BufferAllocUnsafe(Shallow::shallow(a0, n)),
        Expr::BufferConcat(a0) => Expr::BufferConcat(Shallow::shallow(a0, n)),
        Expr::BufferConcatWithLength { list, total_length } => Expr::BufferConcatWithLength {
            list: Shallow::shallow(list, n),
            total_length: Shallow::shallow(total_length, n),
        },
        Expr::BufferIsBuffer(a0) => Expr::BufferIsBuffer(Shallow::shallow(a0, n)),
        Expr::BufferIsEncoding(a0) => Expr::BufferIsEncoding(Shallow::shallow(a0, n)),
        Expr::BufferByteLength { data, encoding } => Expr::BufferByteLength {
            data: Shallow::shallow(data, n),
            encoding: Shallow::shallow(encoding, n),
        },
        Expr::BufferToString { buffer, encoding } => Expr::BufferToString {
            buffer: Shallow::shallow(buffer, n),
            encoding: Shallow::shallow(encoding, n),
        },
        Expr::BufferLength(a0) => Expr::BufferLength(Shallow::shallow(a0, n)),
        Expr::BufferSlice { buffer, start, end } => Expr::BufferSlice {
            buffer: Shallow::shallow(buffer, n),
            start: Shallow::shallow(start, n),
            end: Shallow::shallow(end, n),
        },
        Expr::BufferCopy {
            source,
            target,
            target_start,
            source_start,
            source_end,
        } => Expr::BufferCopy {
            source: Shallow::shallow(source, n),
            target: Shallow::shallow(target, n),
            target_start: Shallow::shallow(target_start, n),
            source_start: Shallow::shallow(source_start, n),
            source_end: Shallow::shallow(source_end, n),
        },
        Expr::BufferWrite {
            buffer,
            string,
            offset,
            encoding,
        } => Expr::BufferWrite {
            buffer: Shallow::shallow(buffer, n),
            string: Shallow::shallow(string, n),
            offset: Shallow::shallow(offset, n),
            encoding: Shallow::shallow(encoding, n),
        },
        Expr::BufferFill { buffer, value } => Expr::BufferFill {
            buffer: Shallow::shallow(buffer, n),
            value: Shallow::shallow(value, n),
        },
        Expr::BufferEquals { buffer, other } => Expr::BufferEquals {
            buffer: Shallow::shallow(buffer, n),
            other: Shallow::shallow(other, n),
        },
        Expr::BufferIndexGet { buffer, index } => Expr::BufferIndexGet {
            buffer: Shallow::shallow(buffer, n),
            index: Shallow::shallow(index, n),
        },
        Expr::BufferIndexSet {
            buffer,
            index,
            value,
        } => Expr::BufferIndexSet {
            buffer: Shallow::shallow(buffer, n),
            index: Shallow::shallow(index, n),
            value: Shallow::shallow(value, n),
        },
        Expr::Uint8ArrayNew(a0) => Expr::Uint8ArrayNew(Shallow::shallow(a0, n)),
        Expr::Uint8ArrayFrom(a0) => Expr::Uint8ArrayFrom(Shallow::shallow(a0, n)),
        Expr::Uint8ArrayLength(a0) => Expr::Uint8ArrayLength(Shallow::shallow(a0, n)),
        Expr::Uint8ArrayGet { array, index } => Expr::Uint8ArrayGet {
            array: Shallow::shallow(array, n),
            index: Shallow::shallow(index, n),
        },
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => Expr::Uint8ArraySet {
            array: Shallow::shallow(array, n),
            index: Shallow::shallow(index, n),
            value: Shallow::shallow(value, n),
        },
        Expr::TypedArrayNew { kind, arg } => Expr::TypedArrayNew {
            kind: kind.clone(),
            arg: Shallow::shallow(arg, n),
        },
        Expr::NativeArenaAlloc(a0) => Expr::NativeArenaAlloc(Shallow::shallow(a0, n)),
        Expr::NativeArenaView {
            owner,
            kind,
            byte_offset,
            length,
        } => Expr::NativeArenaView {
            owner: Shallow::shallow(owner, n),
            kind: kind.clone(),
            byte_offset: Shallow::shallow(byte_offset, n),
            length: Shallow::shallow(length, n),
        },
        Expr::NativePodView {
            owner,
            byte_offset,
            count,
            view_type,
        } => Expr::NativePodView {
            owner: Shallow::shallow(owner, n),
            byte_offset: Shallow::shallow(byte_offset, n),
            count: Shallow::shallow(count, n),
            view_type: view_type.clone(),
        },
        Expr::PodLayoutSizeOf { ty } => Expr::PodLayoutSizeOf { ty: ty.clone() },
        Expr::PodLayoutAlignOf { ty } => Expr::PodLayoutAlignOf { ty: ty.clone() },
        Expr::PodLayoutOffsetOf { ty, field_path } => Expr::PodLayoutOffsetOf {
            ty: ty.clone(),
            field_path: field_path.clone(),
        },
        Expr::NativeArenaDispose(a0) => Expr::NativeArenaDispose(Shallow::shallow(a0, n)),
        Expr::NativeMemoryFillU32 { view, value } => Expr::NativeMemoryFillU32 {
            view: Shallow::shallow(view, n),
            value: Shallow::shallow(value, n),
        },
        Expr::NativeMemoryCopy { dst, src } => Expr::NativeMemoryCopy {
            dst: Shallow::shallow(dst, n),
            src: Shallow::shallow(src, n),
        },
        Expr::ChildProcessExecSync { command, options } => Expr::ChildProcessExecSync {
            command: Shallow::shallow(command, n),
            options: Shallow::shallow(options, n),
        },
        Expr::ChildProcessSpawnSync {
            command,
            args,
            options,
        } => Expr::ChildProcessSpawnSync {
            command: Shallow::shallow(command, n),
            args: Shallow::shallow(args, n),
            options: Shallow::shallow(options, n),
        },
        Expr::ChildProcessSpawn {
            command,
            args,
            options,
        } => Expr::ChildProcessSpawn {
            command: Shallow::shallow(command, n),
            args: Shallow::shallow(args, n),
            options: Shallow::shallow(options, n),
        },
        Expr::ChildProcessFork {
            module,
            args,
            options,
        } => Expr::ChildProcessFork {
            module: Shallow::shallow(module, n),
            args: Shallow::shallow(args, n),
            options: Shallow::shallow(options, n),
        },
        Expr::ChildProcessExec {
            command,
            options,
            callback,
        } => Expr::ChildProcessExec {
            command: Shallow::shallow(command, n),
            options: Shallow::shallow(options, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ChildProcessExecFile {
            file,
            args,
            options,
            callback,
        } => Expr::ChildProcessExecFile {
            file: Shallow::shallow(file, n),
            args: Shallow::shallow(args, n),
            options: Shallow::shallow(options, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ChildProcessExecFileSync {
            file,
            args,
            options,
        } => Expr::ChildProcessExecFileSync {
            file: Shallow::shallow(file, n),
            args: Shallow::shallow(args, n),
            options: Shallow::shallow(options, n),
        },
        Expr::ChildProcessSpawnBackground {
            command,
            args,
            log_file,
            env_json,
        } => Expr::ChildProcessSpawnBackground {
            command: Shallow::shallow(command, n),
            args: Shallow::shallow(args, n),
            log_file: Shallow::shallow(log_file, n),
            env_json: Shallow::shallow(env_json, n),
        },
        Expr::ChildProcessGetProcessStatus(a0) => {
            Expr::ChildProcessGetProcessStatus(Shallow::shallow(a0, n))
        }
        Expr::ChildProcessKillProcess(a0) => Expr::ChildProcessKillProcess(Shallow::shallow(a0, n)),
        _ => unreachable!("internal compiler error: Expr::clone: not a host variant"),
    }
}
