use super::*;

// #6563: node-pty / @lydell/node-pty — `spawn` is the entire module surface
// the target apps touch; everything else lives on the returned IPty object.
#[export_name = "perry_nm_vt_NM_NODE_PTY_DEFAULT"]
pub(crate) static NM_NODE_PTY_DEFAULT: NativeModuleVtable = NativeModuleVtable {
    base_namespace: Some(|| &NM_NODE_PTY),
    ..keyed(&[b"spawn"])
};

#[export_name = "perry_nm_vt_NM_NODE_PTY"]
pub(crate) static NM_NODE_PTY: NativeModuleVtable = NativeModuleVtable {
    default_namespace: Some(&NM_NODE_PTY_DEFAULT),
    ..keyed(&[b"default", b"spawn"])
};

/// A vtable for a module whose keys are still a hand table beside it (no
/// item-level declaration yet).
const fn keyed(module_keys: &'static [&'static [u8]]) -> NativeModuleVtable {
    NativeModuleVtable {
        module_keys,
        ..NATIVE_MODULE_VTABLE_BASE
    }
}

/// The vtable a `vm` namespace is born with under
/// `PERRY_EXPERIMENTAL_VM_MODULES` (the flag is fixed for the process).
pub(crate) fn vm_modules_vtable() -> &'static NativeModuleVtable {
    &NM_VM_MODULES
}

/// `vm` with `PERRY_EXPERIMENTAL_VM_MODULES`: the declared `vm` keys plus the
/// module classes (the birth picks it; the flag is fixed for the process).
#[export_name = "perry_nm_vt_NM_VM_MODULES"]
pub(crate) static NM_VM_MODULES: NativeModuleVtable = keyed(&[
    b"Script",
    b"createContext",
    b"createScript",
    b"runInContext",
    b"runInNewContext",
    b"runInThisContext",
    b"isContext",
    b"compileFunction",
    b"measureMemory",
    b"constants",
    b"Module",
    b"SourceTextModule",
    b"SyntheticModule",
]);

/// The deprecated `constants` module: `constants.default` plus `default`.
#[export_name = "perry_nm_vt_NM_CONSTANTS"]
pub(crate) static NM_CONSTANTS: NativeModuleVtable = NativeModuleVtable {
    keys: |_| {
        Some(ModuleKeys {
            head: super::namespaces::constants::NM_CONSTANTS_DEFAULT.module_keys,
            tail: &[b"default"],
        })
    },
    default_namespace: Some(&super::namespaces::constants::NM_CONSTANTS_DEFAULT),
    ..NATIVE_MODULE_VTABLE_BASE
};

// The other half of the `constants` default link (this vtable is a hand table,
// so it has no `default = ...;` clause to set it).
impl super::namespaces::constants::NM_CONSTANTS_DEFAULT {
    pub const BASE_NAMESPACE: Option<fn() -> &'static NativeModuleVtable> = Some(|| &NM_CONSTANTS);
}

#[cfg(test)]
mod tests {
    #[test]
    fn rtld_deepbind_key_is_platform_gated() {
        let has_rtld_deepbind = super::super::namespaces::constants::NM_CONSTANTS_DEFAULT
            .module_keys
            .iter()
            .any(|key| *key == b"RTLD_DEEPBIND");
        assert_eq!(
            has_rtld_deepbind,
            cfg!(all(target_os = "linux", target_env = "gnu"))
        );
    }
}

// Node 26.5.1 enumerable exports; deprecated top-level constants are
// non-enumerable. All enumeration and interop copies use this surface.

#[export_name = "perry_nm_vt_NM_SQLITE_CONSTANTS"]
pub(crate) static NM_SQLITE_CONSTANTS: NativeModuleVtable = keyed(&[
    b"SQLITE_CHANGESET_DATA",
    b"SQLITE_CHANGESET_NOTFOUND",
    b"SQLITE_CHANGESET_CONFLICT",
    b"SQLITE_CHANGESET_CONSTRAINT",
    b"SQLITE_CHANGESET_FOREIGN_KEY",
    b"SQLITE_CHANGESET_OMIT",
    b"SQLITE_CHANGESET_REPLACE",
    b"SQLITE_CHANGESET_ABORT",
    b"SQLITE_OK",
    b"SQLITE_DENY",
    b"SQLITE_IGNORE",
    b"SQLITE_CREATE_INDEX",
    b"SQLITE_CREATE_TABLE",
    b"SQLITE_CREATE_TEMP_INDEX",
    b"SQLITE_CREATE_TEMP_TABLE",
    b"SQLITE_CREATE_TEMP_TRIGGER",
    b"SQLITE_CREATE_TEMP_VIEW",
    b"SQLITE_CREATE_TRIGGER",
    b"SQLITE_CREATE_VIEW",
    b"SQLITE_DELETE",
    b"SQLITE_DROP_INDEX",
    b"SQLITE_DROP_TABLE",
    b"SQLITE_DROP_TEMP_INDEX",
    b"SQLITE_DROP_TEMP_TABLE",
    b"SQLITE_DROP_TEMP_TRIGGER",
    b"SQLITE_DROP_TEMP_VIEW",
    b"SQLITE_DROP_TRIGGER",
    b"SQLITE_DROP_VIEW",
    b"SQLITE_INSERT",
    b"SQLITE_PRAGMA",
    b"SQLITE_READ",
    b"SQLITE_SELECT",
    b"SQLITE_TRANSACTION",
    b"SQLITE_UPDATE",
    b"SQLITE_ATTACH",
    b"SQLITE_DETACH",
    b"SQLITE_ALTER_TABLE",
    b"SQLITE_REINDEX",
    b"SQLITE_ANALYZE",
    b"SQLITE_CREATE_VTABLE",
    b"SQLITE_DROP_VTABLE",
    b"SQLITE_FUNCTION",
    b"SQLITE_SAVEPOINT",
    b"SQLITE_COPY",
    b"SQLITE_RECURSIVE",
]);

