//! The program's runtime provider set, computed from HIR before codegen
//! (design GC_EFFECTS_AT_BUILD_DESIGN_2026-10-15 §4.4, slice 4).
//!
//! Which archives a program links (perry-stdlib and its features, the
//! `perry-ext-*` wrappers) is a function of which [`crate::ext_registry`]
//! symbols codegen can emit, and lowering picks those symbols from HIR. This
//! pass reads HIR and asks the lowering's own selectors which symbols each
//! native surface lowers to:
//!
//! - a native module call (`NativeMethodCall`): the dispatch-table row
//!   lowering picks for that exact call ([`crate::lower_call::native_lookup_runtime`]),
//!   and the same for a method call on a native receiver
//!   (`early_branches::native_receiver_module`);
//! - every native module whose namespace codegen may materialize (a
//!   namespace reference, an imported binding read as a value,
//!   `require("x")` / `import("x")` by name, a namespace re-export): the
//!   install hook the routing picks for it
//!   ([`crate::NativeRouting::install_symbol`]);
//! - a native import of a wrapper-served module: its binding (which the
//!   import-driven flip links already);
//! - the entry prologue's wrapper install hooks (the driver's list);
//! - an `ExternFuncRef` naming a registry symbol;
//! - the native surfaces lowering handles in dedicated arms rather than the
//!   table ([`hand_lowered_owner`]).
//!
//! Each symbol maps to its owner through [`crate::ext_registry::symbol_owners`],
//! the same function codegen's check uses. The pass may over-approximate
//! (costing archive size only) but must never miss: codegen checks every
//! emitted registry symbol against the result
//! ([`crate::ext_registry::check_emitted_providers`]), so a miss is a compile
//! error naming the symbol, never a silent link change.

use crate::ext_registry::{symbol_owners, OwnerKind, ProviderSet};
use crate::native_routing::NativeRouting;
use perry_hir::{Expr, Module};

/// Inputs the driver already has before codegen.
pub struct ProviderInputs<'a> {
    pub routing: &'a NativeRouting,
    /// Wrapper install hooks the entry prologue calls (the driver's
    /// `native_provider_installs`).
    pub entry_install_hooks: &'a [String],
    /// Feature detection found a crypto builtin or the global WebCrypto
    /// namespace (`ctx.uses_crypto_builtins`): the HIR crypto variants and
    /// `globalThis.crypto` property reads lower into perry-stdlib's crypto
    /// module.
    pub uses_crypto_builtins: bool,
}

/// The providers every module's codegen may call into.
pub fn program_providers<'a>(
    modules: impl IntoIterator<Item = &'a Module>,
    inputs: &ProviderInputs<'_>,
) -> ProviderSet {
    let mut walk = Walk::default();
    for hook in inputs.entry_install_hooks {
        add_symbol(&mut walk.set, hook);
    }
    if inputs.uses_crypto_builtins {
        walk.set.insert(CRYPTO);
    }
    for module in modules {
        walk.native_imports.clear();
        for import in &module.imports {
            if import.is_native {
                walk.native_modules.insert(import.source.clone());
                // A wrapper-served module the program imports is linked by
                // the import-driven flip already; naming its binding keeps the
                // set a superset of what that module's hand-lowered arms emit
                // (`early_branches` `server.close`, the net socket ABI arms).
                let bare = import
                    .source
                    .strip_prefix("node:")
                    .unwrap_or(&import.source);
                if let Some(key) = crate::ext_registry::wrapper_binding(bare) {
                    walk.set.insert(OwnerKind::WellKnown(key));
                }
            }
            for spec in &import.specifiers {
                let local = match spec {
                    perry_hir::ImportSpecifier::Named { imported, local } => {
                        // An aliased built-in import (`import { WebSocket as
                        // W }`) reaches the constructor arm under its original
                        // name (`imported_class_original_names` in `lower_new`).
                        hand_lowered(&mut walk.set, Surface::Global(imported));
                        local
                    }
                    perry_hir::ImportSpecifier::Default { local }
                    | perry_hir::ImportSpecifier::Namespace { local } => local,
                };
                if import.is_native {
                    walk.native_imports
                        .insert(local.clone(), import.source.clone());
                }
            }
        }
        // `export * as ns from "net"` materializes the namespace
        // (`codegen/native_namespace_exports.rs`, dynamic-import entries).
        for export in &module.exports {
            if let perry_hir::Export::NamespaceReExport { source, .. } = export {
                if perry_hir::is_native_module_specifier(source) {
                    walk.native_modules.insert(source.clone());
                    walk.materialized.insert(source.clone());
                }
            }
        }
        for_each_expr(module, &mut |expr| walk.visit(expr));
    }
    let Walk {
        mut set,
        native_modules,
        materialized,
        ..
    } = walk;
    // Codegen calls the install hook the routing picks for a native module
    // wherever it materializes that module's namespace
    // (`nm_install::nm_install_symbol` callers).
    for module in &materialized {
        let bare = module.strip_prefix("node:").unwrap_or(module);
        if let Some(install) = inputs.routing.install_symbol(bare) {
            add_symbol(&mut set, install);
        }
    }
    for module in &native_modules {
        let bare = module.strip_prefix("node:").unwrap_or(module);
        hand_lowered(&mut set, Surface::Module(module));
        hand_lowered(&mut set, Surface::Module(bare));
    }
    set
}

