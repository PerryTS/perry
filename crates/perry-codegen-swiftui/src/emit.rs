//! HIR WidgetNode → SwiftUI source emitter
//!
//! Walks the declarative widget tree and emits valid SwiftUI source code.
//! Supports iOS WidgetKit and watchOS accessory families.

use perry_hir::ir::*;
use std::fmt::Write;

/// Default WidgetKit refresh interval (seconds) when the widget declares no
/// compile-time `reloadPolicy`: 30 minutes.
const DEFAULT_RELOAD_SECONDS: u32 = 1800;

/// WidgetKit floor (seconds): the system's refresh budget does not honor
/// reload requests below ~15 minutes, so anything shorter is clamped.
const MIN_RELOAD_SECONDS: u32 = 900;

/// Resolve the widget's effective timeline reload interval in seconds:
/// the parsed `reloadPolicy` if present, clamped to the WidgetKit floor
/// (with a compile-time warning), or the 30-minute default.
fn effective_reload_seconds(widget: &WidgetDecl) -> u32 {
    match widget.reload_after_seconds {
        None => DEFAULT_RELOAD_SECONDS,
        Some(s) if s < MIN_RELOAD_SECONDS => {
            eprintln!(
                "[perry] warning: widget '{}': reloadPolicy requests a refresh every {}s, \
below the WidgetKit minimum of {}s (15 minutes); clamping to {}s",
                widget.kind, s, MIN_RELOAD_SECONDS, MIN_RELOAD_SECONDS
            );
            MIN_RELOAD_SECONDS
        }
        Some(s) => s,
    }
}

/// Emit the TimelineEntry struct (and any nested Codable structs for complex types)
pub fn emit_entry_struct(widget: &WidgetDecl, name: &str) -> String {
    let mut out = String::new();

    // Emit nested structs for Array(Object(...)) fields first
    for (field_name, field_type) in &widget.entry_fields {
        emit_nested_structs(&mut out, name, field_name, field_type);
    }

    writeln!(out, "struct {}Entry: TimelineEntry {{", name).unwrap();
    writeln!(out, "    let date: Date").unwrap();
    for (field_name, field_type) in &widget.entry_fields {
        let swift_type = swift_type_for_field(name, field_name, field_type);
        writeln!(out, "    let {}: {}", field_name, swift_type).unwrap();
    }
    writeln!(out, "}}").unwrap();
    out
}

/// Emit nested Codable structs for complex field types
fn emit_nested_structs(
    out: &mut String,
    parent_name: &str,
    field_name: &str,
    field_type: &WidgetFieldType,
) {
    match field_type {
        WidgetFieldType::Array(inner) => {
            emit_nested_structs(out, parent_name, field_name, inner);
        }
        WidgetFieldType::Object(fields) => {
            let struct_name = format!("{}{}Item", parent_name, capitalize(field_name));
            writeln!(
                out,
                "struct {}: Codable, Identifiable, Hashable {{",
                struct_name
            )
            .unwrap();
            // Find a suitable id field
            let has_id = fields.iter().any(|(n, _)| n == "id");
            if !has_id {
                // Use first string field as id
                if let Some((first_str, _)) = fields
                    .iter()
                    .find(|(_, t)| matches!(t, WidgetFieldType::String))
                {
                    writeln!(out, "    var id: String {{ {} }}", first_str).unwrap();
                } else {
                    writeln!(out, "    var id: String {{ UUID().uuidString }}").unwrap();
                }
            }
            for (fname, ftype) in fields {
                let swift_type = swift_type_for_field(parent_name, fname, ftype);
                writeln!(out, "    let {}: {}", fname, swift_type).unwrap();
            }
            writeln!(out, "}}").unwrap();
            writeln!(out).unwrap();
        }
        WidgetFieldType::Optional(inner) => {
            emit_nested_structs(out, parent_name, field_name, inner);
        }
        _ => {}
    }
}

/// Map a WidgetFieldType to a Swift type string
fn swift_type_for_field(
    parent_name: &str,
    field_name: &str,
    field_type: &WidgetFieldType,
) -> String {
    match field_type {
        WidgetFieldType::String => "String".to_string(),
        WidgetFieldType::Number => "Double".to_string(),
        WidgetFieldType::Boolean => "Bool".to_string(),
        WidgetFieldType::Array(inner) => match inner.as_ref() {
            WidgetFieldType::Object(_) => {
                format!("[{}{}Item]", parent_name, capitalize(field_name))
            }
            _ => format!("[{}]", swift_type_for_field(parent_name, field_name, inner)),
        },
        WidgetFieldType::Optional(inner) => {
            format!("{}?", swift_type_for_field(parent_name, field_name, inner))
        }
        WidgetFieldType::Object(_) => {
            format!("{}{}Item", parent_name, capitalize(field_name))
        }
    }
}

/// Emit the `, <field>: <decode-expr>` fragment that pulls one entry field out of
/// the provider's JSON `entryDict` and coerces it to the Swift type used by the
/// generated `<Name>Entry` struct.
///
/// Scalars use a direct `as?` cast. Arrays of objects (and any other JSON-backed
/// nested shape) are round-tripped through `JSONSerialization`/`JSONDecoder` into
/// the nested `Codable` struct(s) emitted by `emit_nested_structs`, so that
/// `[<Name><Field>Item]` actually decodes instead of being cast to `String`
/// (see issue #5070).
fn entry_field_decode(parent_name: &str, field_name: &str, field_type: &WidgetFieldType) -> String {
    match field_type {
        WidgetFieldType::String => {
            format!(
                ", {f}: entryDict[\"{f}\"] as? String ?? \"\"",
                f = field_name
            )
        }
        WidgetFieldType::Number => {
            format!(", {f}: entryDict[\"{f}\"] as? Double ?? 0", f = field_name)
        }
        WidgetFieldType::Boolean => {
            format!(
                ", {f}: entryDict[\"{f}\"] as? Bool ?? false",
                f = field_name
            )
        }
        WidgetFieldType::Array(inner) => match inner.as_ref() {
            // Arrays of scalars can be cast directly.
            WidgetFieldType::String => {
                format!(
                    ", {f}: entryDict[\"{f}\"] as? [String] ?? []",
                    f = field_name
                )
            }
            WidgetFieldType::Number => {
                format!(
                    ", {f}: entryDict[\"{f}\"] as? [Double] ?? []",
                    f = field_name
                )
            }
            WidgetFieldType::Boolean => {
                format!(", {f}: entryDict[\"{f}\"] as? [Bool] ?? []", f = field_name)
            }
            // Arrays of objects (and nested arrays) decode through Codable.
            _ => {
                let swift_type = swift_type_for_field(parent_name, field_name, field_type);
                format!(
                    ", {f}: {decode} ?? []",
                    f = field_name,
                    decode = json_decode_expr(field_name, &swift_type)
                )
            }
        },
        WidgetFieldType::Object(_) => {
            // A bare (non-array) object entry field maps to a non-optional struct
            // that has no zero-value, so we decode and force-unwrap. This mirrors
            // the placeholder path, which likewise cannot synthesize a default.
            let swift_type = swift_type_for_field(parent_name, field_name, field_type);
            format!(
                ", {f}: {decode}!",
                f = field_name,
                decode = json_decode_expr(field_name, &swift_type)
            )
        }
        WidgetFieldType::Optional(inner) => match inner.as_ref() {
            WidgetFieldType::String => {
                format!(", {f}: entryDict[\"{f}\"] as? String", f = field_name)
            }
            WidgetFieldType::Number => {
                format!(", {f}: entryDict[\"{f}\"] as? Double", f = field_name)
            }
            WidgetFieldType::Boolean => {
                format!(", {f}: entryDict[\"{f}\"] as? Bool", f = field_name)
            }
            _ => {
                let swift_type = swift_type_for_field(parent_name, field_name, inner);
                format!(
                    ", {f}: {decode}",
                    f = field_name,
                    decode = json_decode_expr(field_name, &swift_type)
                )
            }
        },
    }
}

