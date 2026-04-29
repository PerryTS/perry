//! Android Glance widget code generation for Perry widget extensions
//!
//! Compiles WidgetDecl HIR nodes to Kotlin/Glance source code for Android App Widgets.
//! Produces Kotlin files + widget_info XML that can be compiled with Gradle.

mod emit;
mod emit_glue;

use anyhow::Result;
use perry_hir::ir::WidgetDecl;

/// Generated output for an Android Glance widget
pub struct GlanceWidgetBundle {
    /// Kotlin source files: (filename, source_code)
    pub kotlin_files: Vec<(String, String)>,
    /// Widget provider XML (res/xml/widget_info_{name}.xml)
    pub widget_info_xml: String,
    /// AndroidManifest receiver snippet
    pub manifest_snippet: String,
}

/// Compile a WidgetDecl to an Android Glance widget bundle.
///
/// Generates:
/// - {Name}Widget.kt: GlanceAppWidget with provideGlance() composable
/// - {Name}Receiver.kt: GlanceAppWidgetReceiver subclass
/// - {Name}Bridge.kt: JNI bridge for native provider
/// - {Name}ConfigActivity.kt: Configuration Activity (if config_params non-empty)
/// - widget_info_{name}.xml: AppWidgetProvider metadata
/// - AndroidManifest snippet
pub fn compile_widget_glance(widget: &WidgetDecl, app_package: &str) -> Result<GlanceWidgetBundle> {
    let safe_name = sanitize_kind(&widget.kind);
    let mut kotlin_files = Vec::new();

    // Generate GlanceAppWidget
    let widget_kt = emit::emit_widget(&widget, &safe_name, app_package);
    kotlin_files.push((format!("{}Widget.kt", safe_name), widget_kt));

    // Generate GlanceAppWidgetReceiver
    let receiver_kt = emit::emit_receiver(&safe_name, app_package);
    kotlin_files.push((format!("{}Receiver.kt", safe_name), receiver_kt));

    // Generate JNI bridge if provider exists
    if widget.provider_func_name.is_some() || widget.app_group.is_some() {
        let bridge_kt = emit_glue::emit_bridge(widget, &safe_name, app_package);
        kotlin_files.push((format!("{}Bridge.kt", safe_name), bridge_kt));
    }

    // Generate config activity if config params exist
    if !widget.config_params.is_empty() {
        let config_kt = emit::emit_config_activity(widget, &safe_name, app_package);
        kotlin_files.push((format!("{}ConfigActivity.kt", safe_name), config_kt));
    }

    // Generate widget_info XML
    let widget_info_xml = emit::emit_widget_info_xml(widget, &safe_name);

    // Generate manifest snippet
    let manifest_snippet = emit::emit_manifest_snippet(widget, &safe_name, app_package);

    Ok(GlanceWidgetBundle {
        kotlin_files,
        widget_info_xml,
        manifest_snippet,
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
        "PerryWidget".to_string()
    } else {
        result
    }
}
