//! Wear OS Tiles code generation for Perry widget extensions
//!
//! Compiles WidgetDecl HIR nodes to Kotlin Compose for Wear Tiles source code.
//! Produces Kotlin files that can be compiled with Gradle for Wear OS tile services.

mod emit;
mod emit_glue;

use anyhow::Result;
use perry_hir::ir::WidgetDecl;

/// Generated output for a Wear OS Tile
pub struct WearTileBundle {
    /// Kotlin source files: (filename, source_code)
    pub kotlin_files: Vec<(String, String)>,
    /// AndroidManifest service snippet
    pub manifest_snippet: String,
    /// Tile preview XML (optional)
    pub tile_preview_xml: Option<String>,
}

/// Compile a WidgetDecl to a Wear OS Tile bundle.
///
/// Generates:
/// - {Name}TileService.kt: SuspendingGlanceTileService with tile composable
/// - {Name}TileBridge.kt: JNI bridge for native provider
/// - AndroidManifest snippet
pub fn compile_widget_wear_tile(widget: &WidgetDecl, app_package: &str) -> Result<WearTileBundle> {
    let safe_name = sanitize_kind(&widget.kind);
    let mut kotlin_files = Vec::new();

    // Generate TileService
    let tile_kt = emit::emit_tile_service(widget, &safe_name, app_package);
    kotlin_files.push((format!("{}TileService.kt", safe_name), tile_kt));

    // Generate JNI bridge if provider exists
    if widget.provider_func_name.is_some() || widget.app_group.is_some() {
        let bridge_kt = emit_glue::emit_bridge(widget, &safe_name, app_package);
        kotlin_files.push((format!("{}TileBridge.kt", safe_name), bridge_kt));
    }

    // Generate manifest snippet
    let manifest_snippet = emit::emit_manifest_snippet(&safe_name, app_package);

    Ok(WearTileBundle {
        kotlin_files,
        manifest_snippet,
        tile_preview_xml: None,
    })
}

/// Convert widget kind to a safe Kotlin identifier
fn sanitize_kind(kind: &str) -> String {
    let last = kind.rsplit('.').next().unwrap_or(kind);
    let mut result = String::with_capacity(last.len());
    for c in last.chars() {
        if c.is_alphanumeric() || c == '_' {
            result.push(c);
        }
    }
    if result.is_empty() {
        "PerryTile".to_string()
    } else {
        result
    }
}
