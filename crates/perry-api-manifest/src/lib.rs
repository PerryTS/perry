//! Source-of-truth manifest of stdlib / native APIs Perry implements.
//!
//! Three consumers:
//!
//! - **perry-hir** consults [`module_has_symbol`] during HIR lowering to
//!   reject references to unimplemented APIs at compile time (#463).
//! - **perry-codegen** keeps its native dispatch table aligned with this
//!   manifest via a CI test (`tests/manifest_consistency.rs`) — the
//!   manifest is the entry list, codegen owns the dispatch metadata.
//! - **perry's docs / .d.ts emit** iterates entries to produce an
//!   external view of the supported surface (#465).
//!
//! The schema is also the foundation for #466 Phase 2's external
//! `perry.nativeLibrary` manifest spec — third-party native bindings
//! will declare entries with the same shape, just `source: External`
//! instead of `Stdlib`.

#![deny(missing_docs)]

mod emit;
mod entries;
mod native_abi;

pub use emit::{emit_dts, emit_markdown};
pub use entries::{API_MANIFEST, NATIVE_MODULES, NODE_SUBMODULES, RUNTIME_ONLY_MODULES};
pub use native_abi::{
    native_handle_type_id, NativeAbiParseError, NativeAbiType, NativeHandleAbi,
    NativeHandleOwnership, NativeHandleThreadAffinity, NativePodAbi, NativePodFieldAbi,
    NativePromiseAbi, NativePromiseCompletion, NativePromiseThread,
};

/// One entry in the manifest. Identifies a single named symbol on a
/// known module — a method, a property, or a class.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ApiEntry {
    /// Module specifier (without the `node:` prefix).
    /// Example: `"crypto"`, `"fs"`, `"perry/ui"`, `"@perry/iroh"`.
    pub module: &'static str,
    /// Symbol name on the module.
    /// For methods and properties this is the bare identifier.
    /// For classes it's the class name.
    pub name: &'static str,
    /// What kind of symbol this is.
    pub kind: ApiKind,
    /// Where the implementation lives. Today nearly everything is
    /// [`ApiSource::Stdlib`]; #466 Phase 5 migrates some entries to
    /// [`ApiSource::WellKnown`] without changing user-visible behavior.
    pub source: ApiSource,
    /// Intentional no-op stubs (platform-gated UI symbols, etc.) are
    /// flagged so the unimplemented-API check (#463) does NOT error on
    /// them — the runtime first-call warning from #464 surfaces those.
    pub stub: bool,
    /// ABI version this entry was published against. `None` for
    /// `Stdlib` source — the bundled stdlib is built and shipped
    /// together with the compiler, so its ABI moves in lockstep.
    /// Required for `External` source under #466 Phase 2.
    pub abi_version: Option<&'static str>,
    /// Declared parameter list for this method (#512). Empty for
    /// properties, classes, and methods whose param shape hasn't been
    /// backfilled — emit code falls back to `(...args: any[])` in that
    /// case so editors don't reject working calls. Auto-derived from
    /// `NATIVE_MODULE_TABLE` for module-level (no-receiver, no
    /// class_filter) rows; instance methods stay loose for now since
    /// the receiver-binding shape varies per dispatch path.
    pub params: &'static [ParamSpec],
    /// Declared return type. [`TypeSpec::Any`] means "fall back to the
    /// loose `any` rendering"; concrete values let the .d.ts emitter
    /// produce a typed signature. (#512)
    pub returns: TypeSpec,
}

/// One parameter slot on a method entry. Mirrors the param-type
/// vocabulary in `docs/src/native-libraries/manifest-v1.md` so the
/// in-tree manifest and external `perry.nativeLibrary` manifests share
/// one type model.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum ParamSpec {
    /// A named, typed positional parameter (`name: ty`).
    Named {
        /// Parameter name. Auto-derived params use `p0`/`p1`/... — the
        /// dispatch table doesn't carry user-facing names.
        name: &'static str,
        /// Parameter type.
        ty: TypeSpec,
        /// True when the param is optional (`name?: ty`).
        optional: bool,
    },
    /// A trailing rest parameter (`...name: ty[]`).
    Rest {
        /// Parameter name.
        name: &'static str,
        /// Element type.
        ty: TypeSpec,
    },
}

