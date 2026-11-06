//! Foreign writes admitted by the compiler's native link inventories.
use super::CompilationContext;
use std::borrow::Cow;

fn links_foreign_native_code(ctx: &CompilationContext) -> bool {
    !ctx.native_addons.is_empty() || !ctx.native_libraries.is_empty()
}

pub(super) fn census(ctx: &CompilationContext) -> Cow<'_, perry_hir::ProtoCensus> {
    if !links_foreign_native_code(ctx) {
        return Cow::Borrowed(&ctx.proto_census);
    }
    let mut census = ctx.proto_census.clone();
    // The source scan cannot see foreign stores, definitions or prototype
    // changes. Keep constant candidates behind the same shape checks as a
    // computed source write, and decline unchecked property adds.
    census.dynamic_write = true;
    census.dynamic_define = true;
    census.relink = true;
    Cow::Owned(census)
}

pub(super) fn report(census: &perry_hir::ProtoCensus) {
    if std::env::var_os("PERRY_PROTO_CENSUS_REPORT").is_some() {
        eprintln!("{}", census.summary());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::compile::{NativeAddonModule, NativeFunctionDecl, NativeLibraryManifest};
    use perry_api_manifest::NativeAbiType;
    use std::path::PathBuf;

    fn context(addon: bool) -> CompilationContext {
        let mut ctx = CompilationContext::new(PathBuf::from("/fixture"));
        ctx.proto_census.members.insert("inherited".into());
        if addon {
            let logical_id = "arbitrary/logical-id".to_string();
            ctx.native_addons.insert(
                logical_id.clone(),
                NativeAddonModule {
                    logical_id,
                    package: "arbitrary".into(),
                    version: "1.0.0".into(),
                    source_path: PathBuf::from("/foreign/binary"),
                    package_dir: PathBuf::from("/foreign"),
                    entry_relative: PathBuf::from("binary"),
                    ship_package_payload: false,
                    reach: Default::default(),
                    node_api_imports: Default::default(),
                    #[cfg(feature = "target-wasi")]
                    wasm_archives: Vec::new(),
                },
            );
        }
        ctx
    }

    #[test]
    fn addon_writes_require_shape_checks_on_inherited_reads() {
        let c = census(&context(true)).into_owned();
        assert!(
            c.method_is_constant("inherited"),
            "keep the guarded candidate"
        );
        assert!(
            c.shadow_reachable(),
            "unknown addon writes can shadow the holder"
        );
    }

    #[test]
    fn addon_definitions_can_intercept_a_property_add() {
        let c = census(&context(true)).into_owned();
        assert!(
            !c.create_add_free("inherited"),
            "an addon can define an inherited setter"
        );
    }

    #[test]
    fn ffi_libraries_require_shape_checks_even_with_a_numeric_abi() {
        let mut ctx = context(false);
        // ABI descriptors constrain marshalling, not a foreign body's effects
        // or the values it can retain and reach through the runtime.
        ctx.native_libraries.push(NativeLibraryManifest {
            module: "arbitrary-foreign-library".into(),
            package_dir: PathBuf::from("/foreign"),
            abi_version: Some("0.5".into()),
            functions: vec![NativeFunctionDecl {
                name: "arbitrary_foreign_entry".into(),
                params: vec![NativeAbiType::F64],
                returns: NativeAbiType::F64,
            }],
            target_config: None,
        });
        let c = census(&ctx);
        assert!(
            c.shadow_reachable(),
            "foreign FFI writes can shadow a holder"
        );
        assert!(
            !c.create_add_free("inherited"),
            "foreign FFI can define a setter"
        );
        assert!(
            c.method_is_constant("inherited"),
            "keep the guarded candidate"
        );
    }

    #[test]
    fn programs_without_foreign_code_keep_the_source_census() {
        let mut ctx = context(false);
        ctx.proto_census.written.insert("written".into());
        assert_eq!(census(&ctx).as_ref(), &ctx.proto_census);
        assert!(!census(&ctx).shadow_reachable());
        assert!(census(&ctx).create_add_free("inherited"));
    }
}