/// Build a Swift expression that decodes `entryDict["<key>"]` into `swift_type`
/// via `JSONSerialization` + `JSONDecoder`. The result is an optional (`nil` on
/// any failure); callers append `?? <default>` for non-optional targets.
fn json_decode_expr(key: &str, swift_type: &str) -> String {
    format!(
        "(entryDict[\"{key}\"]).flatMap {{ try? JSONSerialization.data(withJSONObject: $0) }}.flatMap {{ try? JSONDecoder().decode({ty}.self, from: $0) }}",
        key = key,
        ty = swift_type
    )
}

/// Capitalize the first letter of a string
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Emit the SwiftUI View from the render body
pub fn emit_view(widget: &WidgetDecl, name: &str) -> String {
    let mut out = String::new();
    let entry_param = &widget.entry_param_name;

    writeln!(out, "struct {}View: View {{", name).unwrap();
    writeln!(out, "    let {}: {}Entry", entry_param, name).unwrap();

    // Add family environment variable if family-specific rendering is used
    if widget.family_param_name.is_some() {
        writeln!(out, "    @Environment(\\.widgetFamily) var family").unwrap();
    }

    writeln!(out).unwrap();
    writeln!(out, "    var body: some View {{").unwrap();

    // Emit the render tree
    if widget.render_body.is_empty() {
        writeln!(out, "        Text(\"Empty widget\")").unwrap();
    } else if widget.render_body.len() == 1 {
        let node_str = emit_node(&widget.render_body[0], entry_param, 2);
        out.push_str(&node_str);
    } else {
        // Multiple root nodes — wrap in VStack
        writeln!(out, "        VStack {{").unwrap();
        for node in &widget.render_body {
            let node_str = emit_node(node, entry_param, 3);
            out.push_str(&node_str);
        }
        writeln!(out, "        }}").unwrap();
    }

    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    out
}

/// Emit the TimelineProvider
pub fn emit_timeline_provider(widget: &WidgetDecl, name: &str) -> String {
    let has_config = !widget.config_params.is_empty();
    let has_provider = widget.provider_func_name.is_some();

    if has_config {
        emit_app_intent_timeline_provider(widget, name, has_provider)
    } else if has_provider {
        emit_native_timeline_provider(widget, name)
    } else {
        emit_static_timeline_provider(widget, name)
    }
}

/// Emit a static TimelineProvider (no config, no native provider)
fn emit_static_timeline_provider(widget: &WidgetDecl, name: &str) -> String {
    let mut out = String::new();

    writeln!(out, "struct {}Provider: TimelineProvider {{", name).unwrap();
    writeln!(
        out,
        "    func placeholder(in context: Context) -> {}Entry {{",
        name
    )
    .unwrap();
    emit_placeholder_entry(&mut out, widget, name, "        ");
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    func getSnapshot(in context: Context, completion: @escaping ({}Entry) -> ()) {{",
        name
    )
    .unwrap();
    writeln!(out, "        completion(placeholder(in: context))").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "    func getTimeline(in context: Context, completion: @escaping (Timeline<{}Entry>) -> ()) {{", name).unwrap();
    writeln!(out, "        let entry = placeholder(in: context)").unwrap();
    writeln!(
        out,
        "        let timeline = Timeline(entries: [entry], policy: .atEnd)"
    )
    .unwrap();
    writeln!(out, "        completion(timeline)").unwrap();
    writeln!(out, "    }}").unwrap();

    writeln!(out, "}}").unwrap();
    out
}

/// Emit a TimelineProvider that calls a native LLVM-compiled provider function
fn emit_native_timeline_provider(widget: &WidgetDecl, name: &str) -> String {
    let mut out = String::new();
    let func_name = widget.provider_func_name.as_deref().unwrap();
    let reload_seconds = effective_reload_seconds(widget);

    writeln!(out, "struct {}Provider: TimelineProvider {{", name).unwrap();
    writeln!(
        out,
        "    func placeholder(in context: Context) -> {}Entry {{",
        name
    )
    .unwrap();
    emit_placeholder_entry(&mut out, widget, name, "        ");
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    func getSnapshot(in context: Context, completion: @escaping ({}Entry) -> ()) {{",
        name
    )
    .unwrap();
    writeln!(out, "        completion(placeholder(in: context))").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "    func getTimeline(in context: Context, completion: @escaping (Timeline<{}Entry>) -> ()) {{", name).unwrap();
    writeln!(out, "        perry_runtime_widget_init()").unwrap();
    writeln!(out, "        let configJson = \"{{}}\"").unwrap();
    writeln!(
        out,
        "        let configPtr = perry_nanbox_string(configJson)"
    )
    .unwrap();
    writeln!(out, "        let resultPtr = {}(configPtr)", func_name).unwrap();
    writeln!(out, "        let resultJson = perry_get_string(resultPtr)").unwrap();
    writeln!(
        out,
        "        if let data = resultJson.data(using: String.Encoding.utf8),"
    )
    .unwrap();
    writeln!(
        out,
        "           let result = try? JSONSerialization.jsonObject(with: data) as? [String: Any],"
    )
    .unwrap();
    writeln!(
        out,
        "           let entries = result[\"entries\"] as? [[String: Any]] {{"
    )
    .unwrap();
    writeln!(out, "            var timelineEntries: [{}Entry] = []", name).unwrap();
    writeln!(out, "            for entryDict in entries {{").unwrap();
    write!(
        out,
        "                let entry = {}Entry(date: Date()",
        name
    )
    .unwrap();
    for (field_name, field_type) in &widget.entry_fields {
        write!(out, "{}", entry_field_decode(name, field_name, field_type)).unwrap();
    }
    writeln!(out, ")").unwrap();
    writeln!(out, "                timelineEntries.append(entry)").unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(
        out,
        "            let reloadDate = Date().addingTimeInterval({})",
        reload_seconds
    )
    .unwrap();
    writeln!(
        out,
        "            let timeline = Timeline(entries: timelineEntries, policy: .after(reloadDate))"
    )
    .unwrap();
    writeln!(out, "            completion(timeline)").unwrap();
    writeln!(out, "        }} else {{").unwrap();
    writeln!(out, "            let entry = placeholder(in: context)").unwrap();
    writeln!(out, "            let timeline = Timeline(entries: [entry], policy: .after(Date().addingTimeInterval(300)))").unwrap();
    writeln!(out, "            completion(timeline)").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();

    writeln!(out, "}}").unwrap();
    out
}