/// Reduced type vocabulary the manifest uses for parameter and return
/// types. Mirrors the param/return type strings in
/// `docs/src/native-libraries/manifest-v1.md` so the in-tree manifest
/// and external `perry.nativeLibrary` manifests share one model.
///
/// Deliberately small. The dispatch table doesn't carry per-arg
/// TypeScript types (it's a Rust-ABI table); the manifest's job is to
/// say "this slot is a string vs. a number vs. opaque" with enough
/// fidelity that the `.d.ts` catches obvious mismatches like
/// `bcrypt.hash(123, "salt")`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind", rename_all = "camelCase"))]
pub enum TypeSpec {
    /// `string`.
    String,
    /// `number`.
    Number,
    /// `boolean`.
    Bool,
    /// `bigint`.
    BigInt,
    /// `Buffer` (Node Buffer / `Uint8Array`-shaped opaque).
    Buffer,
    /// `any` — opaque handle; the runtime shape varies and is not
    /// expressible in a useful TypeScript type today.
    Handle,
    /// `void`. Used for return slots when the runtime ignores the
    /// return value (`I32Void`/`Void`).
    Void,
    /// `any`. Default when no specific type fits or the dispatch path
    /// returns a NaN-boxed JSValue whose user-side type isn't fixed
    /// (`F64` returns can be `Promise<T>`, mixed strings/numbers, etc.).
    Any,
}

/// What shape of symbol this entry describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind"))]
pub enum ApiKind {
    /// A function/method on the module.
    Method {
        /// True for instance methods (`db.query(...)`); false for
        /// receiver-less calls (`crypto.randomUUID()`).
        has_receiver: bool,
        /// Optional class filter — when `Some("Pool")`, only matches
        /// instance methods whose receiver was constructed via that
        /// class. Mirrors `NativeModSig::class_filter` in codegen.
        class_filter: Option<&'static str>,
    },
    /// A constant or accessor property (`os.EOL`, `path.sep`).
    Property,
    /// A class exported by the module (`Buffer` on `"buffer"`,
    /// `Pool` on `"mysql2/promise"`).
    Class,
}

/// Where the implementation backing an entry lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub enum ApiSource {
    /// `perry-stdlib` or `perry-runtime` ships the implementation.
    Stdlib,
    /// A bundled wrapper crate registered in the well-known bindings
    /// table (#466 Phase 4). User imports the bare name (`mysql2`),
    /// resolution lands here. Same observable behavior as `Stdlib`.
    WellKnown,
    /// A third-party `node_modules/<pkg>/package.json` declares
    /// `perry.nativeLibrary` and provides the implementation (#466).
    External,
    /// Compiler-emitted symbol — no Rust function backs it. Currently
    /// unused; reserved for things like `import.meta.{url,dirname}`
    /// which the HIR lowers to literals at parse time.
    Intrinsic,
}

/// Look up `name` on `module` in the manifest. Strips a leading
/// `node:` prefix so callers don't have to. Returns the entry on hit.
///
/// Used by HIR lowering to gate property access against
/// `Expr::NativeModuleRef` — unknown lookups become a compile error
/// (#463).
pub fn module_has_symbol(module: &str, name: &str) -> Option<&'static ApiEntry> {
    let module = module.strip_prefix("node:").unwrap_or(module);
    // Match either:
    //  - a top-level export by name (`ethers.parseEther` → entry.name = parseEther)
    //  - any method whose class_filter is the requested name (`ethers.Wallet`
    //    → some entry has Method { class_filter: Some("Wallet") }). Without
    //    this branch, `ethers.Wallet.createRandom()` failed the #463
    //    unimplemented gate even though `createRandom` was registered with
    //    class_filter=Wallet.
    API_MANIFEST.iter().find(|e| {
        if e.module != module {
            return false;
        }
        if e.name == name {
            return true;
        }
        matches!(
            e.kind,
            ApiKind::Method { class_filter: Some(c), .. } if c == name
        )
    })
}