#[export_name = "perry_nm_vt_NM_SQLITE"]
pub static NM_SQLITE: NativeModuleVtable = keyed(&[
    b"Database",
    b"DatabaseSync",
    b"Session",
    b"Statement",
    b"StatementSync",
    b"backup",
    b"constants",
    b"default",
]);

#[export_name = "perry_nm_vt_NM_BUN_JSC"]
pub(crate) static NM_BUN_JSC: NativeModuleVtable = keyed(&[b"heapStats"]);

#[export_name = "perry_nm_vt_NM_BUN"]
pub(crate) static NM_BUN: NativeModuleVtable = keyed(&[
    b"Glob",
    b"JSONL",
    b"TOML",
    b"Terminal",
    b"Transpiler",
    b"YAML",
    b"ant",
    b"build",
    b"connect",
    b"deepEquals",
    b"file",
    b"fileURLToPath",
    b"gc",
    b"generateHeapSnapshot",
    b"hash",
    b"isStandaloneExecutable",
    b"listen",
    b"pathToFileURL",
    b"plugin",
    b"semver",
    b"serve",
    b"sleep",
    b"sleepSync",
    b"spawn",
    b"stderr",
    b"stdin",
    b"stdout",
    b"stringWidth",
    b"stripANSI",
    b"version",
    b"which",
    b"wrapAnsi",
    b"write",
    b"zstdDecompress",
    b"zstdDecompressSync",
]);

#[export_name = "perry_nm_vt_NM_BUN_ANT"]
pub(crate) static NM_BUN_ANT: NativeModuleVtable =
    keyed(&[b"getPeerPid", b"getPeerUid", b"memoryPressureLevel"]);

#[export_name = "perry_nm_vt_NM_BUN_FFI"]
pub(crate) static NM_BUN_FFI: NativeModuleVtable = keyed(&[
    b"dlopen",
    b"FFIType",
    b"ptr",
    b"CString",
    b"suffix",
    b"toArrayBuffer",
    b"toBuffer",
    b"JSCallback",
    b"CFunction",
    b"linkSymbols",
    b"viewSource",
    b"read",
]);

// `ffi.default` is the `ffi` namespace itself (so no base link: it is its own base).
#[export_name = "perry_nm_vt_NM_FFI"]
pub(crate) static NM_FFI: NativeModuleVtable = NativeModuleVtable {
    default_namespace: Some(&NM_FFI),
    ..keyed(&[
        b"dlopen",
        b"getRawPointer",
        b"toArrayBuffer",
        b"toBuffer",
        b"toString",
        b"suffix",
    ])
};

/// Enumerability of ZLIB's read-only own data properties. Non-enumerable
/// legacy aliases are still own properties: descriptor lookup and assignment
/// must use this policy before the enumerable-export and override paths.
/// Node exposes all constants except the BROTLI_* family as legacy aliases.
/// Keep values and insertion order in the existing constants table.
pub(crate) fn zlib_readonly_property_enumerability(module_name: &str, key: &str) -> Option<bool> {
    match module_name {
        "zlib" if matches!(key, "codes" | "constants") => Some(true),
        "zlib"
            if !key.starts_with("BROTLI_")
                && super::namespaces::zlib::NM_ZLIB_CONSTANTS
                    .module_keys
                    .contains(&key.as_bytes()) =>
        {
            Some(false)
        }
        "zlib.constants"
            if super::namespaces::zlib::NM_ZLIB_CONSTANTS
                .module_keys
                .contains(&key.as_bytes()) =>
        {
            Some(true)
        }
        _ => None,
    }
}

pub(crate) fn native_module_has_enumerable_key(
    vt: &'static NativeModuleVtable,
    module_name: &str,
    key: &str,
) -> bool {
    if super::native_namespace_member_absent(module_name, key) {
        return false;
    }
    if matches!(
        module_name,
        "process" | "process.namespace" | "process.default"
    ) && key == "permission"
    {
        return crate::process::process_permission_enabled();
    }
    (vt.keys)(vt).is_some_and(|keys| keys.contains(key.as_bytes()))
}