#[derive(Default)]
struct Walk {
    set: ProviderSet,
    /// Every native module the program names.
    native_modules: std::collections::BTreeSet<String>,
    /// The native modules whose namespace codegen may materialize.
    materialized: std::collections::BTreeSet<String>,
    /// This module's native-import locals → their source
    /// (`imported_class_sources` in codegen).
    native_imports: std::collections::HashMap<String, String>,
}

fn add_symbol(set: &mut ProviderSet, symbol: &str) {
    if let Some(owners) = symbol_owners(symbol) {
        set.extend(owners.iter());
    }
}

fn hand_lowered(set: &mut ProviderSet, surface: Surface<'_>) {
    if let Some(owner) = hand_lowered_owner(&surface) {
        set.insert(owner);
    }
}

impl Walk {
    fn visit(&mut self, expr: &Expr) {
        let set = &mut self.set;
        match expr {
            // The table row lowering picks for this exact call
            // (`lower_native_method_call` → `native_module_lookup`).
            Expr::NativeMethodCall {
                module,
                class_name,
                method,
                object,
                ..
            } => {
                self.native_modules.insert(module.clone());
                if let Some(runtime) = crate::lower_call::native_lookup_runtime(
                    module,
                    object.is_some(),
                    method,
                    class_name.as_deref(),
                ) {
                    add_symbol(set, runtime);
                }
            }
            // `early_branches::try_lower_native_chain_method_call`: a method
            // call on a native receiver it can see through.
            Expr::Call { callee, args, .. } => {
                // `require("net")` / `process.getBuiltinModule("net")` by name
                // (`lower_call/mod.rs`).
                if matches!(callee.as_ref(), Expr::ExternFuncRef { .. }) {
                    if let Some(Expr::String(module)) = args.first() {
                        self.materialized.insert(module.clone());
                    }
                }
                if let Expr::PropertyGet {
                    object, property, ..
                } = callee.as_ref()
                {
                    // `expr/calls.rs`: `ReadableStream.from(iterable)`.
                    if property == "from"
                        && matches!(object.as_ref(), Expr::ExternFuncRef { name, .. } if name == "ReadableStream")
                    {
                        set.insert(STREAMS);
                    }
                    if let Some(module) = crate::lower_call::native_receiver_module(object) {
                        if let Some(runtime) =
                            crate::lower_call::native_lookup_runtime(module, true, property, None)
                        {
                            add_symbol(set, runtime);
                        }
                    }
                }
            }
            // `ns.x`, `new ns.X()`, static members: the namespace value
            // (`expr/property_get.rs`, `expr/new_dynamic.rs`,
            // `expr/static_field_meta.rs`).
            Expr::NativeModuleRef(name) => {
                self.native_modules.insert(name.clone());
                self.materialized.insert(name.clone());
            }
            // An imported native binding read as a value
            // (`expr/dyn_extern_i18n.rs`).
            Expr::ExternFuncRef { name, .. } => {
                add_symbol(set, name);
                if let Some(source) = self.native_imports.get(name) {
                    self.materialized.insert(source.clone());
                }
            }
            // `import("node:net")` (`__native_mod__` dynamic-import targets).
            Expr::DynamicImport { paths, .. } => {
                self.materialized.extend(paths.iter().cloned());
            }
            Expr::New { class_name, .. } => hand_lowered(set, Surface::Global(class_name)),
            Expr::ClassRef(name) => hand_lowered(set, Surface::Global(name)),
            // `new globalThis.X()`, `new net.BlockList()`: `expr/new_dynamic.rs`
            // reroutes a member callee to the constructor arms by its property
            // name (`lower_new`, `lower_global_intrinsic_new`).
            Expr::NewDynamic { callee, .. } => {
                if let Expr::PropertyGet { property, .. } = callee.as_ref() {
                    hand_lowered(set, Surface::Global(property));
                }
            }
            _ => {}
        }
        hand_lowered(set, Surface::Expr(expr));
    }
}

/// Every expression of `module`, as codegen lowers it: module init,
/// functions, class members, closure bodies, global initializers.
fn for_each_expr(module: &Module, f: &mut impl FnMut(&Expr)) {
    perry_hir::tdz_check::for_each_module_expr(module, &mut |e| f(e));
    for global in &module.globals {
        if let Some(init) = &global.init {
            walk(init, f);
        }
    }
}

fn walk(expr: &Expr, f: &mut impl FnMut(&Expr)) {
    f(expr);
    perry_hir::walker::walk_expr_children(expr, &mut |child| walk(child, f));
}