/// Emit an AppIntentTimelineProvider (has config params)
fn emit_app_intent_timeline_provider(
    widget: &WidgetDecl,
    name: &str,
    has_provider: bool,
) -> String {
    let mut out = String::new();
    let reload_seconds = effective_reload_seconds(widget);

    writeln!(out, "struct {}Provider: AppIntentTimelineProvider {{", name).unwrap();
    writeln!(
        out,
        "    func placeholder(in context: Context) -> {}Entry {{",
        name
    )
    .unwrap();
    emit_placeholder_entry(&mut out, widget, name, "        ");
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "    func snapshot(for configuration: {}Intent, in context: Context) async -> {}Entry {{",
        name, name
    )
    .unwrap();
    writeln!(out, "        placeholder(in: context)").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "    func timeline(for configuration: {}Intent, in context: Context) async -> Timeline<{}Entry> {{", name, name).unwrap();

    if has_provider {
        let func_name = widget.provider_func_name.as_deref().unwrap();
        writeln!(out, "        perry_runtime_widget_init()").unwrap();

        // Serialize config params to JSON
        write!(out, "        let configJson = \"{{").unwrap();
        for (i, param) in widget.config_params.iter().enumerate() {
            if i > 0 {
                write!(out, ",").unwrap();
            }
            write!(
                out,
                "\\\"{}\\\":\\\"\\(configuration.{}.rawValue)\\\"",
                param.name, param.name
            )
            .unwrap();
        }
        writeln!(out, "}}\"").unwrap();

        writeln!(
            out,
            "        let configPtr = perry_nanbox_string(configJson)"
        )
        .unwrap();
        writeln!(out, "        let resultPtr = {}(configPtr)", func_name).unwrap();
        writeln!(out, "        let resultJson = perry_get_string(resultPtr)").unwrap();
        writeln!(
            out,
            "        if let data = resultJson.data(using: String.Encoding.utf8),"
        )
        .unwrap();
        writeln!(out, "           let result = try? JSONSerialization.jsonObject(with: data) as? [String: Any],").unwrap();
        writeln!(
            out,
            "           let entries = result[\"entries\"] as? [[String: Any]] {{"
        )
        .unwrap();
        writeln!(out, "            var timelineEntries: [{}Entry] = []", name).unwrap();
        writeln!(out, "            for entryDict in entries {{").unwrap();
        write!(
            out,
            "                let entry = {}Entry(date: Date()",
            name
        )
        .unwrap();
        for (field_name, field_type) in &widget.entry_fields {
            write!(out, "{}", entry_field_decode(name, field_name, field_type)).unwrap();
        }
        writeln!(out, ")").unwrap();
        writeln!(out, "                timelineEntries.append(entry)").unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(
            out,
            "            let reloadDate = Date().addingTimeInterval({})",
            reload_seconds
        )
        .unwrap();
        writeln!(
            out,
            "            return Timeline(entries: timelineEntries, policy: .after(reloadDate))"
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
    }

    writeln!(out, "        let entry = placeholder(in: context)").unwrap();
    writeln!(
        out,
        "        return Timeline(entries: [entry], policy: .after(Date().addingTimeInterval(300)))"
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();

    writeln!(out, "}}").unwrap();
    out
}

/// Emit a placeholder entry expression with proper default values
fn emit_placeholder_entry(out: &mut String, widget: &WidgetDecl, name: &str, indent: &str) {
    write!(out, "{}{}Entry(date: Date()", indent, name).unwrap();
    for (field_name, field_type) in &widget.entry_fields {
        let default_val = if let Some(ref ph) = widget.placeholder {
            if let Some((_, val)) = ph.iter().find(|(n, _)| n == field_name) {
                emit_placeholder_swift_value(val, field_type)
            } else {
                default_value_for_type(field_type)
            }
        } else {
            default_value_for_type(field_type)
        };
        write!(out, ", {}: {}", field_name, default_val).unwrap();
    }
    writeln!(out, ")").unwrap();
}

/// Emit a placeholder value as Swift source
fn emit_placeholder_swift_value(
    val: &WidgetPlaceholderValue,
    _field_type: &WidgetFieldType,
) -> String {
    match val {
        WidgetPlaceholderValue::String(s) => format!("\"{}\"", escape_swift_string(s)),
        WidgetPlaceholderValue::Number(n) => format_f64(*n),
        WidgetPlaceholderValue::Bool(b) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        WidgetPlaceholderValue::Array(_) => "[]".to_string(),
        WidgetPlaceholderValue::Object(_) => "nil".to_string(),
        WidgetPlaceholderValue::Null => "nil".to_string(),
    }
}

/// Default value for a field type
fn default_value_for_type(field_type: &WidgetFieldType) -> String {
    match field_type {
        WidgetFieldType::String => "\"...\"".to_string(),
        WidgetFieldType::Number => "0".to_string(),
        WidgetFieldType::Boolean => "false".to_string(),
        WidgetFieldType::Array(_) => "[]".to_string(),
        WidgetFieldType::Optional(_) => "nil".to_string(),
        WidgetFieldType::Object(_) => "nil".to_string(),
    }
}

/// Emit AppIntent configuration types (enums + intent struct)
pub fn emit_app_intent_config(widget: &WidgetDecl, name: &str) -> String {
    let mut out = String::new();

    // Emit AppEnum for each enum config param
    for param in &widget.config_params {
        if let WidgetConfigParamType::Enum { values, default } = &param.param_type {
            let enum_name = format!("{}{}Option", name, capitalize(&param.name));
            writeln!(out, "enum {}: String, AppEnum {{", enum_name).unwrap();
            writeln!(
                out,
                "    static var typeDisplayRepresentation: TypeDisplayRepresentation = \"{}\"",
                param.title
            )
            .unwrap();
            writeln!(
                out,
                "    static var caseDisplayRepresentations: [Self: DisplayRepresentation] = ["
            )
            .unwrap();
            for val in values {
                writeln!(out, "        .{}: \"{}\",", val, val).unwrap();
            }
            writeln!(out, "    ]").unwrap();
            writeln!(out).unwrap();
            for val in values {
                writeln!(out, "    case {}", val).unwrap();
            }
            writeln!(out, "}}").unwrap();
            writeln!(out).unwrap();
            let _ = default; // suppress unused warning
        }
    }

    // Emit WidgetConfigurationIntent
    writeln!(out, "struct {}Intent: WidgetConfigurationIntent {{", name).unwrap();
    writeln!(
        out,
        "    static var title: LocalizedStringResource = \"{}\"",
        widget.display_name
    )
    .unwrap();
    writeln!(
        out,
        "    static var description = IntentDescription(\"{}\")",
        widget.description
    )
    .unwrap();
    writeln!(out).unwrap();
    for param in &widget.config_params {
        match &param.param_type {
            WidgetConfigParamType::Enum { default, .. } => {
                let enum_name = format!("{}{}Option", name, capitalize(&param.name));
                writeln!(
                    out,
                    "    @Parameter(title: \"{}\", default: .{})",
                    param.title, default
                )
                .unwrap();
                writeln!(out, "    var {}: {}", param.name, enum_name).unwrap();
            }
            WidgetConfigParamType::Bool { default } => {
                writeln!(
                    out,
                    "    @Parameter(title: \"{}\", default: {})",
                    param.title, default
                )
                .unwrap();
                writeln!(out, "    var {}: Bool", param.name).unwrap();
            }
            WidgetConfigParamType::String { default } => {
                writeln!(
                    out,
                    "    @Parameter(title: \"{}\", default: \"{}\")",
                    param.title,
                    escape_swift_string(default)
                )
                .unwrap();
                writeln!(out, "    var {}: String", param.name).unwrap();
            }
        }
        writeln!(out).unwrap();
    }
    writeln!(out, "}}").unwrap();
    out
}

/// Emit the @main WidgetBundle
pub fn emit_widget_bundle(widget: &WidgetDecl, name: &str) -> String {
    let mut out = String::new();
    let has_config = !widget.config_params.is_empty();

    writeln!(out, "@main").unwrap();
    writeln!(out, "struct {}WidgetBundle: SwiftUI.WidgetBundle {{", name).unwrap();
    writeln!(out, "    var body: some Widget {{").unwrap();
    writeln!(out, "        {}Widget()", name).unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "struct {}Widget: Widget {{", name).unwrap();
    writeln!(out, "    let kind: String = \"{}\"", widget.kind).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "    var body: some WidgetConfiguration {{").unwrap();

    if has_config {
        writeln!(out, "        AppIntentConfiguration(kind: kind, intent: {}Intent.self, provider: {}Provider()) {{ entry in", name, name).unwrap();
    } else {
        writeln!(
            out,
            "        StaticConfiguration(kind: kind, provider: {}Provider()) {{ entry in",
            name
        )
        .unwrap();
    }

    writeln!(
        out,
        "            {}View({}: entry)",
        name, widget.entry_param_name
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();

    // Display name
    if !widget.display_name.is_empty() {
        writeln!(
            out,
            "        .configurationDisplayName(\"{}\")",
            escape_swift_string(&widget.display_name)
        )
        .unwrap();
    }
    // Description
    if !widget.description.is_empty() {
        writeln!(
            out,
            "        .description(\"{}\")",
            escape_swift_string(&widget.description)
        )
        .unwrap();
    }
    // Supported families
    if !widget.supported_families.is_empty() {
        let families: Vec<String> = widget
            .supported_families
            .iter()
            .map(|f| format!(".{}", f))
            .collect();
        writeln!(out, "        .supportedFamilies([{}])", families.join(", ")).unwrap();
    }

    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    out
}

