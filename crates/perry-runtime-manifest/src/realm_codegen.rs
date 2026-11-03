//! Typed compiler consumer of the runtime build producer's JSON artifact.
//! Included by codegen's build script, never by the runtime manifest crate.
use crate::realm::RealmManifest;
use std::fmt::Write;
use std::path::Path;

pub fn generate(runtime_src: &Path, out: &Path) -> Result<RealmManifest, String> {
    let raw = perry_runtime_manifest::realm_source::generate(runtime_src, out)?;
    let start = std::time::Instant::now();
    let manifest: RealmManifest = serde_json::from_str(&raw.encode()).map_err(|e| e.to_string())?;
    manifest.validate()?;
    let mut rust = String::from("// Generated from source-owned runtime realm facts.\n");
    rust.push_str("pub const REALM_MANIFEST_VERSION: u32 = perry_runtime_manifest::realm_source::REALM_MANIFEST_VERSION;\n");
    rust.push_str("pub static REALM: &[perry_hir::realm_manifest::RealmEntry] = &[\n");
    for d in &manifest.declarations {
        let module = d
            .module
            .as_ref()
            .map_or("None".into(), |m| format!("Some({m:?})"));
        writeln!(rust,"perry_hir::realm_manifest::RealmEntry {{ name: {:?}, kind: perry_hir::realm_manifest::RealmKind::{:?}, births: &{:?}, module: {}, facts: {:?} }},",d.name,d.kind,d.births,module,serde_json::to_string(d).unwrap()).unwrap();
    }
    rust.push_str("];\n");
    std::fs::write(out.join("realm_table.rs"), rust).map_err(|e| e.to_string())?;
    std::fs::write(
        out.join("realm-consume-time-ns"),
        start.elapsed().as_nanos().to_string(),
    )
    .map_err(|e| e.to_string())?;
    Ok(manifest)
}