/// A native surface lowering handles in a dedicated arm.
enum Surface<'a> {
    /// `NativeMethodCall.module` / a native import or namespace.
    Module(&'a str),
    /// A global constructor: `New.class_name`, `new <obj>.X`, an
    /// imported constructor's original name.
    Global(&'a str),
    /// A dedicated HIR variant.
    Expr(&'a Expr),
}

const STREAMS: OwnerKind = OwnerKind::Stdlib {
    feature: Some("bundled-streams"),
};
const HTTP_CLIENT: OwnerKind = OwnerKind::Stdlib {
    feature: Some("http-client"),
};
const CRYPTO: OwnerKind = OwnerKind::Stdlib {
    feature: Some("crypto"),
};

/// The provider of the native surfaces lowering emits registry symbols for
/// outside the dispatch table, keyed exactly as the lowering arm dispatches:
///
/// - `lower_call/options/fetch.rs` branches on the Web Streams
///   pseudo-modules (`module == "readable_stream"`, …);
/// - `lower_call/builtin.rs::lower_builtin_new` on the global constructor
///   name (`ReadableStream.from` is matched in [`Walk::visit`]);
/// - `expr/env_clones.rs` on `Expr::NetCreateServer`;
/// - the `crypto` lowering (`expr/calls/crypto_*.rs`, `misc_methods.rs`,
///   `builtin.rs` `X509Certificate`) on the crypto module, its HIR variants
///   and constructors.
///
/// Every owner is a [`crate::ext_registry`] owner of a symbol that arm emits;
/// the codegen check rejects a program whose code reaches one this map
/// misses.
fn hand_lowered_owner(surface: &Surface<'_>) -> Option<OwnerKind> {
    match surface {
        Surface::Module(module) => match *module {
            "readable_stream"
            | "readable_stream_reader"
            | "writable_stream"
            | "writable_stream_writer"
            | "transform_stream" => Some(STREAMS),
            m if m == "crypto" || m.starts_with("crypto.") || m.starts_with("crypto/") => {
                Some(CRYPTO)
            }
            _ => None,
        },
        Surface::Global(name) => match *name {
            "ReadableStream"
            | "WritableStream"
            | "TransformStream"
            | "TextEncoderStream"
            | "TextDecoderStream"
            | "CompressionStream"
            | "DecompressionStream"
            | "ReadableStreamBYOBReader"
            | "CountQueuingStrategy"
            | "ByteLengthQueuingStrategy" => Some(STREAMS),
            "Blob" | "File" => Some(HTTP_CLIENT),
            "WebSocket" | "WebSocketServer" => Some(OwnerKind::WellKnown("ws")),
            "BlockList" | "SocketAddress" => Some(OwnerKind::WellKnown("net")),
            "X509Certificate" => Some(CRYPTO),
            _ => None,
        },
        Surface::Expr(expr) => match expr {
            Expr::NetCreateServer { .. } => Some(OwnerKind::WellKnown("net")),
            Expr::CryptoRandomBytes(_)
            | Expr::CryptoRandomUUID
            | Expr::CryptoRandomUUIDv7
            | Expr::CryptoSha256(_)
            | Expr::CryptoMd5(_)
            | Expr::CryptoRandomFillSync { .. }
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
            | Expr::WebCryptoUnwrapKey { .. } => Some(CRYPTO),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::Stmt;

    fn providers_of(exprs: Vec<Expr>) -> ProviderSet {
        let mut module = Module::new("providers_probe.ts");
        module.init = exprs.into_iter().map(Stmt::Expr).collect();
        let routing = NativeRouting::new([]);
        program_providers(
            [&module],
            &ProviderInputs {
                routing: &routing,
                entry_install_hooks: &[],
                uses_crypto_builtins: false,
            },
        )
    }

    #[test]
    fn table_row_constructor_and_variant_surfaces_name_their_providers() {
        assert!(providers_of(vec![]).is_empty());
        // The dispatch-table row lowering picks (prefix-net owner).
        let undici = Expr::NativeMethodCall {
            module: "undici".into(),
            class_name: None,
            object: None,
            method: "request".into(),
            args: vec![],
        };
        assert!(providers_of(vec![undici]).contains(&OwnerKind::WellKnown("undici")));
        // `new globalThis.File(...)` reaches the builtin constructor arm.
        let file = Expr::NewDynamic {
            byte_end: 0,
            callee: Box::new(Expr::PropertyGet {
                byte_end: 0,
                object: Box::new(Expr::GlobalGet(0)),
                property: "File".into(),
                byte_offset: 0,
            }),
            args: vec![],
            byte_offset: 0,
        };
        assert!(providers_of(vec![file]).contains(&HTTP_CLIENT));
        assert!(providers_of(vec![Expr::CryptoRandomUUID]).contains(&CRYPTO));
        // A method the table does not list for the module adds nothing.
        let unknown = Expr::NativeMethodCall {
            module: "undici".into(),
            class_name: None,
            object: None,
            method: "notARealMethod".into(),
            args: vec![],
        };
        assert!(providers_of(vec![unknown]).is_empty());
    }
}