/// True if `path` resolves to a Perry-implemented native module.
/// Strips `node:` prefix. This is the migrated home of the
/// `is_native_module` check that previously lived in
/// `perry-hir::ir`.
pub fn is_known_module(path: &str) -> bool {
    let normalized = path.strip_prefix("node:").unwrap_or(path);
    NATIVE_MODULES.contains(&normalized) || NODE_SUBMODULES.contains(&normalized)
}

/// True if `module` is handled entirely by `perry-runtime` (no
/// `perry-stdlib` link required). Used by the linker's auto-feature
/// detection — modules in this list don't enable any
/// `perry-stdlib` cargo feature.
pub fn is_runtime_only_module(module: &str) -> bool {
    let normalized = module.strip_prefix("node:").unwrap_or(module);
    RUNTIME_ONLY_MODULES.contains(&normalized)
}

/// Iterate every entry in the manifest. Stable order: matches the
/// declaration order in `entries.rs`. Useful for the `--print-api-manifest`
/// CLI flag (#465 starter).
pub fn iter_entries() -> impl Iterator<Item = &'static ApiEntry> {
    API_MANIFEST.iter()
}

/// Returns true if the manifest has at least one entry on `module`.
///
/// Used by the unimplemented-API check (#463) to gate strictness:
/// modules with at least one entry have all property accesses
/// validated; modules with zero entries fall through to existing
/// permissive behavior so that incremental coverage doesn't break
/// unrelated working code. Strips `node:` prefix.
pub fn module_has_any_entries(module: &str) -> bool {
    let module = module.strip_prefix("node:").unwrap_or(module);
    API_MANIFEST.iter().any(|e| e.module == module)
}

/// Iterate entries on a specific module. Useful for the docs serializer.
pub fn entries_for_module(module: &str) -> impl Iterator<Item = &'static ApiEntry> {
    let module = module.strip_prefix("node:").unwrap_or(module).to_string();
    API_MANIFEST
        .iter()
        .filter(move |e| e.module == module.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_strips_node_prefix() {
        // Whatever `crypto.randomUUID` resolves to in the real manifest,
        // it must resolve identically under `node:crypto`.
        let bare = module_has_symbol("crypto", "randomUUID");
        let prefixed = module_has_symbol("node:crypto", "randomUUID");
        assert_eq!(bare.is_some(), prefixed.is_some());
    }

    #[test]
    fn buffer_inspect_max_bytes_is_manifest_property() {
        let entry = module_has_symbol("node:buffer", "INSPECT_MAX_BYTES")
            .expect("buffer.INSPECT_MAX_BYTES should be in the manifest");
        assert!(matches!(entry.kind, ApiKind::Property));
    }

    #[test]
    fn util_is_array_is_manifest_method() {
        let entry = module_has_symbol("node:util", "isArray")
            .expect("util.isArray should be in the manifest");
        assert!(matches!(
            entry.kind,
            ApiKind::Method {
                has_receiver: false,
                class_filter: None
            }
        ));
        assert_eq!(entry.params.len(), 1);
        assert!(matches!(entry.returns, TypeSpec::Bool));
    }

    #[test]
    fn known_modules_consistent_with_manifest() {
        // Every entry's module must appear in NATIVE_MODULES.
        // Catches typos and entries on un-registered modules.
        for entry in API_MANIFEST {
            assert!(
                is_known_module(entry.module),
                "manifest entry {}::{} on unknown module",
                entry.module,
                entry.name
            );
        }
    }
}
