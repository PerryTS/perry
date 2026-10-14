//! Environment inputs shared by the object and completed-build cache keys.
//! Keep the ordered list authoritative for both caches; the source audit below
//! requires every compiler environment read to be keyed or explicitly justified.

/// Conservatively includes toolchain, linking and runtime-feature selection as
/// well as lowering gates. Sorted and unique so each input is hashed once.
pub(super) const COMPILE_ENV_VARS: &[&str] = &[
    "ANDROID_NDK_HOME",
    "DYLD_LIBRARY_PATH",
    "HARMONYOS_SDK_HOME",
    "LD_LIBRARY_PATH",
    "LIB",
    "LIBPATH",
    "LIBRARY_PATH",
    "OHOS_SDK_HOME",
    "PATH",
    "PERRY_5951_NO_RETYPE",
    "PERRY_5951_SKIP_MODS",
    "PERRY_AGENT_PTR_ACCESS",
    "PERRY_ALLOC_SITE_SAMPLE",
    "PERRY_ALLOW_DYNAMIC_STDLIB",
    "PERRY_ALLOW_EVAL",
    "PERRY_ALLOW_PARTIAL_CODEGEN",
    "PERRY_ALLOW_PERRY_FEATURES",
    "PERRY_ALLOW_UNIMPLEMENTED",
    "PERRY_ASSERT_CAPTURE_CELLS",
    "PERRY_B4_SABOTAGE",
    "PERRY_BOX_CAPTURE_ENTRY_CELLS",
    "PERRY_CALLEE_BINDING_RESOLUTION",
    "PERRY_CALL_DEVIRT",
    "PERRY_CANONICAL_I32_LOCALS",
    "PERRY_CANONICAL_STR_LOCALS",
    "PERRY_CHECK_CONTRACTS",
    "PERRY_CODEGEN_UNITS",
    "PERRY_CODEGEN_UNIT_BYTES",
    "PERRY_CODEGEN_UNIT_SIZE",
    "PERRY_CONCAT_SITE_CACHE",
    "PERRY_CONSTFN_SHAPE",
    "PERRY_CONST_ARRAY_DESCRIPTOR",
    "PERRY_DEBUG_INIT",
    "PERRY_DEBUG_SYMBOLS",
    "PERRY_DISABLE_BUFFER_FAST_PATH",
    "PERRY_DISABLE_WELL_KNOWN",
    "PERRY_DYNAMIC_ADD_PAIR_GUARD",
    "PERRY_ENTRY_SYMBOL",
    "PERRY_ENUM_DIAG",
    "PERRY_EVAL_CSP",
    "PERRY_EXTRA_LINK_ARGS",
    "PERRY_FACTS_BUDGET_SECS",
    "PERRY_FACTS_ELIDE",
    "PERRY_FACTS_ELIDE_TA",
    "PERRY_FACTS_MAX_PTS",
    "PERRY_FACTS_MAX_WORK",
    "PERRY_FACTS_SABOTAGE",
    "PERRY_FACTS_WIDEN",
    "PERRY_FAST_MATH",
    "PERRY_FIELD_REPR_VERIFY",
    "PERRY_FNINFER_APPLY",
    "PERRY_FNINFER_SABOTAGE",
    "PERRY_FNINFER_STRICT",
    "PERRY_FORCE_WELL_KNOWN",
    "PERRY_FP_CONTRACT",
    "PERRY_FULL_OUTLINE_IC",
    "PERRY_FULL_OUTLINE_IC_MIN_FUNCS",
    "PERRY_FUNCTION_SOURCE",
    "PERRY_GC_BUDGETED_OLD_RECLAIM",
    "PERRY_GC_CENSUS",
    "PERRY_GC_FROMSPACE_SCAN",
    "PERRY_GC_FROMSPACE_SCAN_ABORT",
    "PERRY_GC_INSTRUMENTS",
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_GC_PROTECT_FROMSPACE",
    "PERRY_GC_PROTECT_OLD_SWEEP",
    "PERRY_GC_SAFEPOINT_ONLY",
    "PERRY_GC_SCHEDULE_SEED",
    "PERRY_GC_TRACE",
    "PERRY_GC_VERIFY_CLASSIFIER",
    "PERRY_GC_VERIFY_MARK",
    "PERRY_GLOBAL_SCRIPT_THIS",
    "PERRY_GUARDED_ARITH",
    "PERRY_GUARDED_PREINLINE_MAX_IR_BYTES",
    "PERRY_IC_DIAG",
    "PERRY_INLINE_CTOR",
    "PERRY_INLINE_HOT_SMALL",
    "PERRY_INLINE_HOT_SMALL_CAP",
    "PERRY_INLINE_HOT_SMALL_MAX_SITES",
    "PERRY_INLINE_HOT_SMALL_THRESHOLD",
    "PERRY_INLINE_HOT_TLS",
    "PERRY_INLINE_NEW",
    "PERRY_INLINE_NONBIGINT_BITWISE",
    "PERRY_INT_VALUED_LOCALS",
    "PERRY_IOS_SYSROOT",
    "PERRY_JSCVT",
    "PERRY_KEEP_SYMBOLS",
    "PERRY_LAYOUT_DESCS",
    "PERRY_LAYOUT_DIAG",
    "PERRY_LD",
    "PERRY_LD64.LLD",
    "PERRY_LIB_DIR",
    "PERRY_LINUX_ARM64_CC",
    "PERRY_LINUX_ARM64_SYSROOT",
    "PERRY_LINUX_MUSL_AARCH64_CC",
    "PERRY_LINUX_MUSL_CC",
    "PERRY_LINUX_MUSL_SYSROOT",
    "PERRY_LINUX_SYSROOT",
    "PERRY_LLD_LINK",
    "PERRY_LLVM_AR",
    "PERRY_LLVM_BITCODE_LINK",
    "PERRY_LLVM_CLANG",
    "PERRY_LLVM_INPROCESS",
    "PERRY_LLVM_LIB",
    "PERRY_LLVM_LLC",
    "PERRY_LLVM_LLVM_AS",
    "PERRY_LLVM_LLVM_LINK",
    "PERRY_LLVM_NM",
    "PERRY_LLVM_OBJCOPY",
    "PERRY_LLVM_OPT",
    "PERRY_LL_OPT_LEVEL",
    "PERRY_LL_RS4GC_MAX_INSTRS",
    "PERRY_LL_SIZE_OPT",
    "PERRY_LOCKDOWN",
    "PERRY_LOOP_PROPERTY_HOIST",
    "PERRY_MACOS_SYSROOT",
    "PERRY_METHOD_CERT_SABOTAGE",
    "PERRY_METHOD_INLINE_PROBE",
    "PERRY_METHOD_SITE",
    "PERRY_NO_5951",
    "PERRY_NO_AUTO_OPTIMIZE",
    "PERRY_NO_CLASS_ENV",
    "PERRY_NO_REEXPORT_PRUNE",
    "PERRY_NUMBER_BY_CONSTRUCTION",
    "PERRY_NUMBER_LOCAL_LOOP",
    "PERRY_OBJECT_LITERAL_SHAPE_METHODS",
    "PERRY_OUTLINE_ENTRY",
    "PERRY_OUTLINE_ENTRY_CHUNK_STMTS",
    "PERRY_OUTLINE_METHOD_DISPATCH",
    "PERRY_PACKED_LOOP_ABRUPT",
    "PERRY_PACKED_LOOP_NUMERIC_ACCUMULATOR",
    "PERRY_PRECOMPILE_CAPTURE",
    "PERRY_PROGRAM_FACTS",
    "PERRY_PROP_CSE",
    "PERRY_PTR_NUMARRAY_LOCALS",
    "PERRY_PTR_SHAPE_LOCALS",
    "PERRY_PTR_SHAPE_THIS",
    "PERRY_RECEIVER_REPR_DIAG",
    "PERRY_RECV_ROUTE_COUNT",
    "PERRY_REGEX_DIAG",
    "PERRY_REGIONS",
    "PERRY_REGION_JOINT",
    "PERRY_REGION_NODES_PER_BARE",
    "PERRY_REGION_READS",
    "PERRY_REGION_SPILL",
    "PERRY_REQUIRE_FAITHFUL_BINDINGS",
    "PERRY_RUNTIME_DIR",
    "PERRY_SEGVIEW",
    "PERRY_SIZE_LTO",
    "PERRY_SIZE_OPT",
    "PERRY_SIZE_PANIC",
    "PERRY_SPECIALIZED_ABI",
    "PERRY_SPECIALIZED_ABI_MAX",
    "PERRY_SPEC_PRESERVE_NONE",
    "PERRY_STACK_SYMBOLS",
    "PERRY_STATIC_STRING_LOWERING",
    "PERRY_STORE_CENSUS",
    "PERRY_STRING_INIT_CHUNK_SIZE",
    "PERRY_TARGET_CPU",
    "PERRY_TA_PARAM_F64_READ",
    "PERRY_TRANSITION_IC_PROBE",
    "PERRY_TREE_SHAKE",
    "PERRY_TVOS_SYSROOT",
    "PERRY_TYPED_ARRAY_RMW",
    "PERRY_TYPED_BODIES",
    "PERRY_TYPED_FEEDBACK",
    "PERRY_TYPED_FEEDBACK_TRACE",
    "PERRY_U8_INLINE_READ",
    "PERRY_VERIFY_NATIVE_REGIONS",
    "PERRY_VISIONOS_SYSROOT",
    "PERRY_WATCHOS_ARM64_32",
    "PERRY_WATCHOS_MIN",
    "PERRY_WINDOWS_SYSROOT",
    "PERRY_WORKSPACE_ROOT",
    "PERRY_WRITE_BARRIERS",
    "PERRY_XCRUN",
    "PKG_CONFIG_LIBDIR",
    "PKG_CONFIG_PATH",
    "PKG_CONFIG_SYSROOT_DIR",
    "SDKROOT",
];