/// Emit the shared per-bundle runtime FFI block (`PerryWidgetRuntime.swift`).
///
/// This block — the `@_silgen_name` perry-runtime imports, the two
/// `perry_nanbox_string` overloads, the `perry_get_string` wrapper, and the
/// `@_cdecl("perry_widget_shared_storage_get")` bridge — must be emitted
/// **once per bundle**, `internal`. A widget bundle is compiled as a single
/// Swift module (`swiftc *.swift`), so:
///
/// - `private` (the #1294 fix) scopes each declaration to its own glue file,
///   but the helpers are *called* from a different generated file
///   (`{Name}.swift`), so the call site can't see them → "inaccessible due to
///   'private'" (#5069).
/// - non-`private` per-widget emits N copies across N glue files →
///   "redeclaration" (the original #1294 failure).
///
/// Emitting the shared block once, `internal`, satisfies both: a single
/// definition, visible module-wide. The `@_cdecl` shared-storage bridge
/// exports a fixed C symbol, so it likewise must appear exactly once per
/// bundle (≥2 widgets sharing an app group would otherwise duplicate it).
pub fn emit_shared_runtime(app_group: Option<&str>) -> String {
    let mut out = String::new();

    writeln!(out, "// Auto-generated native bridge — do not edit").unwrap();
    writeln!(out, "import Foundation").unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "// Perry runtime FFI (shared across every widget in this bundle)"
    )
    .unwrap();
    writeln!(out, "@_silgen_name(\"perry_runtime_widget_init\")").unwrap();
    writeln!(out, "func perry_runtime_widget_init()").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "@_silgen_name(\"js_nanbox_string\")").unwrap();
    writeln!(
        out,
        "func perry_nanbox_string(_ s: UnsafePointer<CChar>) -> Int64"
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(out, "@_silgen_name(\"js_get_string_pointer_unified\")").unwrap();
    writeln!(
        out,
        "func perry_get_string_ptr(_ val: Int64) -> UnsafePointer<CChar>"
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(out, "func perry_nanbox_string(_ s: String) -> Int64 {{").unwrap();
    writeln!(
        out,
        "    return s.withCString {{ perry_nanbox_string($0) }}"
    )
    .unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "func perry_get_string(_ val: Int64) -> String {{").unwrap();
    writeln!(out, "    return String(cString: perry_get_string_ptr(val))").unwrap();
    writeln!(out, "}}").unwrap();

    // sharedStorage bridge — emitted once per bundle because @_cdecl exports a
    // fixed C symbol (`perry_widget_shared_storage_get`).
    if let Some(app_group) = app_group {
        writeln!(out).unwrap();
        writeln!(
            out,
            "// Shared storage bridge — called from native provider code"
        )
        .unwrap();
        writeln!(out, "@_cdecl(\"perry_widget_shared_storage_get\")").unwrap();
        writeln!(
            out,
            "func widgetSharedStorageGet(_ keyPtr: Int64) -> Int64 {{"
        )
        .unwrap();
        writeln!(out, "    let key = perry_get_string(keyPtr)").unwrap();
        writeln!(
            out,
            "    let value = UserDefaults(suiteName: \"{}\")?.string(forKey: key) ?? \"\"",
            escape_swift_string(app_group)
        )
        .unwrap();
        writeln!(out, "    return perry_nanbox_string(value)").unwrap();
        writeln!(out, "}}").unwrap();
    }

    out
}

/// Emit the per-widget native bridge: just the widget-unique provider extern.
///
/// The shared runtime FFI (`perry_runtime_widget_init`, `perry_nanbox_string`,
/// `perry_get_string`, …) and the shared-storage `@_cdecl` bridge live in
/// `PerryWidgetRuntime.swift` (see [`emit_shared_runtime`]); this file declares
/// only the `@_silgen_name` import of *this* widget's LLVM-compiled provider,
/// which is uniquely named per widget and so is safe to emit per-file.
pub fn emit_glue(widget: &WidgetDecl, name: &str) -> String {
    let mut out = String::new();

    writeln!(out, "// Auto-generated native bridge — do not edit").unwrap();
    writeln!(out, "import Foundation").unwrap();
    writeln!(out).unwrap();

    // Provider function extern (unique per widget).
    if let Some(ref func_name) = widget.provider_func_name {
        writeln!(out, "@_silgen_name(\"{}\")", func_name).unwrap();
        writeln!(out, "func {}(_ configJson: Int64) -> Int64", func_name).unwrap();
    }

    let _ = name; // suppress unused warning
    out
}