/// Lossless OS-value encoding shared by both cache keys. None denotes absence;
/// Some("") denotes a present empty value (var_os presence gates distinguish it).
pub(super) fn compile_env_value(name: &str) -> Option<String> {
    use std::fmt::Write;
    std::env::var_os(name).map(|value| {
        let bytes = value.as_encoded_bytes();
        let mut encoded = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            write!(&mut encoded, "{byte:02x}").expect("write environment value");
        }
        encoded
    })
}

#[cfg(test)]
const COMPILE_ENV_EXCLUSIONS: &[(&str, &str)] = &[
    (
        "PERRY_5951_TRACE",
        "Prints lowering diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_8512_CHILD_MODE",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_8512_CONSOLE_FIXTURE",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_A2_FRESH_WORKER_GATE_TEST",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_ALLOC_CENSUS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_ANDROID_KEYSTORE",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_ANDROID_KEYSTORE_PASSWORD",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_ANDROID_KEY_PASSWORD",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_APPLE_CERTIFICATE",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_APPLE_CERTIFICATE_PASSWORD",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_APPLE_INSTALLER_CERTIFICATE_PASSWORD",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_APPLE_NOTARIZE_CERTIFICATE_PASSWORD",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_APP_ID",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_ARRAY_SUBCLASS_ELEMENTS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_B1_NO_GC_PROBE",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_B1_SABOTAGE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_BENCH_SRC_DIR",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_BIGINT_MIX_DIAG",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_BUILD_CACHE_FEATURE_TEST_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_BUILD_CACHE_FEATURE_TEST_EXPECTED",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_BUILD_COMMIT",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_BUILD_WORKSPACE_ROOT",
        "Baked into the compiler by build.rs; compiler fingerprint already covers this hint.",
    ),
    (
        "PERRY_CACHE_CONTRACT_NON_UNICODE_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_CACHE_DEBUG_HIR",
        "Prints cache identity diagnostics only.",
    ),
    (
        "PERRY_CACHE_DEBUG_KEY",
        "Prints cache identity diagnostics only.",
    ),
    (
        "PERRY_CACHE_DIR",
        "Controls cache storage or reuse policy; identical compile inputs emit identical bytes.",
    ),
    (
        "PERRY_CENSUS_TLS_TEST_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_CLASS_CAPTURE_DIAG",
        "Prints lowering diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_CLI_UPDATE_PUBLIC_KEYS",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_CODEGEN_PROGRESS",
        "Controls scheduling or progress telemetry, not codegen partitioning or emitted bytes.",
    ),
    (
        "PERRY_CODEGEN_UNIT_JOBS",
        "Controls scheduling or progress telemetry, not codegen partitioning or emitted bytes.",
    ),
    (
        "PERRY_CODEGEN_UNIT_TIMINGS",
        "Controls scheduling or progress telemetry, not codegen partitioning or emitted bytes.",
    ),
    (
        "PERRY_COLLECT_ONLY",
        "Stops before object emission; the build-cache probe rejects diagnostic/no-link modes.",
    ),
    (
        "PERRY_CONSERVATIVE_STACK_SCAN",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_CONTRACT_TEST_KIND",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_DATE_UTC_GETTER_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_DEBUG",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_DEBUG_CJS_WRAP",
        "Prints lowering diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_DEFOREST_DEBUG",
        "Prints lowering diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_DETERMINISTIC_NET",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_DEV_VERBOSE",
        "Controls scheduling or progress telemetry, not codegen partitioning or emitted bytes.",
    ),
    (
        "PERRY_DIALECT_DUMP",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_DISABLE_BUILD_CACHE",
        "Controls cache storage or reuse policy; identical compile inputs emit identical bytes.",
    ),
    (
        "PERRY_DISPATCH_DIAG",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_DT_PLATFORM_BUILD",
        "Changes Apple bundle metadata only; build-cache eligibility rejects bundled outputs.",
    ),
    (
        "PERRY_DT_PLATFORM_VERSION",
        "Changes Apple bundle metadata only; build-cache eligibility rejects bundled outputs.",
    ),
    (
        "PERRY_CONTRACTS_REPORT",
        "Read by the generated program runtime to print contract statistics; never consulted by compiler lowering.",
    ),
    (
        "PERRY_DT_SDK_BUILD",
        "Changes Apple bundle metadata only; build-cache eligibility rejects bundled outputs.",
    ),
    (
        "PERRY_DT_XCODE",
        "Changes Apple bundle metadata only; build-cache eligibility rejects bundled outputs.",
    ),
    (
        "PERRY_DT_XCODE_BUILD",
        "Changes Apple bundle metadata only; build-cache eligibility rejects bundled outputs.",
    ),
    (
        "PERRY_DXC",
        "Selects resource shader compiler only; resource side effects disable the build cache.",
    ),
    (
        "PERRY_EAGER_FN_METADATA_UTF8",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_EH_TRACE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_EH_WALKER",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_EMIT_ATTEST",
        "Requests sidecar output; sidecar mode disables the build cache.",
    ),
    (
        "PERRY_EMIT_SANDBOX",
        "Requests sidecar output; sidecar mode disables the build cache.",
    ),
    (
        "PERRY_ENV_ALIAS_RUNTIME_TEST_6622",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_ERROR_STACK_DIAG",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_EVAL_DIAG",
        "Prints lowering diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_EXPERIMENTAL_VM_MODULES",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_FACTS_DUMP",
        "Diagnostics/analysis-only output of the program-facts pass; emitted code does not depend on it.",
    ),
    (
        "PERRY_FACTS_ELIDE_TRACE",
        "Diagnostics/analysis-only output of the program-facts pass; emitted code does not depend on it.",
    ),
    (
        "PERRY_FACTS_EXPLAIN",
        "Diagnostics/analysis-only output of the program-facts pass; emitted code does not depend on it.",
    ),
    (
        "PERRY_FACTS_ONLY",
        "Diagnostics/analysis-only output of the program-facts pass; emitted code does not depend on it.",
    ),
    (
        "PERRY_FACTS_PROFILE",
        "Diagnostics/analysis-only output of the program-facts pass; emitted code does not depend on it.",
    ),
    (
        "PERRY_FACTS_RELATIONS",
        "Diagnostics/analysis-only output of the program-facts pass; emitted code does not depend on it.",
    ),
    (
        "PERRY_FACTS_REPORT",
        "Diagnostics/analysis-only output of the program-facts pass; emitted code does not depend on it.",
    ),
    (
        "PERRY_FACTS_WHATIF_NO_E14",
        "Diagnostics/analysis-only output of the program-facts pass; emitted code does not depend on it.",
    ),
    (
        "PERRY_FAKE_XCRUN_LOG",
        "Test-only fake xcrun logging in target discovery tests.",
    ),
    (
        "PERRY_FETCH_DIAG",
        "Prints lowering diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_FNINFER_DUMP",
        "Writes the fninfer precision dump; emitted code does not depend on it.",
    ),
    (
        "PERRY_FNINFER_ONLY",
        "Exits after writing the fninfer dump (analysis-only run); produces no object.",
    ),
    (
        "PERRY_FNINFER_REPORT",
        "Prints the count of applied parameter types; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_FNINFER_TRACE",
        "Prints fninfer solver diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_FORCE_LINK_RESPONSE_FILE",
        "Changes linker argument transport only, preserving the argument vector.",
    ),
    (
        "PERRY_FORIN_LAZY_SHADOW",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_FORK_EXECPATH",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_FS_WATCH_POLL",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_FUNCTION_ORDER_OUT",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_DIAG",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_GC_HEAP_LIMIT",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_INTERP_SAFEPOINTS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_MAJOR_PACING_FLOOR_MB",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_MAJOR_PACING_GROWTH",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_NATIVE_SLOT_VERIFY",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_OLD_DEFRAG",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_POISON_FROMSPACE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_PROMOTE_IN_PLACE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_PROTECT_FROMSPACE_DEPTH",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_PROTECT_FROMSPACE_HOLDERS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_SCAVENGE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_SCAVENGE_NURSERY_MB",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_SCHEDULE_ALLOC_KB",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_SCHEDULE_RATE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GC_TENURING_SURVIVALS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_GLSLC",
        "Selects resource shader compiler only; resource side effects disable the build cache.",
    ),
    (
        "PERRY_HARMONYOS_BUNDLE_NAME",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_HARMONYOS_CERT",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_HARMONYOS_HAPSIGN",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_HARMONYOS_KEY_ALIAS",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_HARMONYOS_KEY_PASSWORD",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_HARMONYOS_P12",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_HARMONYOS_P12_PASSWORD",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_HARMONYOS_PROFILE",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_HARMONYOS_SIGN_ALG",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_I18N_DEBUG",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_IC_OUTLINE_FASTPATH",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_INLINE_DEBUG",
        "Prints lowering diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_ITER_BT",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_JSON_TAPE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_KEYADD_IC",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_LAZY_STACK_MAPS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_LEARNED_INLINE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_LIB_CACHE_DIR",
        "Controls cache storage or reuse policy; identical compile inputs emit identical bytes.",
    ),
    (
        "PERRY_LICENSE_KEY",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_LINK_MAP",
        "Requests a linker map sidecar without changing linked program instructions.",
    ),
    (
        "PERRY_LLVM_DIFF_DIR",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_LLVM_KEEP_IR",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_LOCK_FROZEN",
        "Controls lock-file validation or persistence, not lowering of identical resolved inputs.",
    ),
    (
        "PERRY_LOCK_UPDATE",
        "Controls lock-file validation or persistence, not lowering of identical resolved inputs.",
    ),
    (
        "PERRY_LOOP_STATS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_MAIN_STACK_MB",
        "Sets the compiler process stack size only.",
    ),
    (
        "PERRY_METHOD_SITE_STATS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_MODULE_JOBS",
        "Controls scheduling or progress telemetry, not codegen partitioning or emitted bytes.",
    ),
    (
        "PERRY_MT_PROFILE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_NATIVEINST_DIAG",
        "Prints lowering diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_NATIVE_REPS",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_NATIVE_REPS_DIR",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_NODE_TARGET",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_NO_ARCHIVE_CACHE",
        "Controls cache storage or reuse policy; identical compile inputs emit identical bytes.",
    ),
    (
        "PERRY_NO_CACHE",
        "Controls cache storage or reuse policy; identical compile inputs emit identical bytes.",
    ),
    (
        "PERRY_NO_CLANG_PROBE",
        "Skips tool-capability probing; selected tool still keys both caches.",
    ),
    (
        "PERRY_NO_LINK_CACHE",
        "Controls cache storage or reuse policy; identical compile inputs emit identical bytes.",
    ),
    (
        "PERRY_NO_TELEMETRY",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_NO_UPDATE_CHECK",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_OBJECT_CACHE_BUILD_ID",
        "Explicit build-identity override already hashed in the object key build_id field.",
    ),
    (
        "PERRY_OBJECT_DICTIONARY_LAYOUT_ID_BUDGET",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_OBJECT_DICTIONARY_MIN_KEYS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_OBJECT_TOMBSTONES",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_OPT_REPORT",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_OUTLINE_ENTRY_REPORT",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_PACKED_LOOP_TRACE",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_PLAN_DIAG",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_PROCESS_EXEC_ARGV",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_R2_SABOTAGE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_REGION_DIAG",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_REJECTION_DIAG",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_RELEASE_STRIP_INSTRUMENTS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_REPSEL_DEBUG",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_RUNTIME_BUILD_ID",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_RUNTIME_ISOLATED_TEST_NAME",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_RUNTIME_TARGET",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_SANDBOX_BUILDRS",
        "Restricts external build-script permissions; successful script output is unchanged.",
    ),
    (
        "PERRY_SAVE_LL",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_SEGVIEW_DIAG",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_SETTER_SITE_STATS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_SFW",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_SHAPE_FACT_AUDIT",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_SKIP_CODEGEN",
        "Stops before object emission; the build-cache probe rejects diagnostic/no-link modes.",
    ),
    (
        "PERRY_SPIN_THROTTLE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_SSO_FORCE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_STACKMAP_WALKER",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_STATEPOINT_REPORT",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
    (
        "PERRY_STATIC_SHAPE_CENSUS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_STATIC_SHAPE_IDS_REPORT",
        "Prints lowering diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_STRICT_STUBS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_STUB_DIAG",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_SUPER_DEBUG",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_SYNC_PROGRAM_DIAG",
        "Prints lowering diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_TEST_BUNDLE_LAUNCH_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_BUN_CONDITIONS_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_CALLBACK_SABOTAGE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TEST_CONSTFN_ALIAS_KEYS_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_EMITTER_INLINE_ONLY",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TEST_GC_DIAG_PARSE_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_LIFECYCLE_SABOTAGE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TEST_LOCAL_RUNTIME_EXPECTED",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_LOCAL_RUNTIME_TARGET",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_METHOD_DIRECT_SHAPE_PAIR_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_NET_SABOTAGE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TEST_PAYLOAD_BUFFER_SABOTAGE",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_PIN_LATCH_SABOTAGE",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_PLAIN_STATIC_REFUSAL_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_REGEXP_BUILTIN_SHAPE_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_RESTORE_COVERAGE_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_SPLIT_REGEXP_SITE_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_STACK_SYMBOLS_NM_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_TEST_STREAM_LITERAL_KEY",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TEST_STREAM_MODE_BY_NAME",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TEST_STREAM_MODE_REREAD",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TEST_STREAM_SABOTAGE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TEST_VERIFY_PASS_LINE_CHILD",
        "Test-only fixture/child-process control; absent from compiler production paths.",
    ),
    (
        "PERRY_THENABLE_VERIFY",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TLS_HOT_STATS",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TRACE_ASYNC",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TRACE_OBJECT_ARRAY_WRITE_GUARD",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TRACE_RELOWER",
        "Prints lowering diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_TREE_SHAKE_DIAG",
        "Prints lowering diagnostics; no emitted instructions depend on the report.",
    ),
    (
        "PERRY_TTY_TEST_PIPED_CHILD",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_TYPED_BODIES_LOG",
        "Writes the typed-body clone log; no emitted instructions depend on the log.",
    ),
    (
        "PERRY_TYPED_ELEMENT_SABOTAGE",
        "Test-only typed-element negative control under cfg(test).",
    ),
    (
        "PERRY_UNCAUGHT_BACKTRACE",
        "Read by the generated program runtime, never consulted by compiler lowering.",
    ),
    (
        "PERRY_UPDATE_MODE",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_UPDATE_SERVER",
        "Used by standalone CLI/update/publish commands, outside the compile pipeline.",
    ),
    (
        "PERRY_WASM_DEBUG_IMPORTS",
        "Writes IR/report files or diagnostic output without changing emitted objects.",
    ),
];

#[cfg(test)]
mod tests;