/// Emit a single WidgetNode as SwiftUI source
fn emit_node(node: &WidgetNode, entry_param: &str, indent: usize) -> String {
    let mut out = String::new();
    let pad = "    ".repeat(indent);

    match node {
        WidgetNode::Text { content, modifiers } => {
            let text_arg = emit_text_content(content, entry_param);
            write!(out, "{}Text({})", pad, text_arg).unwrap();
            emit_modifiers(&mut out, modifiers, indent);
            writeln!(out).unwrap();
        }
        WidgetNode::Stack {
            kind,
            spacing,
            children,
            modifiers,
        } => {
            let stack_name = match kind {
                WidgetStackKind::VStack => "VStack",
                WidgetStackKind::HStack => "HStack",
                WidgetStackKind::ZStack => "ZStack",
            };
            if let Some(sp) = spacing {
                write!(out, "{}{}(spacing: {})", pad, stack_name, format_f64(*sp)).unwrap();
            } else {
                write!(out, "{}{}", pad, stack_name).unwrap();
            }
            writeln!(out, " {{").unwrap();
            for child in children {
                out.push_str(&emit_node(child, entry_param, indent + 1));
            }
            write!(out, "{}}}", pad).unwrap();
            emit_modifiers(&mut out, modifiers, indent);
            writeln!(out).unwrap();
        }
        WidgetNode::Image {
            system_name,
            modifiers,
        } => {
            write!(
                out,
                "{}Image(systemName: \"{}\")",
                pad,
                escape_swift_string(system_name)
            )
            .unwrap();
            emit_modifiers(&mut out, modifiers, indent);
            writeln!(out).unwrap();
        }
        WidgetNode::Spacer => {
            writeln!(out, "{}Spacer()", pad).unwrap();
        }
        WidgetNode::Divider => {
            writeln!(out, "{}Divider()", pad).unwrap();
        }
        WidgetNode::Conditional {
            field,
            op,
            value,
            then_node,
            else_node,
        } => {
            let cond = emit_condition(field, op, value, entry_param);
            writeln!(out, "{}if {} {{", pad, cond).unwrap();
            out.push_str(&emit_node(then_node, entry_param, indent + 1));
            if let Some(else_n) = else_node {
                writeln!(out, "{}}} else {{", pad).unwrap();
                out.push_str(&emit_node(else_n, entry_param, indent + 1));
            }
            writeln!(out, "{}}}", pad).unwrap();
        }
        WidgetNode::ForEach {
            collection_field,
            item_param,
            body,
        } => {
            writeln!(
                out,
                "{}ForEach({}.{}, id: \\.self) {{ {} in",
                pad, entry_param, collection_field, item_param
            )
            .unwrap();
            out.push_str(&emit_node(body, item_param, indent + 1));
            writeln!(out, "{}}}", pad).unwrap();
        }
        WidgetNode::Label {
            text,
            system_image,
            modifiers,
        } => {
            let text_arg = emit_text_content(text, entry_param);
            write!(
                out,
                "{}Label({}, systemImage: \"{}\")",
                pad,
                text_arg,
                escape_swift_string(system_image)
            )
            .unwrap();
            emit_modifiers(&mut out, modifiers, indent);
            writeln!(out).unwrap();
        }
        WidgetNode::FamilySwitch { cases, default } => {
            writeln!(out, "{}switch family {{", pad).unwrap();
            for (family_value, node) in cases {
                writeln!(out, "{}case .{}:", pad, family_value).unwrap();
                out.push_str(&emit_node(node, entry_param, indent + 1));
            }
            if let Some(default_node) = default {
                writeln!(out, "{}default:", pad).unwrap();
                out.push_str(&emit_node(default_node, entry_param, indent + 1));
            } else {
                writeln!(out, "{}default:", pad).unwrap();
                writeln!(out, "{}    EmptyView()", pad).unwrap();
            }
            writeln!(out, "{}}}", pad).unwrap();
        }
        WidgetNode::Gauge {
            value_expr,
            label,
            style,
            modifiers,
        } => {
            // Parse value expression for entry param references
            let value_str =
                if value_expr.contains('/') || value_expr.contains('*') || value_expr.contains(' ')
                {
                    // Complex expression like "totalClicks / clicksGoal" → entry.totalClicks / entry.clicksGoal
                    value_expr
                        .split_whitespace()
                        .map(|part| {
                            if part.len() > 1
                                && part
                                    .chars()
                                    .next()
                                    .map(|c| c.is_alphabetic())
                                    .unwrap_or(false)
                                && !matches!(part, "/" | "*" | "+" | "-")
                            {
                                format!("{}.{}", entry_param, part)
                            } else {
                                part.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    format!("{}.{}", entry_param, value_expr)
                };

            writeln!(out, "{}Gauge(value: {}) {{", pad, value_str).unwrap();
            writeln!(out, "{}    Text(\"{}\")", pad, escape_swift_string(label)).unwrap();
            write!(out, "{}}}", pad).unwrap();

            let gauge_style = match style {
                GaugeStyle::Circular => ".accessoryCircularCapacity",
                GaugeStyle::LinearCapacity => ".linearCapacity",
            };
            write!(out, "\n{}    .gaugeStyle({})", pad, gauge_style).unwrap();
            emit_modifiers(&mut out, modifiers, indent);
            writeln!(out).unwrap();
        }
    }

    out
}

/// Emit text content as a Swift expression
fn emit_text_content(content: &WidgetTextContent, entry_param: &str) -> String {
    match content {
        WidgetTextContent::Literal(s) => {
            format!("\"{}\"", escape_swift_string(s))
        }
        WidgetTextContent::Field(field) => {
            // Check if field is numeric — if so, use String interpolation
            format!("\"\\({}.{})\"", entry_param, field)
        }
        WidgetTextContent::Template(parts) => {
            let mut s = String::from("\"");
            for part in parts {
                match part {
                    WidgetTemplatePart::Literal(lit) => {
                        s.push_str(&escape_swift_string(lit));
                    }
                    WidgetTemplatePart::Field(field) => {
                        write!(s, "\\({}.{})", entry_param, field).unwrap();
                    }
                    WidgetTemplatePart::Formatted(expr) => {
                        write!(s, "\\({})", emit_format_swift(expr, entry_param)).unwrap();
                    }
                }
            }
            s.push('"');
            s
        }
        WidgetTextContent::Formatted(expr) => {
            // Wrap the produced Swift expression in a Swift string so
            // Text(...) consumes a String literal — same convention as
            // the Field arm above.
            format!("\"\\({})\"", emit_format_swift(expr, entry_param))
        }
    }
}

/// Emit a Swift expression for one of the whitelisted formatter calls.
/// The result is a `String`-typed expression suitable for dropping into
/// `Text(...)` directly or interpolating with `\(...)`.
fn emit_format_swift(expr: &WidgetFormatExpr, entry_param: &str) -> String {
    let value = emit_format_arg_swift(&expr.arg, entry_param);
    match expr.call {
        WidgetFormatCall::StringCast | WidgetFormatCall::ToString => {
            format!("String({})", value)
        }
        WidgetFormatCall::NumberCast => {
            // `Number(x)` is rarely useful in a render position; coerce
            // through Double so callers can interpolate it.
            format!("Double({}) ?? 0", value)
        }
        WidgetFormatCall::Round => format!("Int({}.rounded())", value),
        WidgetFormatCall::Floor => format!("Int({}.rounded(.down))", value),
        WidgetFormatCall::Ceil => format!("Int({}.rounded(.up))", value),
        WidgetFormatCall::ToFixed { digits } => {
            format!("String(format: \"%.{}f\", {})", digits, value)
        }
    }
}

fn emit_format_arg_swift(arg: &WidgetFormatArg, entry_param: &str) -> String {
    match arg {
        WidgetFormatArg::Field(field) => format!("{}.{}", entry_param, field),
        WidgetFormatArg::Number(n) => format_f64(*n),
        WidgetFormatArg::String(s) => format!("\"{}\"", escape_swift_string(s)),
    }
}

/// Emit a condition expression
fn emit_condition(
    field: &str,
    op: &WidgetConditionOp,
    value: &WidgetTextContent,
    entry_param: &str,
) -> String {
    let lhs = format!("{}.{}", entry_param, field);
    match op {
        WidgetConditionOp::Truthy => lhs,
        WidgetConditionOp::GreaterThan => {
            format!("{} > {}", lhs, emit_condition_value(value))
        }
        WidgetConditionOp::LessThan => {
            format!("{} < {}", lhs, emit_condition_value(value))
        }
        WidgetConditionOp::Equals => {
            format!("{} == {}", lhs, emit_condition_value(value))
        }
        WidgetConditionOp::NotEquals => {
            format!("{} != {}", lhs, emit_condition_value(value))
        }
    }
}

fn emit_condition_value(value: &WidgetTextContent) -> String {
    match value {
        WidgetTextContent::Literal(s) => {
            // Try as number first
            if let Ok(n) = s.parse::<f64>() {
                format_f64(n)
            } else {
                format!("\"{}\"", escape_swift_string(s))
            }
        }
        WidgetTextContent::Field(f) => f.clone(),
        WidgetTextContent::Template(_) => "\"\"".to_string(),
        // Conditions compare against constants today, not formatted
        // expressions — fall through to an empty literal so we don't
        // emit a syntactically dubious comparison.
        WidgetTextContent::Formatted(_) => "\"\"".to_string(),
    }
}

/// Emit SwiftUI modifiers as chained method calls
fn emit_modifiers(out: &mut String, modifiers: &[WidgetModifier], _indent: usize) {
    let pad = "    ".repeat(_indent);
    for modifier in modifiers {
        match modifier {
            WidgetModifier::Font(font) => {
                let font_str = match font {
                    WidgetFont::System(size) => format!(".system(size: {})", format_f64(*size)),
                    WidgetFont::Named(name) => {
                        format!(".custom(\"{}\", size: 17)", escape_swift_string(name))
                    }
                    WidgetFont::Headline => ".headline".to_string(),
                    WidgetFont::Title => ".title".to_string(),
                    WidgetFont::Title2 => ".title2".to_string(),
                    WidgetFont::Title3 => ".title3".to_string(),
                    WidgetFont::Body => ".body".to_string(),
                    WidgetFont::Caption => ".caption".to_string(),
                    WidgetFont::Caption2 => ".caption2".to_string(),
                    WidgetFont::Footnote => ".footnote".to_string(),
                    WidgetFont::Subheadline => ".subheadline".to_string(),
                    WidgetFont::LargeTitle => ".largeTitle".to_string(),
                };
                write!(out, "\n{}    .font({})", pad, font_str).unwrap();
            }
            WidgetModifier::FontWeight(weight) => {
                write!(out, "\n{}    .fontWeight(.{})", pad, weight).unwrap();
            }
            WidgetModifier::ForegroundColor(color) => {
                let swift_color = swift_color_expr(color);
                write!(out, "\n{}    .foregroundColor({})", pad, swift_color).unwrap();
            }
            WidgetModifier::Padding(p) => {
                write!(out, "\n{}    .padding({})", pad, format_f64(*p)).unwrap();
            }
            WidgetModifier::Frame { width, height } => {
                let mut args = Vec::new();
                if let Some(w) = width {
                    args.push(format!("width: {}", format_f64(*w)));
                }
                if let Some(h) = height {
                    args.push(format!("height: {}", format_f64(*h)));
                }
                if !args.is_empty() {
                    write!(out, "\n{}    .frame({})", pad, args.join(", ")).unwrap();
                }
            }
            WidgetModifier::CornerRadius(r) => {
                write!(out, "\n{}    .cornerRadius({})", pad, format_f64(*r)).unwrap();
            }
            WidgetModifier::Background(color) => {
                let swift_color = swift_color_expr(color);
                write!(out, "\n{}    .background({})", pad, swift_color).unwrap();
            }
            WidgetModifier::Opacity(o) => {
                write!(out, "\n{}    .opacity({})", pad, format_f64(*o)).unwrap();
            }
            WidgetModifier::LineLimit(n) => {
                write!(out, "\n{}    .lineLimit({})", pad, n).unwrap();
            }
            WidgetModifier::Multiline => {
                write!(out, "\n{}    .lineLimit(nil)", pad).unwrap();
            }
            WidgetModifier::MinimumScaleFactor(v) => {
                write!(out, "\n{}    .minimumScaleFactor({})", pad, format_f64(*v)).unwrap();
            }
            WidgetModifier::ContainerBackground(color) => {
                let swift_color = swift_color_expr(color);
                write!(
                    out,
                    "\n{}    .containerBackground({}.gradient, for: .widget)",
                    pad, swift_color
                )
                .unwrap();
            }
            WidgetModifier::FrameMaxWidth => {
                write!(out, "\n{}    .frame(maxWidth: .infinity)", pad).unwrap();
            }
            WidgetModifier::WidgetURL(url) => {
                write!(
                    out,
                    "\n{}    .widgetURL(URL(string: \"{}\")!)",
                    pad,
                    escape_swift_string(url)
                )
                .unwrap();
            }
            WidgetModifier::PaddingEdge { edge, value } => {
                write!(
                    out,
                    "\n{}    .padding(.{}, {})",
                    pad,
                    edge,
                    format_f64(*value)
                )
                .unwrap();
            }
        }
    }
}

/// Convert a color name to a Swift Color expression
fn swift_color_expr(color: &str) -> String {
    match color {
        "red" => "Color.red".to_string(),
        "blue" => "Color.blue".to_string(),
        "green" => "Color.green".to_string(),
        "white" => "Color.white".to_string(),
        "black" => "Color.black".to_string(),
        "gray" | "grey" => "Color.gray".to_string(),
        "orange" => "Color.orange".to_string(),
        "yellow" => "Color.yellow".to_string(),
        "purple" => "Color.purple".to_string(),
        "pink" => "Color.pink".to_string(),
        "primary" => "Color.primary".to_string(),
        "secondary" => "Color.secondary".to_string(),
        "clear" => "Color.clear".to_string(),
        _ => {
            // Try hex color
            if color.starts_with('#') && color.len() == 7 {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    u8::from_str_radix(&color[1..3], 16),
                    u8::from_str_radix(&color[3..5], 16),
                    u8::from_str_radix(&color[5..7], 16),
                ) {
                    return format!(
                        "Color(red: {:.3}, green: {:.3}, blue: {:.3})",
                        r as f64 / 255.0,
                        g as f64 / 255.0,
                        b as f64 / 255.0
                    );
                }
            }
            format!("Color.{}", color)
        }
    }
}

/// Format f64 without trailing zeros
fn format_f64(v: f64) -> String {
    if v == v.floor() {
        format!("{:.0}", v)
    } else {
        format!("{}", v)
    }
}

/// Escape a string for Swift string literals
fn escape_swift_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_widget(
        kind: &str,
        entry_fields: Vec<(String, WidgetFieldType)>,
        render_body: Vec<WidgetNode>,
    ) -> WidgetDecl {
        WidgetDecl {
            kind: kind.to_string(),
            display_name: String::new(),
            description: String::new(),
            supported_families: vec![],
            entry_fields,
            render_body,
            entry_param_name: "entry".to_string(),
            config_params: vec![],
            provider_func_name: None,
            placeholder: None,
            family_param_name: None,
            app_group: None,
            reload_after_seconds: None,
        }
    }

    #[test]
    fn test_native_provider_custom_reload_policy() {
        let mut widget = make_widget(
            "com.test.Reload",
            vec![("value".to_string(), WidgetFieldType::Number)],
            vec![],
        );
        widget.provider_func_name = Some("__widget_provider_Reload".to_string());
        widget.reload_after_seconds = Some(3600);

        let swift = emit_timeline_provider(&widget, "Reload");
        assert!(swift.contains("Date().addingTimeInterval(3600)"));
        assert!(swift.contains("policy: .after(reloadDate)"));
        assert!(!swift.contains("addingTimeInterval(1800)"));
    }

    #[test]
    fn test_native_provider_default_reload_policy() {
        let mut widget = make_widget(
            "com.test.Reload",
            vec![("value".to_string(), WidgetFieldType::Number)],
            vec![],
        );
        widget.provider_func_name = Some("__widget_provider_Reload".to_string());
        // reload_after_seconds stays None → 30-minute default.

        let swift = emit_timeline_provider(&widget, "Reload");
        assert!(swift.contains("Date().addingTimeInterval(1800)"));
    }

    #[test]
    fn test_native_provider_reload_policy_clamped_to_widgetkit_floor() {
        let mut widget = make_widget(
            "com.test.Reload",
            vec![("value".to_string(), WidgetFieldType::Number)],
            vec![],
        );
        widget.provider_func_name = Some("__widget_provider_Reload".to_string());
        // 5 minutes — below the 15-minute WidgetKit floor, must clamp to 900s.
        widget.reload_after_seconds = Some(300);

        let swift = emit_timeline_provider(&widget, "Reload");
        assert!(swift.contains("Date().addingTimeInterval(900)"));
        assert!(!swift.contains("addingTimeInterval(300)\n"));
    }

    #[test]
    fn test_app_intent_provider_custom_reload_policy() {
        let mut widget = make_widget(
            "com.test.Reload",
            vec![("value".to_string(), WidgetFieldType::Number)],
            vec![],
        );
        widget.provider_func_name = Some("__widget_provider_Reload".to_string());
        widget.config_params = vec![WidgetConfigParam {
            name: "site".to_string(),
            title: "Site".to_string(),
            param_type: WidgetConfigParamType::String {
                default: String::new(),
            },
        }];
        widget.reload_after_seconds = Some(2700);

        let swift = emit_timeline_provider(&widget, "Reload");
        assert!(swift.contains("AppIntentTimelineProvider"));
        assert!(swift.contains("Date().addingTimeInterval(2700)"));
    }

    #[test]
    fn test_simple_text_widget() {
        let widget = WidgetDecl {
            kind: "com.example.Hello".to_string(),
            display_name: "Hello Widget".to_string(),
            description: "A simple hello widget".to_string(),
            supported_families: vec!["systemSmall".to_string()],
            entry_fields: vec![("greeting".to_string(), WidgetFieldType::String)],
            render_body: vec![WidgetNode::Text {
                content: WidgetTextContent::Field("greeting".to_string()),
                modifiers: vec![
                    WidgetModifier::Font(WidgetFont::Title),
                    WidgetModifier::ForegroundColor("blue".to_string()),
                ],
            }],
            entry_param_name: "entry".to_string(),
            config_params: vec![],
            provider_func_name: None,
            placeholder: None,
            family_param_name: None,
            app_group: None,
            reload_after_seconds: None,
        };

        let view = emit_view(&widget, "Hello");
        assert!(view.contains("struct HelloView: View"));
        assert!(view.contains("let entry: HelloEntry"));
        assert!(view.contains("Text(\"\\(entry.greeting)\")"));
        assert!(view.contains(".font(.title)"));
        assert!(view.contains(".foregroundColor(Color.blue)"));
    }

    #[test]
    fn test_vstack_with_children() {
        let widget = make_widget(
            "com.test.Stack",
            vec![
                ("title".to_string(), WidgetFieldType::String),
                ("count".to_string(), WidgetFieldType::Number),
            ],
            vec![WidgetNode::Stack {
                kind: WidgetStackKind::VStack,
                spacing: Some(8.0),
                children: vec![
                    WidgetNode::Text {
                        content: WidgetTextContent::Field("title".to_string()),
                        modifiers: vec![WidgetModifier::Font(WidgetFont::Headline)],
                    },
                    WidgetNode::Text {
                        content: WidgetTextContent::Template(vec![
                            WidgetTemplatePart::Literal("Count: ".to_string()),
                            WidgetTemplatePart::Field("count".to_string()),
                        ]),
                        modifiers: vec![],
                    },
                ],
                modifiers: vec![WidgetModifier::Padding(16.0)],
            }],
        );

        let view = emit_view(&widget, "Stack");
        assert!(view.contains("VStack(spacing: 8)"));
        assert!(view.contains(".padding(16)"));
        assert!(view.contains("Text(\"Count: \\(entry.count)\")"));
    }

    #[test]
    fn test_entry_struct_with_array() {
        let widget = make_widget(
            "com.test.Entry",
            vec![
                (
                    "sites".to_string(),
                    WidgetFieldType::Array(Box::new(WidgetFieldType::Object(vec![
                        ("url".to_string(), WidgetFieldType::String),
                        ("clicks".to_string(), WidgetFieldType::Number),
                    ]))),
                ),
                ("totalClicks".to_string(), WidgetFieldType::Number),
                (
                    "error".to_string(),
                    WidgetFieldType::Optional(Box::new(WidgetFieldType::String)),
                ),
            ],
            vec![],
        );

        let s = emit_entry_struct(&widget, "Entry");
        assert!(s.contains("struct EntrySitesItem: Codable, Identifiable, Hashable"));
        assert!(s.contains("let url: String"));
        assert!(s.contains("let clicks: Double"));
        assert!(s.contains("let sites: [EntrySitesItem]"));
        assert!(s.contains("let totalClicks: Double"));
        assert!(s.contains("let error: String?"));
    }

    #[test]
    fn test_timeline_decode_array_of_objects() {
        // Regression test for #5070: an array-of-objects entry field must decode
        // the JSON array into the nested Codable struct, not cast it to String.
        let mut widget = make_widget(
            "com.test.TopSites",
            vec![
                (
                    "sites".to_string(),
                    WidgetFieldType::Array(Box::new(WidgetFieldType::Object(vec![
                        ("siteUrl".to_string(), WidgetFieldType::String),
                        ("clicks".to_string(), WidgetFieldType::String),
                    ]))),
                ),
                ("totalClicks".to_string(), WidgetFieldType::Number),
                (
                    "tags".to_string(),
                    WidgetFieldType::Array(Box::new(WidgetFieldType::String)),
                ),
            ],
            vec![],
        );
        widget.provider_func_name = Some("topSitesProvider".to_string());

        let provider = emit_timeline_provider(&widget, "TopSites");

        // The decode site must NOT cast the object array to String.
        assert!(
            !provider.contains("sites: entryDict[\"sites\"] as? String"),
            "array-of-objects field must not be decoded as String:\n{provider}"
        );
        // It must decode into the nested Codable struct type.
        assert!(
            provider.contains("JSONDecoder().decode([TopSitesSitesItem].self"),
            "expected JSONDecoder decode into [TopSitesSitesItem]:\n{provider}"
        );
        assert!(
            provider.contains("sites: (entryDict[\"sites\"])"),
            "expected sites to be assigned from entryDict[\"sites\"]:\n{provider}"
        );
        // Arrays of scalars cast directly.
        assert!(provider.contains("tags: entryDict[\"tags\"] as? [String] ?? []"));
        // Scalars are unchanged.
        assert!(provider.contains("totalClicks: entryDict[\"totalClicks\"] as? Double ?? 0"));
    }

    #[test]
    fn test_conditional() {
        let widget = make_widget(
            "com.test.Cond",
            vec![("count".to_string(), WidgetFieldType::Number)],
            vec![WidgetNode::Conditional {
                field: "count".to_string(),
                op: WidgetConditionOp::GreaterThan,
                value: WidgetTextContent::Literal("0".to_string()),
                then_node: Box::new(WidgetNode::Text {
                    content: WidgetTextContent::Literal("Has items".to_string()),
                    modifiers: vec![],
                }),
                else_node: Some(Box::new(WidgetNode::Text {
                    content: WidgetTextContent::Literal("Empty".to_string()),
                    modifiers: vec![],
                })),
            }],
        );

        let view = emit_view(&widget, "Cond");
        assert!(view.contains("if entry.count > 0"));
        assert!(view.contains("Text(\"Has items\")"));
        assert!(view.contains("} else {"));
        assert!(view.contains("Text(\"Empty\")"));
    }

    #[test]
    fn test_foreach() {
        let widget = make_widget(
            "com.test.ForEach",
            vec![(
                "items".to_string(),
                WidgetFieldType::Array(Box::new(WidgetFieldType::String)),
            )],
            vec![WidgetNode::ForEach {
                collection_field: "items".to_string(),
                item_param: "item".to_string(),
                body: Box::new(WidgetNode::Text {
                    content: WidgetTextContent::Field("item".to_string()),
                    modifiers: vec![],
                }),
            }],
        );

        let view = emit_view(&widget, "ForEach");
        assert!(view.contains("ForEach(entry.items, id: \\.self) { item in"));
    }

    #[test]
    fn test_divider_and_label() {
        let widget = make_widget(
            "com.test.Label",
            vec![],
            vec![
                WidgetNode::Label {
                    text: WidgetTextContent::Literal("Favorites".to_string()),
                    system_image: "star.fill".to_string(),
                    modifiers: vec![WidgetModifier::Font(WidgetFont::Caption)],
                },
                WidgetNode::Divider,
            ],
        );

        let view = emit_view(&widget, "Label");
        assert!(view.contains("Label(\"Favorites\", systemImage: \"star.fill\")"));
        assert!(view.contains("Divider()"));
    }

    #[test]
    fn test_gauge() {
        let widget = make_widget(
            "com.test.Gauge",
            vec![("progress".to_string(), WidgetFieldType::Number)],
            vec![WidgetNode::Gauge {
                value_expr: "progress".to_string(),
                label: "Done".to_string(),
                style: GaugeStyle::Circular,
                modifiers: vec![],
            }],
        );

        let view = emit_view(&widget, "Gauge");
        assert!(view.contains("Gauge(value: entry.progress)"));
        assert!(view.contains("Text(\"Done\")"));
        assert!(view.contains(".gaugeStyle(.accessoryCircularCapacity)"));
    }

    #[test]
    fn test_family_switch() {
        let widget = make_widget(
            "com.test.Family",
            vec![("title".to_string(), WidgetFieldType::String)],
            vec![WidgetNode::FamilySwitch {
                cases: vec![(
                    "systemSmall".to_string(),
                    WidgetNode::Text {
                        content: WidgetTextContent::Literal("Small".to_string()),
                        modifiers: vec![],
                    },
                )],
                default: Some(Box::new(WidgetNode::Text {
                    content: WidgetTextContent::Literal("Default".to_string()),
                    modifiers: vec![],
                })),
            }],
        );

        let mut w = widget.clone();
        w.family_param_name = Some("family".to_string());
        let view = emit_view(&w, "Family");
        assert!(view.contains("@Environment(\\.widgetFamily) var family"));
        assert!(view.contains("switch family"));
        assert!(view.contains("case .systemSmall:"));
        assert!(view.contains("Text(\"Small\")"));
        assert!(view.contains("default:"));
        assert!(view.contains("Text(\"Default\")"));
    }

    #[test]
    fn test_new_modifiers() {
        let widget = make_widget(
            "com.test.Modifiers",
            vec![],
            vec![WidgetNode::Text {
                content: WidgetTextContent::Literal("Hello".to_string()),
                modifiers: vec![
                    WidgetModifier::MinimumScaleFactor(0.5),
                    WidgetModifier::FrameMaxWidth,
                    WidgetModifier::WidgetURL("myapp://home".to_string()),
                    WidgetModifier::ContainerBackground("blue".to_string()),
                ],
            }],
        );

        let view = emit_view(&widget, "Modifiers");
        assert!(view.contains(".minimumScaleFactor(0.5)"));
        assert!(view.contains(".frame(maxWidth: .infinity)"));
        assert!(view.contains(".widgetURL(URL(string: \"myapp://home\")!)"));
        assert!(view.contains(".containerBackground(Color.blue.gradient, for: .widget)"));
    }

    #[test]
    fn format_to_fixed_emits_string_format() {
        let s = emit_format_swift(
            &WidgetFormatExpr {
                call: WidgetFormatCall::ToFixed { digits: 2 },
                arg: WidgetFormatArg::Field("totalClicks".to_string()),
            },
            "entry",
        );
        assert_eq!(s, "String(format: \"%.2f\", entry.totalClicks)");
    }

    #[test]
    fn format_math_round_emits_rounded_int() {
        let s = emit_format_swift(
            &WidgetFormatExpr {
                call: WidgetFormatCall::Round,
                arg: WidgetFormatArg::Field("totalClicks".to_string()),
            },
            "entry",
        );
        assert_eq!(s, "Int(entry.totalClicks.rounded())");
    }

    #[test]
    fn format_math_floor_and_ceil_emit_directed_rounding() {
        let floor = emit_format_swift(
            &WidgetFormatExpr {
                call: WidgetFormatCall::Floor,
                arg: WidgetFormatArg::Field("totalClicks".to_string()),
            },
            "entry",
        );
        assert_eq!(floor, "Int(entry.totalClicks.rounded(.down))");
        let ceil = emit_format_swift(
            &WidgetFormatExpr {
                call: WidgetFormatCall::Ceil,
                arg: WidgetFormatArg::Field("totalClicks".to_string()),
            },
            "entry",
        );
        assert_eq!(ceil, "Int(entry.totalClicks.rounded(.up))");
    }

    #[test]
    fn format_string_cast_wraps_in_string_init() {
        let s = emit_format_swift(
            &WidgetFormatExpr {
                call: WidgetFormatCall::StringCast,
                arg: WidgetFormatArg::Field("totalClicks".to_string()),
            },
            "entry",
        );
        assert_eq!(s, "String(entry.totalClicks)");
    }

    #[test]
    fn text_formatted_render_replaces_empty_string_bug() {
        // Issue #1179 follow-up: rendering `Text(Math.round(entry.totalClicks))`
        // used to emit `Text("")`. The new path produces a proper Swift
        // string interpolation through the whitelisted format helper.
        let widget = make_widget(
            "com.test.Formatted",
            vec![("totalClicks".to_string(), WidgetFieldType::Number)],
            vec![WidgetNode::Text {
                content: WidgetTextContent::Formatted(WidgetFormatExpr {
                    call: WidgetFormatCall::Round,
                    arg: WidgetFormatArg::Field("totalClicks".to_string()),
                }),
                modifiers: vec![],
            }],
        );
        let view = emit_view(&widget, "Formatted");
        assert!(view.contains("Text(\"\\(Int(entry.totalClicks.rounded()))\")"));
        // And it must not collapse to the empty-string bug.
        assert!(!view.contains("Text(\"\")"));
    }

    #[test]
    fn shared_runtime_emits_internal_ffi_once() {
        // #5069: the perry-runtime FFI helpers must be `internal` (not
        // `private`) so they're callable from the sibling `{Name}.swift`
        // files in the same swiftc module.
        let runtime = emit_shared_runtime(None);
        assert!(runtime.contains("func perry_runtime_widget_init()"));
        assert!(runtime.contains("func perry_nanbox_string(_ s: UnsafePointer<CChar>) -> Int64"));
        assert!(runtime.contains("func perry_get_string(_ val: Int64) -> String"));
        // No `private` — that's exactly the inaccessibility bug.
        assert!(!runtime.contains("private func"));
        // The shared-storage @_cdecl bridge is gated on an app group.
        assert!(!runtime.contains("@_cdecl"));
    }

    #[test]
    fn shared_runtime_includes_storage_bridge_with_app_group() {
        let runtime = emit_shared_runtime(Some("group.com.example.app"));
        assert!(runtime.contains("@_cdecl(\"perry_widget_shared_storage_get\")"));
        assert!(runtime.contains("UserDefaults(suiteName: \"group.com.example.app\")"));
    }

    #[test]
    fn per_widget_glue_only_declares_provider_extern() {
        // The per-widget glue must NOT re-emit the shared FFI helpers (that's
        // what caused the duplicate-symbol / inaccessibility regression). It
        // carries only this widget's unique provider import.
        let mut widget = make_widget("com.test.Glue", vec![], vec![]);
        widget.provider_func_name = Some("__widget_provider_quickstats".to_string());
        let glue = emit_glue(&widget, "Glue");
        assert!(glue.contains("@_silgen_name(\"__widget_provider_quickstats\")"));
        assert!(glue.contains("func __widget_provider_quickstats(_ configJson: Int64) -> Int64"));
        // Shared helpers and the @_cdecl bridge live in PerryWidgetRuntime.swift.
        assert!(!glue.contains("perry_runtime_widget_init"));
        assert!(!glue.contains("perry_get_string"));
        assert!(!glue.contains("@_cdecl"));
    }

    #[test]
    fn template_with_formatted_hole_interpolates_through_helper() {
        // ``Total: ${entry.totalClicks.toFixed(2)}`` should land as
        // `Text("Total: \(String(format: "%.2f", entry.totalClicks))")`.
        let widget = make_widget(
            "com.test.Template",
            vec![("totalClicks".to_string(), WidgetFieldType::Number)],
            vec![WidgetNode::Text {
                content: WidgetTextContent::Template(vec![
                    WidgetTemplatePart::Literal("Total: ".to_string()),
                    WidgetTemplatePart::Formatted(WidgetFormatExpr {
                        call: WidgetFormatCall::ToFixed { digits: 2 },
                        arg: WidgetFormatArg::Field("totalClicks".to_string()),
                    }),
                ]),
                modifiers: vec![],
            }],
        );
        let view = emit_view(&widget, "Template");
        assert!(view.contains("Text(\"Total: \\(String(format: \"%.2f\", entry.totalClicks))\")"));
    }
}
