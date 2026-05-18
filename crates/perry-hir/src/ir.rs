//! HIR (High-level Intermediate Representation) definitions
//!
//! The HIR is a typed, lowered representation of TypeScript that is
//! easier to compile to native code than the raw AST.

use perry_types::{FuncId, GlobalId, LocalId, Type, TypeParam};

/// TypedArray element-kind tags. Must match `crates/perry-runtime/src/typedarray.rs`.
pub const TYPED_ARRAY_KIND_INT8: u8 = 0;
pub const TYPED_ARRAY_KIND_UINT8: u8 = 1;
pub const TYPED_ARRAY_KIND_INT16: u8 = 2;
pub const TYPED_ARRAY_KIND_UINT16: u8 = 3;
pub const TYPED_ARRAY_KIND_INT32: u8 = 4;
pub const TYPED_ARRAY_KIND_UINT32: u8 = 5;
pub const TYPED_ARRAY_KIND_FLOAT32: u8 = 6;
pub const TYPED_ARRAY_KIND_FLOAT64: u8 = 7;
/// Uint8ClampedArray: 1-byte elements, stores via ToUint8Clamp (not truncate-wrap).
pub const TYPED_ARRAY_KIND_UINT8_CLAMPED: u8 = 8;

/// Map a class name (e.g. "Int32Array") to its `TYPED_ARRAY_KIND_*` tag.
pub fn typed_array_kind_for_name(name: &str) -> Option<u8> {
    match name {
        "Int8Array" => Some(TYPED_ARRAY_KIND_INT8),
        "Uint8Array" => Some(TYPED_ARRAY_KIND_UINT8),
        "Uint8ClampedArray" => Some(TYPED_ARRAY_KIND_UINT8_CLAMPED),
        "Int16Array" => Some(TYPED_ARRAY_KIND_INT16),
        "Uint16Array" => Some(TYPED_ARRAY_KIND_UINT16),
        "Int32Array" => Some(TYPED_ARRAY_KIND_INT32),
        "Uint32Array" => Some(TYPED_ARRAY_KIND_UINT32),
        "Float32Array" => Some(TYPED_ARRAY_KIND_FLOAT32),
        "Float64Array" => Some(TYPED_ARRAY_KIND_FLOAT64),
        _ => None,
    }
}

/// Known native module names that map to stdlib implementations.
/// These are npm packages that have native Rust replacements.
///
/// Source of truth lives in `perry-api-manifest::NATIVE_MODULES`
/// (#463 — the manifest is the unified source for compile-time
/// unimplemented-API checks, codegen dispatch, and docs/.d.ts emit
/// per #465). This re-export keeps existing
/// `perry_hir::NATIVE_MODULES` callers working; new code should
/// import from `perry_api_manifest` directly.
pub const NATIVE_MODULES: &[&str] = perry_api_manifest::NATIVE_MODULES;

thread_local! {
    /// Refs #665: per-thread set of packages the user opted into via
    /// `perry.compilePackages`. When non-empty, `is_native_module` returns
    /// false for any path whose package name is in this set — so HIR
    /// lowering treats the import as a regular ESM/CJS module (running
    /// cjs_wrap, registering classes as imported rather than native), and
    /// `obj.method` on a compile-package-overridden class lowers as a
    /// real PropertyGet instead of a zero-arg `NativeMethodCall` (which
    /// would have called the missing FFI getter and returned `0.0`).
    ///
    /// The compiler driver sets this thread-local before each
    /// `lower_module_full` invocation and clears it after. Rayon's
    /// thread pool gives each worker its own copy.
    static COMPILE_PACKAGES_OVERRIDE: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Set the per-thread override of which packages to treat as
/// non-native during HIR lowering. Called by the compiler driver before
/// each `lower_module_full` invocation. Refs #665.
pub fn set_compile_packages_override(set: std::collections::HashSet<String>) {
    COMPILE_PACKAGES_OVERRIDE.with(|cell| *cell.borrow_mut() = set);
}

/// Clear the per-thread override. Refs #665.
pub fn clear_compile_packages_override() {
    COMPILE_PACKAGES_OVERRIDE.with(|cell| cell.borrow_mut().clear());
}

// ---- #503 dynamic-stdlib-dispatch refusal config ----

thread_local! {
    /// #503: when true, HIR lowering refuses compile-time `obj[expr]()` /
    /// `obj[expr]` on known stdlib namespace receivers (`process`, `fs`,
    /// `crypto`, `child_process`, `net`, `os`, `path`, `http`, `https`,
    /// `stream`, `url`, `util`, `buffer`, `events`, `dns`, `tls`,
    /// `querystring`, `zlib`) unless the index is a string literal or
    /// compile-time-foldable string. Catches the dispatch-by-string class
    /// of supply-chain evasion. Set to false by `perry.allowDynamicStdlibDispatch: true`
    /// or `PERRY_ALLOW_DYNAMIC_STDLIB=1`.
    static REFUSE_DYNAMIC_STDLIB_DISPATCH: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };

    /// #503: per-thread set of npm package names that opted out of the
    /// dynamic-stdlib-dispatch refusal (`perry.allowDynamicStdlibDispatch:
    /// ["@scope/pkg", ...]`). When the currently-lowering source file
    /// belongs to one of these packages, the check is skipped.
    static ALLOW_DYNAMIC_STDLIB_PACKAGES: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());

    /// #503: source text of the module currently being lowered. Used by
    /// the dynamic-dispatch check to look up `// @perry-allow-dynamic`
    /// line annotations adjacent to violation sites. Set once per
    /// `lower_module_full` invocation by the compiler driver.
    static CURRENT_MODULE_SOURCE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// #503: enable (true) or disable (false) the dynamic-stdlib-dispatch
/// refusal pass. Default is true (refusal active). The compile driver
/// calls this once with the resolved configuration before kicking off
/// HIR lowering for the build.
pub fn set_refuse_dynamic_stdlib_dispatch(refuse: bool) {
    REFUSE_DYNAMIC_STDLIB_DISPATCH.with(|c| c.set(refuse));
}

/// #503: is the dynamic-stdlib-dispatch refusal pass currently enabled
/// for this thread?
pub fn refuse_dynamic_stdlib_dispatch_enabled() -> bool {
    REFUSE_DYNAMIC_STDLIB_DISPATCH.with(|c| c.get())
}

/// #503: install the per-thread allow-list of package names whose
/// modules may legitimately use dynamic dispatch on stdlib namespaces.
pub fn set_allow_dynamic_stdlib_packages(set: std::collections::HashSet<String>) {
    ALLOW_DYNAMIC_STDLIB_PACKAGES.with(|c| *c.borrow_mut() = set);
}

/// #503: clear the per-thread allow-list.
pub fn clear_allow_dynamic_stdlib_packages() {
    ALLOW_DYNAMIC_STDLIB_PACKAGES.with(|c| c.borrow_mut().clear());
}

/// #503: is the given package name on the allow-list? `package_name_of`
/// is the canonical extractor (scope-aware).
pub fn dynamic_stdlib_allowed_for_package(pkg: &str) -> bool {
    ALLOW_DYNAMIC_STDLIB_PACKAGES.with(|c| c.borrow().contains(pkg))
}

/// #503: install the source text of the module about to be lowered.
/// The dynamic-dispatch check reads this to look up site annotations
/// without re-reading the file from disk.
pub fn set_current_module_source(src: String) {
    CURRENT_MODULE_SOURCE.with(|c| *c.borrow_mut() = Some(src));
}

/// #503: clear the source-text thread-local.
pub fn clear_current_module_source() {
    CURRENT_MODULE_SOURCE.with(|c| *c.borrow_mut() = None);
}

/// #503: look up `// @perry-allow-dynamic` near `byte_offset` in the
/// currently-installed module source. Returns true if the annotation
/// appears on the same line as the offending site, or on any of the
/// contiguous comment/blank lines immediately above it (so authors can
/// stack other line comments like `// @ts-ignore` alongside the
/// annotation without losing the opt-out).
pub fn current_module_has_allow_dynamic_at(byte_offset: u32) -> bool {
    CURRENT_MODULE_SOURCE.with(|cell| {
        let borrowed = cell.borrow();
        let Some(src) = borrowed.as_ref() else {
            return false;
        };
        let offset = byte_offset as usize;
        if offset > src.len() {
            return false;
        }
        // Walk back to the start of the line containing `offset`.
        let line_start = src[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_end = src[offset..]
            .find('\n')
            .map(|i| offset + i)
            .unwrap_or(src.len());
        if src[line_start..line_end].contains("@perry-allow-dynamic") {
            return true;
        }
        // Walk up through contiguous comment-only and blank lines.
        // A "comment-only" line trims to either an empty string or a
        // string starting with `//`. The walk stops at the first line
        // that contains executable code — anything stronger would let
        // an annotation drift arbitrarily far from its target.
        let mut cursor = line_start;
        while cursor > 0 {
            let prev_end = cursor - 1; // index of '\n'
            let prev_start = src[..prev_end].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let prev = &src[prev_start..prev_end];
            let trimmed = prev.trim();
            let is_comment_or_blank = trimmed.is_empty() || trimmed.starts_with("//");
            if !is_comment_or_blank {
                return false;
            }
            if prev.contains("@perry-allow-dynamic") {
                return true;
            }
            cursor = prev_start;
        }
        false
    })
}

/// #503: extract the package name from a source file path, if any. The
/// path is searched for the rightmost `node_modules/` segment; the
/// segment(s) immediately following it form the package name
/// (scope-aware). Returns `None` for user-source files (no
/// `node_modules/` in the path).
pub fn package_name_for_source_path(source_path: &str) -> Option<&str> {
    let idx = source_path.rfind("node_modules/")?;
    let after = &source_path[idx + "node_modules/".len()..];
    if let Some(stripped) = after.strip_prefix('@') {
        // Scoped: `@scope/pkg/...` → `@scope/pkg`
        let mut parts = stripped.splitn(3, '/');
        let scope = parts.next().unwrap_or("");
        let pkg = parts.next().unwrap_or("");
        if scope.is_empty() || pkg.is_empty() {
            return None;
        }
        let end = idx + "node_modules/".len() + 1 + scope.len() + 1 + pkg.len();
        Some(&source_path[idx + "node_modules/".len()..end])
    } else {
        let pkg = after.split('/').next()?;
        if pkg.is_empty() {
            None
        } else {
            Some(pkg)
        }
    }
}

/// Parse the package name out of an import specifier. Mirrors the
/// `parse_package_specifier` helper in `crates/perry/src/commands/compile/resolve.rs`
/// but lives here so `is_native_module` doesn't gain a perry-crate dep.
fn package_name_of(path: &str) -> &str {
    let normalized = path.strip_prefix("node:").unwrap_or(path);
    if let Some(stripped) = normalized.strip_prefix('@') {
        // Scoped: `@scope/pkg/subpath` → `@scope/pkg`
        let mut parts = stripped.splitn(3, '/');
        let scope = parts.next().unwrap_or("");
        let pkg = parts.next().unwrap_or("");
        if scope.is_empty() || pkg.is_empty() {
            normalized
        } else {
            // Return a slice covering "@scope/pkg" from the original normalized
            // string. Since `stripped = &normalized[1..]`, the scope segment
            // ends at `1 + scope.len()` and "@scope/pkg" ends at
            // `1 + scope.len() + 1 + pkg.len()`.
            let end = 1 + scope.len() + 1 + pkg.len();
            &normalized[..end]
        }
    } else {
        // Regular: `pkg/subpath` → `pkg`
        normalized.split('/').next().unwrap_or(normalized)
    }
}

/// Check if a module path refers to a native stdlib module.
///
/// Refs #665: when the user has opted the package into
/// `perry.compilePackages`, this returns false even for paths that
/// match the built-in NATIVE_MODULES manifest — the user's
/// `node_modules` copy will be compiled from source and HIR lowering
/// must not register the import as a native module (which would
/// cascade into `obj.prop` being lowered as a zero-arg FFI getter call
/// instead of a real PropertyGet → bound-method-closure).
pub fn is_native_module(path: &str) -> bool {
    let normalized = path.strip_prefix("node:").unwrap_or(path);
    if !NATIVE_MODULES.contains(&normalized) {
        return false;
    }
    let pkg = package_name_of(path);
    let overridden = COMPILE_PACKAGES_OVERRIDE.with(|cell| cell.borrow().contains(pkg));
    !overridden
}

/// Check if a module path refers to a native module, including external native libraries.
/// External modules are provided by packages with `perry.nativeLibrary` in package.json.
pub fn is_native_module_with_externals(path: &str, externals: &[String]) -> bool {
    let normalized = path.strip_prefix("node:").unwrap_or(path);
    NATIVE_MODULES.contains(&normalized) || externals.iter().any(|ext| ext == normalized)
}

/// Check if a native module import requires linking perry-stdlib.
/// Returns false for modules that are handled entirely by perry-runtime.
///
/// `net` is intentionally absent from `RUNTIME_ONLY_MODULES` so this
/// returns true for `import 'net'` — the auto-optimizer needs that to
/// enable the `net` feature on perry-stdlib.
pub fn requires_stdlib(module: &str) -> bool {
    if !is_native_module(module) {
        return false;
    }
    !perry_api_manifest::is_runtime_only_module(module)
}

/// The kind of module being imported, determining how it's executed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModuleKind {
    /// Native TypeScript compiled to machine code (default for .ts/.tsx files)
    #[default]
    NativeCompiled,
    /// Native Rust stdlib implementation (mysql2, pg, etc.)
    NativeRust,
    /// V8-interpreted JavaScript (fallback for .js modules)
    /// This requires explicit opt-in and user confirmation
    Interpreted,
}

/// Whether a module is initialized eagerly (at program start, in topo order
/// across static imports) or lazily (on first dynamic `import()` resolving
/// to it). See issue #100.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModuleInitKind {
    /// Reachable from the entry through at least one static-import chain.
    #[default]
    Eager,
    /// Every path from the entry to this module goes through a dynamic
    /// `import()` edge — init runs on first dispatch.
    Deferred,
}

/// Determine the module kind for a given import path
pub fn determine_module_kind(source: &str, resolved_path: Option<&std::path::Path>) -> ModuleKind {
    // First check if it's a native Rust stdlib module
    if is_native_module(source) {
        return ModuleKind::NativeRust;
    }

    // Check the resolved path extension
    if let Some(path) = resolved_path {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            match ext {
                "ts" | "tsx" => return ModuleKind::NativeCompiled,
                "js" | "mjs" | "cjs" => return ModuleKind::Interpreted,
                _ => {}
            }
        }
    }

    // Default to native compiled (assume TypeScript)
    ModuleKind::NativeCompiled
}

/// Unique identifier for a class
pub type ClassId = u32;

/// Unique identifier for an enum
pub type EnumId = u32;

/// Unique identifier for an interface
pub type InterfaceId = u32;

/// Unique identifier for a type alias
pub type TypeAliasId = u32;

/// A complete HIR module (corresponds to one TypeScript file)
#[derive(Debug, Clone)]
pub struct Module {
    /// Module name/path
    pub name: String,
    /// Imports from other modules
    pub imports: Vec<Import>,
    /// Exports from this module
    pub exports: Vec<Export>,
    /// Class definitions
    pub classes: Vec<Class>,
    /// Interface definitions
    pub interfaces: Vec<Interface>,
    /// Type alias definitions
    pub type_aliases: Vec<TypeAlias>,
    /// Enum definitions
    pub enums: Vec<Enum>,
    /// Global variable declarations
    pub globals: Vec<Global>,
    /// Function definitions
    pub functions: Vec<Function>,
    /// Top-level statements to execute
    pub init: Vec<Stmt>,
    /// Exported native module instances: (export_name, module_name, class_name)
    /// This tracks variables like `export const pool = new Pool(...)` from pg
    pub exported_native_instances: Vec<(String, String, String)>,
    /// Exported functions that return native module instances: (func_name, module_name, class_name)
    /// e.g., `export function getRedis(): Promise<Redis>` -> ("getRedis", "ioredis", "Redis")
    pub exported_func_return_native_instances: Vec<(String, String, String)>,
    /// Exported object literals: export_name
    /// This tracks variables like `export const config = { ... }`
    pub exported_objects: Vec<String>,
    /// Exported functions that need globals for cross-module value passing
    /// This tracks functions like `export function foo() { ... }` or `export async function bar() { ... }`
    /// that may be imported and used as values (not just called) by other modules
    pub exported_functions: Vec<(String, FuncId)>,
    /// Widget extension declarations (perry/widget)
    pub widgets: Vec<WidgetDecl>,
    /// Whether this module uses fetch() — requires perry-stdlib for js_fetch_with_options
    pub uses_fetch: bool,
    /// Whether this module references `WebAssembly.*` (issue #76). Drives
    /// auto-link of `libperry_wasm_host.a` so users don't have to remember
    /// `--enable-wasm-runtime` when they actually use the API.
    pub uses_webassembly: bool,
    /// External FFI function declarations (name, param_types, return_type)
    /// Populated from `declare function` statements with no body.
    pub extern_funcs: Vec<(String, Vec<Type>, Type)>,
    /// Set to `true` by `perry_transform::unroll_static_loops` when any
    /// for-loop in `init` got unrolled. Mirrors `Function::was_unrolled`
    /// for top-level statements (which don't belong to a Function).
    /// Image_convolution puts its blur kernel directly at module init,
    /// not inside a function, so the codegen-side channel-vector SIMD
    /// gate consults this flag for module.init lowering.
    pub init_was_unrolled: bool,
    /// Issue #100: true iff this module's top-level `init` contains an
    /// `await` expression OUTSIDE any function/closure body. Drives the
    /// deferred-import dispatch to chain the init promise rather than
    /// returning a pre-resolved namespace.
    pub has_top_level_await: bool,
    /// Issue #100: eager vs deferred init. Modules reachable from the
    /// entry over only static-import edges init at program start (Eager).
    /// Modules only reachable through dynamic `import()` init lazily on
    /// the first dispatch (Deferred). Populated during `collect_modules`
    /// after the import graph is fully built.
    pub init_kind: ModuleInitKind,
    /// Issue #1021: closure func_ids whose body has been rewritten by
    /// `transform_async_to_generator` from a plain async closure into a
    /// generator + async-step driver. `compile_closure` consults this set
    /// to decide whether the closure body is already a state machine
    /// returning a Promise (no busy-wait pump needed). Populated by the
    /// transform pass; consumed by codegen.
    pub async_step_closures: std::collections::HashSet<perry_types::FuncId>,
}

/// A widget extension declaration (WidgetKit on iOS/watchOS, Glance on Android, Tiles on Wear OS)
#[derive(Debug, Clone)]
pub struct WidgetDecl {
    /// Widget kind identifier (e.g., "com.example.MyWidget")
    pub kind: String,
    /// Display name for the widget gallery
    pub display_name: String,
    /// Description for the widget gallery
    pub description: String,
    /// Supported widget families (e.g., "systemSmall", "systemMedium", "systemLarge",
    /// "accessoryCircular", "accessoryRectangular", "accessoryInline")
    pub supported_families: Vec<String>,
    /// Entry type fields: (name, type) — flattened from the TypeScript interface
    pub entry_fields: Vec<(String, WidgetFieldType)>,
    /// The render function body — compiled to SwiftUI/Compose source at compile time
    pub render_body: Vec<WidgetNode>,
    /// The render function's entry parameter name
    pub entry_param_name: String,
    /// AppIntent configuration parameters
    pub config_params: Vec<WidgetConfigParam>,
    /// Name of the lowered provider function (compiled via LLVM)
    pub provider_func_name: Option<String>,
    /// Placeholder data for widget gallery preview
    pub placeholder: Option<Vec<(String, WidgetPlaceholderValue)>>,
    /// Family parameter name in render function (for family-specific rendering)
    pub family_param_name: Option<String>,
    /// App group identifier for shared storage (e.g., "group.io.searchbird.shared")
    pub app_group: Option<String>,
    /// Timeline refresh interval in seconds
    pub reload_after_seconds: Option<u32>,
}

/// Configuration parameter for widget (AppIntent on iOS, Config Activity on Android)
#[derive(Debug, Clone)]
pub struct WidgetConfigParam {
    pub name: String,
    pub title: String,
    pub param_type: WidgetConfigParamType,
}

/// Configuration parameter type
#[derive(Debug, Clone)]
pub enum WidgetConfigParamType {
    Enum {
        values: Vec<String>,
        default: String,
    },
    Bool {
        default: bool,
    },
    String {
        default: String,
    },
}

/// Placeholder value for widget preview
#[derive(Debug, Clone)]
pub enum WidgetPlaceholderValue {
    String(String),
    Number(f64),
    Bool(bool),
    Array(Vec<WidgetPlaceholderValue>),
    Object(Vec<(String, WidgetPlaceholderValue)>),
    Null,
}

/// Supported field types in a widget entry
#[derive(Debug, Clone)]
pub enum WidgetFieldType {
    String,
    Number,
    Boolean,
    /// Array of a given element type (e.g., sites: Site[])
    Array(Box<WidgetFieldType>),
    /// Optional type (e.g., error?: string)
    Optional(Box<WidgetFieldType>),
    /// Nested object type with named fields (e.g., { url: string, clicks: number })
    Object(Vec<(String, WidgetFieldType)>),
}

/// A node in the widget render tree — declarative UI description
#[derive(Debug, Clone)]
pub enum WidgetNode {
    /// Text("hello") or Text(entry.field)
    Text {
        content: WidgetTextContent,
        modifiers: Vec<WidgetModifier>,
    },
    /// VStack/HStack/ZStack container
    Stack {
        kind: WidgetStackKind,
        spacing: Option<f64>,
        children: Vec<WidgetNode>,
        modifiers: Vec<WidgetModifier>,
    },
    /// Image(systemName: "star.fill")
    Image {
        system_name: String,
        modifiers: Vec<WidgetModifier>,
    },
    /// Spacer()
    Spacer,
    /// Conditional rendering: condition ? then : else
    Conditional {
        field: String,
        op: WidgetConditionOp,
        value: WidgetTextContent,
        then_node: Box<WidgetNode>,
        else_node: Option<Box<WidgetNode>>,
    },
    /// ForEach(entry.items, (item) => ...)
    ForEach {
        collection_field: String,
        item_param: String,
        body: Box<WidgetNode>,
    },
    /// Divider()
    Divider,
    /// Label("text", systemImage: "star.fill")
    Label {
        text: WidgetTextContent,
        system_image: String,
        modifiers: Vec<WidgetModifier>,
    },
    /// Family-specific rendering: switch on widget family
    FamilySwitch {
        cases: Vec<(String, WidgetNode)>,
        default: Option<Box<WidgetNode>>,
    },
    /// Gauge for watchOS complications
    Gauge {
        value_expr: String,
        label: String,
        style: GaugeStyle,
        modifiers: Vec<WidgetModifier>,
    },
}

/// Gauge display style (for watchOS complications / Wear OS tiles)
#[derive(Debug, Clone)]
pub enum GaugeStyle {
    /// Circular ring gauge (accessoryCircular)
    Circular,
    /// Horizontal bar gauge (accessoryRectangular)
    LinearCapacity,
}

/// Text content — either static string or entry field reference
#[derive(Debug, Clone)]
pub enum WidgetTextContent {
    /// Static string literal
    Literal(String),
    /// Reference to entry field (e.g., entry.title)
    Field(String),
    /// Template literal with parts: `Score: ${entry.score}`
    Template(Vec<WidgetTemplatePart>),
}

#[derive(Debug, Clone)]
pub enum WidgetTemplatePart {
    Literal(String),
    Field(String),
}

#[derive(Debug, Clone)]
pub enum WidgetStackKind {
    VStack,
    HStack,
    ZStack,
}

#[derive(Debug, Clone)]
pub enum WidgetConditionOp {
    GreaterThan,
    LessThan,
    Equals,
    NotEquals,
    Truthy,
}

/// Style modifiers for widget nodes
#[derive(Debug, Clone)]
pub enum WidgetModifier {
    Font(WidgetFont),
    FontWeight(String),
    ForegroundColor(String),
    Padding(f64),
    Frame {
        width: Option<f64>,
        height: Option<f64>,
    },
    CornerRadius(f64),
    Background(String),
    Opacity(f64),
    LineLimit(u32),
    Multiline,
    /// .minimumScaleFactor(0.5)
    MinimumScaleFactor(f64),
    /// .containerBackground(Color.blue.gradient, for: .widget)
    ContainerBackground(String),
    /// .frame(maxWidth: .infinity)
    FrameMaxWidth,
    /// Deep link URL on a view: .widgetURL(URL(string: "...")!)
    WidgetURL(String),
    /// Edge-specific padding: .padding(.leading, 8)
    PaddingEdge {
        edge: String,
        value: f64,
    },
}

#[derive(Debug, Clone)]
pub enum WidgetFont {
    System(f64),
    Named(String),
    Headline,
    Title,
    Title2,
    Title3,
    Body,
    Caption,
    Caption2,
    Footnote,
    Subheadline,
    LargeTitle,
}

/// An enum definition
#[derive(Debug, Clone)]
pub struct Enum {
    pub id: EnumId,
    pub name: String,
    pub members: Vec<EnumMember>,
    pub is_exported: bool,
}

/// An enum member
#[derive(Debug, Clone)]
pub struct EnumMember {
    pub name: String,
    pub value: EnumValue,
}

/// Value of an enum member
#[derive(Debug, Clone)]
pub enum EnumValue {
    /// Numeric value (auto-incremented or explicit)
    Number(i64),
    /// String value
    String(String),
}

/// An interface definition
#[derive(Debug, Clone)]
pub struct Interface {
    pub id: InterfaceId,
    pub name: String,
    /// Generic type parameters (e.g., T, K in interface<T, K>)
    pub type_params: Vec<TypeParam>,
    /// Extended interfaces
    pub extends: Vec<Type>,
    /// Property signatures
    pub properties: Vec<InterfaceProperty>,
    /// Method signatures
    pub methods: Vec<InterfaceMethod>,
    pub is_exported: bool,
}

/// A property in an interface
#[derive(Debug, Clone)]
pub struct InterfaceProperty {
    pub name: String,
    pub ty: Type,
    pub optional: bool,
    pub readonly: bool,
}

/// A method signature in an interface
#[derive(Debug, Clone)]
pub struct InterfaceMethod {
    pub name: String,
    /// Method's own type parameters (separate from interface's)
    pub type_params: Vec<TypeParam>,
    pub params: Vec<(String, Type, bool)>, // name, type, optional
    pub return_type: Type,
}

/// A type alias definition
#[derive(Debug, Clone)]
pub struct TypeAlias {
    pub id: TypeAliasId,
    pub name: String,
    /// Generic type parameters
    pub type_params: Vec<TypeParam>,
    /// The aliased type
    pub ty: Type,
    pub is_exported: bool,
}

/// An import declaration
#[derive(Debug, Clone)]
pub struct Import {
    /// Source module path (e.g., "./utils" or "fs")
    pub source: String,
    /// Import specifiers
    pub specifiers: Vec<ImportSpecifier>,
    /// True if this imports from a native stdlib module (mysql2, pg, etc.)
    pub is_native: bool,
    /// The kind of module (native compiled, native Rust, or V8 interpreted)
    pub module_kind: ModuleKind,
    /// Resolved absolute path to the module file (if available)
    pub resolved_path: Option<String>,
    /// True if the WHOLE import is type-only (`import type * as X`,
    /// `import type { Foo } from "..."`). Type-only imports are erased at
    /// runtime — they MUST NOT participate in module init order
    /// (refs #680). Pre-tracking they were treated like value imports,
    /// creating phantom init-order edges that flipped real cycles in the
    /// topological sort. Per-specifier type-only (`import { type Foo,
    /// bar }`) is still tracked because the same declaration also has
    /// value specifiers — only the whole-decl flag is runtime-meaningless.
    pub type_only: bool,
    /// Issue #100: synthesized from a dynamic `import()` call whose path
    /// const-folded to this source. Dynamic edges enter the import graph
    /// but do NOT pin the target as eager — if no static edge reaches it
    /// the target is `Deferred`. `specifiers` is empty for these.
    pub is_dynamic: bool,
}

/// Import specifier
#[derive(Debug, Clone)]
pub enum ImportSpecifier {
    /// Named import: import { foo, bar as baz } from "..."
    Named { imported: String, local: String },
    /// Default import: import foo from "..."
    Default { local: String },
    /// Namespace import: import * as foo from "..."
    Namespace { local: String },
}

/// An export declaration
#[derive(Debug, Clone)]
pub enum Export {
    /// Named export: export { foo, bar as baz }
    Named { local: String, exported: String },
    /// Re-export: export { foo } from "..."
    ReExport {
        source: String,
        imported: String,
        exported: String,
    },
    /// Export all: export * from "..."
    ExportAll { source: String },
    /// Namespace re-export: export * as Foo from "..."
    ///
    /// `name` is the local namespace alias the consumer sees as a Named
    /// import. The source module's full export surface is reachable via
    /// `<name>.<member>`, mirroring `import * as <name> from "..."` on
    /// the consumer side. Closes #310 (without this variant, SWC's
    /// `ExportSpecifier::Namespace` was silently dropped by the
    /// `ExportNamed` lowering's `if let Named` filter, so the re-exported
    /// file never entered the module graph and every `<name>.<member>`
    /// access lowered to 0).
    NamespaceReExport { source: String, name: String },
}

/// A class definition
#[derive(Debug, Clone)]
pub struct Class {
    pub id: ClassId,
    pub name: String,
    /// Generic type parameters (e.g., T, K, V in class<T, K, V>)
    pub type_params: Vec<TypeParam>,
    /// Parent class (for inheritance)
    pub extends: Option<ClassId>,
    /// Parent class name (for inheritance from imported classes where ClassId may not be known)
    pub extends_name: Option<String>,
    /// Native parent class (module_name, class_name) - e.g., ("events", "EventEmitter")
    pub native_extends: Option<(String, String)>,
    /// Issue #711: `class X extends fn(...)` / `class X extends Y.method(...)` —
    /// when the super-class expression is anything other than `Ident` or
    /// `Member` (i.e., not statically resolvable to a known class), we capture
    /// the lowered expression here. Codegen emits a runtime call
    /// `js_register_class_parent_dynamic(child_cid, eval(extends_expr))` at
    /// the source-order position of the class declaration so the parent edge
    /// in `CLASS_REGISTRY` is wired before the first instance is created.
    /// `extends` and `extends_name` are both `None` for these classes (the
    /// parent class_id is only known at runtime).
    pub extends_expr: Option<Box<Expr>>,
    /// Instance fields
    pub fields: Vec<ClassField>,
    /// Constructor (if any)
    pub constructor: Option<Function>,
    /// Instance methods
    pub methods: Vec<Function>,
    /// Getter methods (property_name -> function that returns the value)
    pub getters: Vec<(String, Function)>,
    /// Setter methods (property_name -> function that takes the value)
    pub setters: Vec<(String, Function)>,
    /// Static fields
    pub static_fields: Vec<ClassField>,
    /// Static methods
    pub static_methods: Vec<Function>,
    /// Legacy TypeScript decorators applied to the class.
    pub decorators: Vec<Decorator>,
    /// Whether this class is exported from the module
    pub is_exported: bool,
    /// Self-binding aliases for class-expression bindings:
    /// `var X = class _X { ... new _X() ... }` records `_X` here so codegen
    /// can look it up as the same class. Refs #486.
    pub aliases: Vec<String>,
}

/// A class field
#[derive(Debug, Clone)]
pub struct ClassField {
    pub name: String,
    /// When `Some`, this field's key is the lowered expression evaluated at
    /// construction time (e.g. `[Symbol.for("k")]` or `[Parent.Symbol.X]`).
    /// `name` is then a synthetic placeholder used only for HIR identity —
    /// runtime property writes go through `IndexSet` with this expression.
    pub key_expr: Option<Expr>,
    pub ty: Type,
    pub init: Option<Expr>,
    pub is_private: bool,
    pub is_readonly: bool,
    /// Legacy TypeScript decorators applied to this property.
    pub decorators: Vec<Decorator>,
}

/// A global variable
#[derive(Debug, Clone)]
pub struct Global {
    pub id: GlobalId,
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
    pub init: Option<Expr>,
}

/// A decorator applied to a method or class
#[derive(Debug, Clone)]
pub struct Decorator {
    /// The decorator function name (e.g., "log" for @log)
    pub name: String,
    /// Arguments if this is a decorator factory call (e.g., @log("prefix") -> args = ["prefix"])
    pub args: Vec<Expr>,
    /// True for decorator factories (`@dec(...)`), false for bare decorators (`@dec`).
    pub is_factory: bool,
    /// True for `@Reflect.metadata(key, value)`, which Perry lowers directly.
    pub is_reflect_metadata: bool,
}

/// A function definition
#[derive(Debug, Clone)]
pub struct Function {
    pub id: FuncId,
    pub name: String,
    /// Generic type parameters (e.g., T, K in function<T, K>)
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Vec<Stmt>,
    pub is_async: bool,
    pub is_generator: bool,
    pub is_exported: bool,
    /// Captured variables (for closures)
    pub captures: Vec<LocalId>,
    /// Decorators applied to this function/method
    pub decorators: Vec<Decorator>,
    /// Issue #256: true if this function was originally a plain async function
    /// that the async_to_generator pre-pass rewrote into a generator. The
    /// generator state-machine transform reads this flag and wraps the
    /// resulting iterator in an async-step driver so the function returns
    /// a Promise that respects spec microtask ordering.
    pub was_plain_async: bool,
    /// True if `perry_transform::unroll_static_loops` expanded any
    /// static-trip-count `for` loops in this function's body. Codegen
    /// reads this flag to decide whether to skip the manual `<4 x i32>`
    /// channel-vector reduction (which fights LLVM's freedom to choose
    /// vectorization shape across the unrolled body — the canonical
    /// case is image_convolution's 5×5 blur kernel where post-unroll
    /// `KERNEL[ky+2][kx+2]` constant-folds to integer literals and
    /// LLVM picks a better mul-by-shift shape than the pre-committed
    /// vector form). Default `false`. Pre-existing functions with no
    /// unrollable loops keep the manual SIMD path active for their
    /// (still-vectorizable) bodies.
    pub was_unrolled: bool,
}

/// A function parameter
#[derive(Debug, Clone)]
pub struct Param {
    pub id: LocalId,
    pub name: String,
    pub ty: Type,
    pub default: Option<Expr>,
    /// Legacy TypeScript decorators applied to this parameter.
    pub decorators: Vec<Decorator>,
    /// True if this is a rest parameter (...args)
    pub is_rest: bool,
}

/// Statement in function body
#[derive(Debug, Clone)]
pub enum Stmt {
    /// Local variable declaration: let/const x = expr
    Let {
        id: LocalId,
        name: String,
        ty: Type,
        mutable: bool,
        init: Option<Expr>,
    },
    /// Expression statement
    Expr(Expr),
    /// Return statement
    Return(Option<Expr>),
    /// If statement
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    /// While loop
    While { condition: Expr, body: Vec<Stmt> },
    /// Do-while loop (body runs at least once, condition checked at the end)
    DoWhile { body: Vec<Stmt>, condition: Expr },
    /// For loop (lowered from various JS for loops)
    For {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        update: Option<Expr>,
        body: Vec<Stmt>,
    },
    /// Labeled statement: `label: for/while/do/block`
    Labeled { label: String, body: Box<Stmt> },
    /// Break statement
    Break,
    /// Continue statement
    Continue,
    /// Labeled break: `break label;`
    LabeledBreak(String),
    /// Labeled continue: `continue label;`
    LabeledContinue(String),
    /// Throw statement
    Throw(Expr),
    /// Try-catch-finally
    Try {
        body: Vec<Stmt>,
        catch: Option<CatchClause>,
        finally: Option<Vec<Stmt>>,
    },
    /// Switch statement
    Switch {
        discriminant: Expr,
        cases: Vec<SwitchCase>,
    },
    /// Pre-allocate slot+box for a set of LocalIds at function-body
    /// entry. Emitted by `lower_fn_body_block_stmt` to support hoisted
    /// inner `function`-declarations that capture sibling FnDecls or
    /// forward `let`/`const` bindings whose own `Stmt::Let` would
    /// otherwise lazily allocate the box at source position. Issue #569.
    PreallocateBoxes(Vec<LocalId>),
}

/// A case in a switch statement
#[derive(Debug, Clone)]
pub struct SwitchCase {
    /// Test expression (None for default case)
    pub test: Option<Expr>,
    /// Statements in this case (including fallthrough)
    pub body: Vec<Stmt>,
}

/// Catch clause in try statement
#[derive(Debug, Clone)]
pub struct CatchClause {
    pub param: Option<(LocalId, String)>,
    pub body: Vec<Stmt>,
}

/// Expression
#[derive(Debug, Clone)]
pub enum Expr {
    // Literals
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    Integer(i64),   // Integer literal that fits in i64 (for optimization)
    BigInt(String), // Store as string to preserve precision
    String(String),
    /// String literal containing WTF-8 bytes (lone surrogates U+D800..U+DFFF).
    /// Raw WTF-8 bytes — cannot be represented as a valid Rust String.
    /// Lowers to js_string_from_wtf8_bytes at runtime.
    WtfString(Vec<u8>),
    /// Localizable string — resolved at compile time from locale files.
    /// The string_idx indexes into the global i18n string table (2D: [locale][key]).
    /// For parameterized strings like "Hello, {name}!", params contains the values to interpolate.
    /// For plural strings, plural_forms maps CLDR category (0-5) → string_idx.
    I18nString {
        key: String,
        string_idx: u32,
        /// Parameters for interpolation: (param_name, value_expr).
        /// Empty for simple strings like "Next".
        params: Vec<(String, Box<Expr>)>,
        /// Plural forms: (category_id, string_idx) pairs.
        /// Categories: 0=zero, 1=one, 2=two, 3=few, 4=many, 5=other.
        /// Empty for non-plural strings.
        plural_forms: Vec<(u8, u32)>,
        /// The param name that controls plural selection (e.g., "count").
        /// Only set when plural_forms is non-empty.
        plural_param: Option<String>,
    },

    // Variables
    LocalGet(LocalId),
    LocalSet(LocalId, Box<Expr>),
    GlobalGet(GlobalId),
    GlobalSet(GlobalId, Box<Expr>),

    // Update (++/--)
    Update {
        id: LocalId,
        op: UpdateOp,
        prefix: bool, // true for ++x, false for x++
    },

    // Operations
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },

    // Comparison
    Compare {
        op: CompareOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },

    // Logical
    Logical {
        op: LogicalOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },

    // Function call
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        /// Explicit type arguments (e.g., identity<number>(x))
        type_args: Vec<Type>,
    },

    /// Function call with spread arguments (e.g., fn(a, ...arr, b))
    CallSpread {
        callee: Box<Expr>,
        args: Vec<CallArg>,
        type_args: Vec<Type>,
    },

    // Named function reference
    FuncRef(FuncId),

    // External function reference (imported from another module)
    // Includes type information for proper code generation
    ExternFuncRef {
        name: String,
        param_types: Vec<Type>,
        return_type: Type,
    },

    // Native module reference (e.g., mysql2, pg)
    // The string is the module name, the local name is tracked separately
    NativeModuleRef(String),

    // Native module method call (e.g., mysql.createConnection, connection.query)
    // module: the native module name (e.g., "mysql2")
    // class_name: optional class name for distinguishing object types (e.g., "Pool" vs "Connection")
    // object: optional object to call method on (None for static methods like createConnection)
    // method: the method name
    // args: call arguments
    NativeMethodCall {
        module: String,
        class_name: Option<String>,
        object: Option<Box<Expr>>,
        method: String,
        args: Vec<Expr>,
    },

    // Object/property access
    PropertyGet {
        object: Box<Expr>,
        property: String,
    },
    PropertySet {
        object: Box<Expr>,
        property: String,
        value: Box<Expr>,
    },
    // Property update (++/--)
    PropertyUpdate {
        object: Box<Expr>,
        property: String,
        op: BinaryOp, // Add for ++, Sub for --
        prefix: bool, // true for ++x, false for x++
    },

    // Array/index access
    IndexGet {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    IndexSet {
        object: Box<Expr>,
        index: Box<Expr>,
        value: Box<Expr>,
    },
    // Index update (arr[i]++ or obj[key]++)
    IndexUpdate {
        object: Box<Expr>,
        index: Box<Expr>,
        op: BinaryOp, // Add for ++, Sub for --
        prefix: bool, // true for ++x, false for x++
    },

    // Object literal
    Object(Vec<(String, Expr)>),

    // Object literal with spread: { ...src, key: val, ...src2, key2: val2 }
    // Each part is (None, expr) for a spread source, or (Some(key), expr) for a static prop.
    // Parts are ordered to reflect JavaScript evaluation order (later props override earlier spreads).
    ObjectSpread {
        parts: Vec<(Option<String>, Expr)>,
    },

    // `Object.assign(target, ...sources)` — distinct from ObjectSpread because
    // the spec mutates `target` and returns it (preserving identity, class_id,
    // and the SYMBOL_PROPERTIES side-table entries). ObjectSpread allocates a
    // fresh object, which is wrong for `Object.assign` per #590.
    ObjectAssign {
        target: Box<Expr>,
        sources: Vec<Expr>,
    },

    // Array literal
    Array(Vec<Expr>),

    // Array literal with spread elements
    // Each element is either a regular expression (Left) or a spread expression (Right)
    ArraySpread(Vec<ArrayElement>),

    // Conditional expression (ternary)
    Conditional {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },

    // Type operations
    TypeOf(Box<Expr>),
    // Void operator: evaluate operand for side effects, return undefined
    Void(Box<Expr>),
    InstanceOf {
        expr: Box<Expr>,
        ty: String,
        /// Dynamic type expression — populated when `ty` is a runtime
        /// value (e.g. a function arg or a local holding a class ref)
        /// rather than a known compile-time class name. Codegen evaluates
        /// this expression and dispatches through `js_instanceof_dynamic`,
        /// which extracts the class_id from the INT32 NaN-tag at runtime.
        /// Refs #420 / #618 followup.
        ty_expr: Option<Box<Expr>>,
    },
    /// The 'in' operator: checks if property exists in object
    /// e.g., "prop" in obj or key in obj
    In {
        property: Box<Expr>,
        object: Box<Expr>,
    },

    // Await expression (for async functions)
    Await(Box<Expr>),

    // Yield expression (for generator functions)
    Yield {
        value: Option<Box<Expr>>,
        delegate: bool,
    },

    // New expression (class instantiation)
    New {
        class_name: String,
        args: Vec<Expr>,
        /// Explicit type arguments (e.g., new Box<number>(42))
        type_args: Vec<Type>,
    },

    /// Dynamic new expression (new with non-identifier callee)
    /// e.g., new (condition ? ClassA : ClassB)()
    /// or new someVariable()
    NewDynamic {
        /// The expression that evaluates to a constructor
        callee: Box<Expr>,
        /// Arguments to pass to the constructor
        args: Vec<Expr>,
    },

    // Class reference (for new expressions)
    ClassRef(String),

    // Enum member access (e.g., Color.Red)
    EnumMember {
        enum_name: String,
        member_name: String,
    },

    // Static field access (e.g., Counter.count)
    StaticFieldGet {
        class_name: String,
        field_name: String,
    },

    // Static field assignment (e.g., Counter.count = 5)
    StaticFieldSet {
        class_name: String,
        field_name: String,
        value: Box<Expr>,
    },

    // Static computed-key Symbol field assignment, e.g.
    // `class C { static [Symbol.for("k")] = "v" }`. Lowered at runtime
    // through `js_class_register_static_symbol(class_id, key, value)`.
    // Refs #420.
    ClassStaticSymbolSet {
        class_name: String,
        key: Box<Expr>,
        value: Box<Expr>,
    },

    // Issue #711: dynamic parent-class registration for
    // `class X extends fn(...)` shapes where the parent class_id is only
    // known at runtime. Emitted by lower.rs into module.init at the
    // source-order position of the class declaration. Codegen lowers
    // `parent_expr` to a Perry value, then calls
    // `js_register_class_parent_dynamic(class_id, value)` which reads the
    // value's class_id (via GcHeader for real objects, ClassRef tag for
    // class references) and wires the (child, parent) edge into
    // CLASS_REGISTRY. No-op if `parent_expr` evaluates to a value with no
    // class_id (e.g., a closure or primitive — preserves the "no parent"
    // baseline rather than crashing).
    RegisterClassParentDynamic {
        class_name: String,
        parent_expr: Box<Expr>,
    },

    /// Issue #894: `class C { static [keyExpr] = initExpr }` where the
    /// class is returned from a factory function body. The static-Symbol
    /// registration must re-run each time the factory is called, with
    /// the key/init evaluated against the current scope (closure
    /// captures + module lets that may have been assigned by user code
    /// between the class's HIR hoisting and the factory call).
    /// Sequenced in front of the `ClassRef` returned from the
    /// `ast::Expr::Class` lowering, parallel to
    /// `RegisterClassParentDynamic`. Codegen emits a call to
    /// `js_class_register_static_symbol(class_id, key, value)`.
    RegisterClassStaticSymbol {
        class_name: String,
        key_expr: Box<Expr>,
        value_expr: Box<Expr>,
    },

    // Issue #711 part 2: `<func_expr>.prototype = <obj_expr>` pattern,
    // used by Effect's effectable.ts to declare prototype-based
    // classes. Codegen emits a call to `js_set_function_prototype`
    // which stores `func_value → synthetic_class_id` in a side-table
    // and binds the object as the synthetic class's prototype source.
    // When `class Derived extends <func>` evaluates later, the dynamic
    // parent registration looks up that synthetic class_id and wires
    // it into CLASS_REGISTRY so method dispatch on Derived instances
    // walks through to the prototype object's methods.
    SetFunctionPrototype {
        func: Box<Expr>,
        proto: Box<Expr>,
    },

    // Issue #838: `<ClassName>.prototype.<method> = <fn>` and the
    // aliased shape `let p = <ClassName>.prototype; p.<method> = <fn>`.
    // dayjs / chalk / pre-ES6 npm packages still attach instance
    // methods via this pattern instead of inside the `class { … }`
    // block. Codegen emits `js_register_prototype_method(class_id,
    // name, fn)` which stores the closure into a per-class side
    // table; the runtime's `js_object_get_field_by_name` and
    // `js_native_call_method` dispatch hot paths consult it after
    // the regular vtable / proto-object walks miss, so
    // `(new Class()).method()` reaches the registered closure with
    // `this` bound to the receiver.
    RegisterPrototypeMethod {
        class_name: String,
        method_name: String,
        value: Box<Expr>,
    },

    // Issue #838 followup (b): function-classic prototype-method dispatch.
    // dayjs's minified bundle (and Babel's `var Foo = function(){ function
    // Foo(...){...}; var p = Foo.prototype; p.x = …; return Foo; }()`
    // emit pattern) declares its instance "class" via a function
    // declaration, not a `class` block. The #838 recogniser bailed
    // because `lookup_class("M")` returned None for function decls. This
    // node carries the function ref so codegen can pass the closure
    // value to `js_register_function_prototype_method` — the runtime
    // helper allocates a synthetic class id keyed by the closure's
    // bits and stores the method on `CLASS_PROTOTYPE_METHODS[cid]`.
    // Paired with `Expr::NewDynamic` lowering: when the callee is the
    // same function ref, the new-construct helper stamps the same
    // synthetic id on the instance, so dispatch finds the method via
    // the regular `(*obj).class_id → CLASS_PROTOTYPE_METHODS` walk.
    RegisterFunctionPrototypeMethod {
        func: Box<Expr>,
        method_name: String,
        value: Box<Expr>,
    },

    // Read side of the JS-classic prototype-method pattern:
    // `<funcDecl>.prototype.<name>` (or `<funcDecl>.prototype['<name>']`).
    // Returns the closure stored in `CLASS_PROTOTYPE_METHODS` for the
    // synthetic class id derived from the function value. Pre-fix this
    // shape lowered to `PropertyGet(PropertyGet(funcDecl, "prototype"),
    // name)` whose receiver evaluated to `undefined` — the user's
    // `typeof Foo.prototype.method` came back as `'undefined'` even
    // though `(new Foo()).method` reached the registered closure via
    // the side-table walk. Ramda's transducer pattern only needs the
    // assignment side, but the read side rounds out spec parity for
    // `Constructor.prototype.method` introspection.
    GetFunctionPrototypeMethod {
        func: Box<Expr>,
        method_name: String,
    },

    // Static method call (e.g., Counter.increment())
    StaticMethodCall {
        class_name: String,
        method_name: String,
        args: Vec<Expr>,
    },

    // This expression
    This,

    // Super constructor call: super(args)
    SuperCall(Vec<Expr>),

    // Super method call: super.method(args)
    SuperMethodCall {
        method: String,
        args: Vec<Expr>,
    },

    // Super property read (value form). super.<prop>. Resolved at
    // codegen by walking the parent class's method table (issue #774).
    SuperPropertyGet {
        property: String,
    },

    // Environment variable access: process.env.VARNAME
    EnvGet(String),
    // Dynamic environment variable access: process.env[expr]
    EnvGetDynamic(Box<Expr>),
    // Bare `process.env` as a value (not followed by .KEY) — materializes
    // the OS environment as a JS object. Used by patterns like
    // `const e = process.env`, `Object.keys(process.env)`, and indirect
    // access through `globalThis`/aliases where the static `.KEY` fast
    // path doesn't fire.
    ProcessEnv,
    // `globalThis` materialized as an actual object value (not the
    // `Expr::GlobalGet(0)` sentinel — that one routes by property
    // name from the parent PropertyGet/Call context and lowers to
    // the `0.0` placeholder when used bare). This variant lowers to
    // a real `js_get_global_this()` call so that
    // `Function('return this')()` (the canonical "get globalThis"
    // idiom every CJS/UMD library copies — lodash, underscore,
    // Effect, …) actually evaluates to the lazily-allocated global
    // singleton instead of `undefined`/`0.0`. Without this fold the
    // double call lowers as `GlobalGet(0)(literal)(): TypeError:
    // value is not a function` at module init and the import
    // resolves to undefined. Followup to #957 / PR #959.
    GlobalThisExpr,
    // Process uptime: process.uptime() -> number (seconds)
    ProcessUptime,
    // Process current working directory: process.cwd() -> string
    ProcessCwd,
    // Process command line arguments: process.argv -> string[]
    ProcessArgv,
    // Process memory usage: process.memoryUsage() -> object { rss, heapTotal, heapUsed, external, arrayBuffers }
    ProcessMemoryUsage,
    // Process PID: process.pid -> number
    ProcessPid,
    // Process parent PID: process.ppid -> number
    ProcessPpid,
    // Process Node version string: process.version -> string (e.g. "v22.0.0")
    ProcessVersion,
    // Process versions object: process.versions -> { node, v8, ... }
    ProcessVersions,
    // process.hrtime.bigint() -> bigint (nanoseconds since arbitrary point)
    ProcessHrtimeBigint,
    // process.nextTick(callback) -> void
    ProcessNextTick(Box<Expr>),
    // process.on(event, handler) -> void (registers an event listener)
    ProcessOn {
        event: Box<Expr>,
        handler: Box<Expr>,
    },
    // process.chdir(directory) -> void
    ProcessChdir(Box<Expr>),
    // process.kill(pid, signal?) -> void
    ProcessKill {
        pid: Box<Expr>,
        signal: Option<Box<Expr>>,
    },
    // process.exit(code?) -> never. Bare `process.exit()` lowers as
    // `ProcessExit(None)` which the runtime treats as code 0.
    ProcessExit(Option<Box<Expr>>),
    // process.stdin -> stub object { write: fn }
    ProcessStdin,
    // process.stdout -> stub object { write: fn }
    ProcessStdout,
    // process.stderr -> stub object { write: fn }
    ProcessStderr,
    // process.stdin.setRawMode(enabled) -> stdin (#347 Phase 2)
    ProcessStdinSetRawMode(Box<Expr>),
    // process.stdin.on(event, handler) -> stdin (#347 Phase 2)
    // Supported events: 'data', 'keypress', 'end', 'close'.
    ProcessStdinOn {
        event: Box<Expr>,
        handler: Box<Expr>,
    },
    // process.stdout.on('resize', handler) -> stdout (#347 Phase 3)
    // Registers a SIGWINCH handler that fires when the terminal is
    // resized. Other events fall through to the generic dispatch.
    ProcessStdoutOn {
        event: Box<Expr>,
        handler: Box<Expr>,
    },
    // process.stdin.isTTY / process.stdout.isTTY / process.stderr.isTTY
    // (#347 Phase 3) — boolean property reflecting whether the fd is a
    // terminal. Each evaluates to libc::isatty(fd) on Unix /
    // GetFileType(STD_*_HANDLE) == FILE_TYPE_CHAR on Windows.
    ProcessStdinIsTTY,
    ProcessStdoutIsTTY,
    ProcessStderrIsTTY,
    // process.stdout.columns / .rows (#347 Phase 3) — terminal width
    // and height in cells, evaluated fresh on every read via
    // TIOCGWINSZ on Unix / GetConsoleScreenBufferInfo on Windows.
    // Returns `undefined` when stdout isn't a TTY.
    ProcessStdoutColumns,
    ProcessStdoutRows,
    // tty.isatty(fd) -> boolean (#347 Phase 3)
    TtyIsAtty(Box<Expr>),

    // File system operations
    FsReadFileSync(Box<Expr>), // fs.readFileSync(path) -> string
    FsWriteFileSync(Box<Expr>, Box<Expr>), // fs.writeFileSync(path, content) -> void
    FsExistsSync(Box<Expr>),   // fs.existsSync(path) -> boolean
    FsMkdirSync(Box<Expr>),    // fs.mkdirSync(path) -> void
    FsUnlinkSync(Box<Expr>),   // fs.unlinkSync(path) -> void
    FsAppendFileSync(Box<Expr>, Box<Expr>), // fs.appendFileSync(path, content) -> void
    FsReadFileBinary(Box<Expr>), // fs.readFileBuffer(path) -> Buffer (binary-safe)
    FsRmRecursive(Box<Expr>),  // fs.rmRecursive(path) -> boolean

    // Path operations
    PathJoin(Box<Expr>, Box<Expr>),        // path.join(a, b) -> string
    PathDirname(Box<Expr>),                // path.dirname(path) -> string
    PathBasename(Box<Expr>),               // path.basename(path) -> string
    PathBasenameExt(Box<Expr>, Box<Expr>), // path.basename(path, ext) -> string (strips ext suffix)
    PathExtname(Box<Expr>),                // path.extname(path) -> string
    PathResolve(Box<Expr>),                // path.resolve(path) -> string
    PathIsAbsolute(Box<Expr>),             // path.isAbsolute(path) -> boolean
    PathRelative(Box<Expr>, Box<Expr>),    // path.relative(from, to) -> string
    PathNormalize(Box<Expr>),              // path.normalize(path) -> string
    PathParse(Box<Expr>),                  // path.parse(path) -> { root, dir, base, ext, name }
    PathFormat(Box<Expr>),                 // path.format({ dir, base }) -> string
    PathSep,                               // path.sep constant
    PathDelimiter,                         // path.delimiter constant
    PathToNamespacedPath(Box<Expr>),       // path.toNamespacedPath(path) -> string (POSIX: no-op)
    PathMatchesGlob(Box<Expr>, Box<Expr>), // path.matchesGlob(path, pattern) -> boolean
    PathResolveJoin(Box<Expr>, Box<Expr>), // internal: join with reset-on-absolute (multi-arg resolve)
    PathWin32Join(Box<Expr>, Box<Expr>),   // path.win32.join(a, b) -> string (issue #810)

    // WeakRef and FinalizationRegistry
    WeakRefNew(Box<Expr>),              // new WeakRef(obj) -> WeakRef
    WeakRefDeref(Box<Expr>),            // ref.deref() -> object | undefined
    FinalizationRegistryNew(Box<Expr>), // new FinalizationRegistry(callback) -> registry
    FinalizationRegistryRegister {
        // registry.register(target, held, token?)
        registry: Box<Expr>,
        target: Box<Expr>,
        held: Box<Expr>,
        token: Option<Box<Expr>>,
    },
    FinalizationRegistryUnregister {
        registry: Box<Expr>,
        token: Box<Expr>,
    }, // registry.unregister(token) -> bool

    // Object property descriptor methods
    ObjectDefineProperty(Box<Expr>, Box<Expr>, Box<Expr>), // Object.defineProperty(obj, key, desc)
    ObjectGetOwnPropertyDescriptor(Box<Expr>, Box<Expr>), // Object.getOwnPropertyDescriptor(obj, key)
    ObjectGetOwnPropertyNames(Box<Expr>), // Object.getOwnPropertyNames(obj) -> string[]
    ObjectCreate(Box<Expr>),              // Object.create(proto)
    ObjectFreeze(Box<Expr>),              // Object.freeze(obj)
    ObjectSeal(Box<Expr>),                // Object.seal(obj)
    ObjectPreventExtensions(Box<Expr>),   // Object.preventExtensions(obj)
    ObjectIsFrozen(Box<Expr>),            // Object.isFrozen(obj)
    ObjectIsSealed(Box<Expr>),            // Object.isSealed(obj)
    ObjectIsExtensible(Box<Expr>),        // Object.isExtensible(obj)
    ObjectGetPrototypeOf(Box<Expr>),      // Object.getPrototypeOf(obj)
    ObjectSetPrototypeOf(Box<Expr>, Box<Expr>), // Object.setPrototypeOf(obj, proto) -> obj
    ObjectDefineProperties(Box<Expr>, Box<Expr>), // Object.defineProperties(target, descriptors)
    ObjectGetOwnPropertySymbols(Box<Expr>), // Object.getOwnPropertySymbols(obj) -> symbol[]

    // Symbol operations
    SymbolNew(Option<Box<Expr>>), // Symbol() / Symbol(description)
    SymbolFor(Box<Expr>),         // Symbol.for(key) -> registered symbol
    SymbolKeyFor(Box<Expr>),      // Symbol.keyFor(sym) -> key | undefined
    SymbolDescription(Box<Expr>), // sym.description
    SymbolToString(Box<Expr>),    // sym.toString()

    // URL operations
    FileURLToPath(Box<Expr>), // url.fileURLToPath(url) -> string

    // RegExp operations
    RegExpExec {
        regex: Box<Expr>,
        string: Box<Expr>,
    },
    RegExpSource(Box<Expr>),
    RegExpFlags(Box<Expr>),
    RegExpLastIndex(Box<Expr>),
    RegExpSetLastIndex {
        regex: Box<Expr>,
        value: Box<Expr>,
    },
    RegExpReplaceFn {
        string: Box<Expr>,
        regex: Box<Expr>,
        callback: Box<Expr>,
    },
    RegExpExecIndex,
    RegExpExecGroups,

    // JSON operations
    JsonParse(Box<Expr>), // JSON.parse(string) -> value
    /// `JSON.parse<T>(string)` with a compile-time type argument
    /// (issue #179 tier 1 via typed-parse plan). The `ty` carries the
    /// expected shape so codegen can emit a specialized parse call.
    /// `ordered_keys`, when present, is the field list in SOURCE order
    /// (as declared in the TypeScript interface/type literal) —
    /// preserved from the AST because `ObjectType::properties` is a
    /// HashMap that loses insertion order. Codegen uses this to emit
    /// the shape hint in an order that matches how JSON.stringify
    /// output typically lays out fields (declaration order), so the
    /// per-field fast path in `parse_object_shaped` actually hits.
    /// Semantically identical to `JsonParse` (the `<T>` is fully
    /// erased at runtime — Node-compatible); Perry may opt into a
    /// faster specialized path per shape. Falls back to the generic
    /// parser transparently if the input doesn't match the declared
    /// shape.
    JsonParseTyped {
        text: Box<Expr>,
        ty: Type,
        ordered_keys: Option<Vec<String>>,
    },
    JsonParseReviver {
        text: Box<Expr>,
        reviver: Box<Expr>,
    },
    JsonParseWithReviver(Box<Expr>, Box<Expr>),
    JsonStringify(Box<Expr>), // JSON.stringify(value) -> string
    JsonStringifyPretty {
        value: Box<Expr>,
        replacer: Option<Box<Expr>>,
        space: Box<Expr>,
    },
    JsonStringifyFull(Box<Expr>, Box<Expr>, Box<Expr>),

    // Math operations
    MathFloor(Box<Expr>),            // Math.floor(x) -> number
    MathCeil(Box<Expr>),             // Math.ceil(x) -> number
    MathRound(Box<Expr>),            // Math.round(x) -> number
    MathAbs(Box<Expr>),              // Math.abs(x) -> number
    MathSqrt(Box<Expr>),             // Math.sqrt(x) -> number
    MathLog(Box<Expr>),              // Math.log(x) -> number
    MathLog2(Box<Expr>),             // Math.log2(x) -> number
    MathLog10(Box<Expr>),            // Math.log10(x) -> number
    MathPow(Box<Expr>, Box<Expr>),   // Math.pow(base, exp) -> number
    MathMin(Vec<Expr>),              // Math.min(...values) -> number
    MathMax(Vec<Expr>),              // Math.max(...values) -> number
    MathMinSpread(Box<Expr>),        // Math.min(...array) -> number (spread from single array)
    MathMaxSpread(Box<Expr>),        // Math.max(...array) -> number (spread from single array)
    MathImul(Box<Expr>, Box<Expr>),  // Math.imul(a, b) -> number (32-bit integer multiply)
    MathRandom,                      // Math.random() -> number
    MathSin(Box<Expr>),              // Math.sin(x) -> number
    MathCos(Box<Expr>),              // Math.cos(x) -> number
    MathTan(Box<Expr>),              // Math.tan(x) -> number
    MathAsin(Box<Expr>),             // Math.asin(x) -> number
    MathAcos(Box<Expr>),             // Math.acos(x) -> number
    MathAtan(Box<Expr>),             // Math.atan(x) -> number
    MathAtan2(Box<Expr>, Box<Expr>), // Math.atan2(y, x) -> number
    MathCbrt(Box<Expr>),             // Math.cbrt(x) -> number
    MathHypot(Vec<Expr>),            // Math.hypot(...values) -> number
    MathFround(Box<Expr>),           // Math.fround(x) -> number
    MathClz32(Box<Expr>),            // Math.clz32(x) -> number
    MathExpm1(Box<Expr>),            // Math.expm1(x) -> number
    MathLog1p(Box<Expr>),            // Math.log1p(x) -> number
    MathSinh(Box<Expr>),             // Math.sinh(x) -> number
    MathCosh(Box<Expr>),             // Math.cosh(x) -> number
    MathTanh(Box<Expr>),             // Math.tanh(x) -> number
    MathAsinh(Box<Expr>),            // Math.asinh(x) -> number
    MathAcosh(Box<Expr>),            // Math.acosh(x) -> number
    MathAtanh(Box<Expr>),            // Math.atanh(x) -> number
    MathExp(Box<Expr>),              // Math.exp(x) -> number (e^x)

    /// performance.now() -> number (high-resolution time in ms)
    PerformanceNow,

    // WebAssembly host (issue #76). MVP surface — see
    // `crates/perry-runtime/src/webassembly.rs` for the FFI shape.
    /// `WebAssembly.validate(bytes)` -> boolean
    WebAssemblyValidate(Box<Expr>),
    /// `WebAssembly.instantiate(bytes)` -> opaque instance handle (Perry
    /// MVP shape — sync, no Promise, no `{module, instance}` pair).
    WebAssemblyInstantiate(Box<Expr>),
    /// `WebAssembly.callExport(instance, name, ...args)` — Perry-specific
    /// helper for invoking numeric exports (see issue #76 PoC scope).
    WebAssemblyCallExport {
        instance: Box<Expr>,
        name: Box<Expr>,
        args: Vec<Expr>,
    },
    /// atob(base64) -> string
    Atob(Box<Expr>),
    /// btoa(string) -> string
    Btoa(Box<Expr>),

    // TextEncoder / TextDecoder
    /// new TextEncoder() -> opaque handle (stateless, always utf-8)
    TextEncoderNew,
    /// encoder.encode(string) -> Buffer (Uint8Array of UTF-8 bytes)
    TextEncoderEncode(Box<Expr>),
    /// new TextDecoder() or new TextDecoder("utf-8") -> opaque handle
    TextDecoderNew,
    /// decoder.decode(buffer) -> string (UTF-8 decode)
    TextDecoderDecode(Box<Expr>),

    // URI encoding / decoding
    /// encodeURI(string) -> string
    EncodeURI(Box<Expr>),
    /// decodeURI(string) -> string
    DecodeURI(Box<Expr>),
    /// encodeURIComponent(string) -> string
    EncodeURIComponent(Box<Expr>),
    /// decodeURIComponent(string) -> string
    DecodeURIComponent(Box<Expr>),

    /// structuredClone(value) -> deep-cloned value
    StructuredClone(Box<Expr>),
    /// queueMicrotask(callback) -> void
    QueueMicrotask(Box<Expr>),

    /// Async-step iter-result scratch helpers — used by the
    /// async-to-generator transform's state machine and step driver.
    /// Eliminate the per-await `{value, done}` heap alloc on the
    /// async hot path. `IterResultSet(value, done)` writes to a
    /// thread-local pair and returns `undefined`; the matching
    /// `IterResultGetValue` / `IterResultGetDone` read them back.
    /// Only emitted by the generator transform for `was_plain_async`
    /// functions — never by user code.
    IterResultSet(Box<Expr>, bool),
    IterResultGetValue,
    IterResultGetDone,

    /// Optimized async-step chain: equivalent to
    /// `Promise.resolve(value).then(v => step(v, false), e => step(e, true))`
    /// but skips the two arrow-wrapper closure allocations + dispatches
    /// by carrying the step closure directly through the task queue.
    /// Only emitted by `build_async_step_driver_direct` — never by user
    /// code.
    AsyncStepChain {
        value: Box<Expr>,
        step_closure: Box<Expr>,
    },

    /// Optimized done-case for the async-step driver: equivalent to
    /// `Promise.resolve(value)` at the position where the state machine
    /// terminates. Saves the per-call fulfilled-Promise allocation when
    /// step is invoked from inside the microtask runner — the runner has
    /// stashed the in-flight `next` Promise in `INLINE_TRAP_NEXT` and
    /// step's return is checked against `next` for a self-chain skip.
    /// When INLINE_TRAP_NEXT is null (initial entry / no-await async
    /// function), the helper falls back to a fresh `js_promise_resolved`.
    /// Only emitted by `build_async_step_driver_direct`.
    AsyncStepDone {
        value: Box<Expr>,
        step_closure: Box<Expr>,
    },

    /// #691 Phase 2. Returns the currently-running step closure as a
    /// NaN-boxed pointer (read from `INLINE_TRAP.current_step` TLS).
    /// Used by `build_async_step_driver_direct` to replace the
    /// `step_id` self-capture inside the step body — eliminates the
    /// per-invocation `js_box_alloc` for the self-reference and
    /// shrinks the step closure by one capture slot. Codegen also
    /// recognizes it as a callee in `Expr::Call` so the catch arm's
    /// `__step(e, true)` recursive re-entry works without the
    /// captured local.
    /// Only emitted by `build_async_step_driver_direct` — never by
    /// user code.
    CurrentStepClosure,

    /// #691 Phase 2. Invokes a freshly-built step closure with
    /// (undefined, false) and the proper `CURRENT_STEP_CLOSURE` TLS
    /// setup. Used at the bottom of the async-step wrapper in place
    /// of a direct `__step(undefined, false)` call so that
    /// `Expr::CurrentStepClosure` inside the body returns the right
    /// pointer on the very first state transition. The runtime
    /// helper saves and restores the previous trap state so nested
    /// async calls compose.
    /// Only emitted by `build_async_step_driver_direct`.
    AsyncFirstCall {
        step_closure: Box<Expr>,
    },

    // Crypto operations
    CryptoRandomBytes(Box<Expr>), // crypto.randomBytes(size) -> string (hex)
    CryptoRandomUUID,             // crypto.randomUUID() -> string
    CryptoSha256(Box<Expr>),      // crypto.sha256(data) -> string (hex)
    CryptoMd5(Box<Expr>),         // crypto.md5(data) -> string (hex)

    // Web Crypto API (issue #561). The async wrapping is decorative —
    // the SHA / HMAC primitives are CPU-bound and resolve synchronously
    // inside the returned Promise. CryptoKey is implemented as a Buffer
    // marked Uint8Array, with `(buf_addr → algo, hash)` recorded in the
    // perry-stdlib WebCrypto registry at importKey time.
    /// `crypto.subtle.digest(alg, data)` -> Promise<ArrayBuffer>
    WebCryptoDigest {
        algo: Box<Expr>,
        data: Box<Expr>,
    },
    /// `crypto.subtle.importKey(format, key, algorithm, extractable, usages)` -> Promise<CryptoKey>
    WebCryptoImportKey {
        format: Box<Expr>,
        key: Box<Expr>,
        algorithm: Box<Expr>,
        extractable: Box<Expr>,
        usages: Box<Expr>,
    },
    /// `crypto.subtle.sign(algorithm, key, data)` -> Promise<ArrayBuffer>
    WebCryptoSign {
        algorithm: Box<Expr>,
        key: Box<Expr>,
        data: Box<Expr>,
    },
    /// `crypto.subtle.verify(algorithm, key, signature, data)` -> Promise<boolean>
    WebCryptoVerify {
        algorithm: Box<Expr>,
        key: Box<Expr>,
        signature: Box<Expr>,
        data: Box<Expr>,
    },
    /// `crypto.subtle.encrypt(algorithm, key, data)` -> Promise<ArrayBuffer>
    ///
    /// Initial implementation covers AES-GCM (the surface jose's
    /// `gcmEncrypt` / `rsaes` reach for); AES-CBC, AES-CTR, and
    /// RSA-OAEP are TODO follow-ups tracked alongside #561.
    WebCryptoEncrypt {
        algorithm: Box<Expr>,
        key: Box<Expr>,
        data: Box<Expr>,
    },
    /// `crypto.subtle.decrypt(algorithm, key, data)` -> Promise<ArrayBuffer>
    WebCryptoDecrypt {
        algorithm: Box<Expr>,
        key: Box<Expr>,
        data: Box<Expr>,
    },
    /// `crypto.subtle.generateKey(algorithm, extractable, keyUsages)` ->
    /// Promise<CryptoKey>. Initial implementation covers symmetric
    /// AES-GCM (the shape jose's `generateSecret('A256GCM')` reaches
    /// for); asymmetric and other algorithms are TODO follow-ups
    /// tracked alongside #561.
    WebCryptoGenerateKey {
        algorithm: Box<Expr>,
        extractable: Box<Expr>,
        usages: Box<Expr>,
    },
    /// `crypto.subtle.wrapKey(format, key, wrappingKey, wrapAlgorithm)`
    /// → Promise<Uint8Array>. Initial implementation covers AES-KW
    /// + AES-GCM wrap (the shape jose's `wrapKey` reaches for);
    /// asymmetric (RSA-OAEP) wrap is a TODO follow-up tracked
    /// alongside #561.
    WebCryptoWrapKey {
        format: Box<Expr>,
        key: Box<Expr>,
        wrapping_key: Box<Expr>,
        wrap_algorithm: Box<Expr>,
    },
    /// `crypto.subtle.unwrapKey(format, wrappedKey, unwrappingKey,
    /// unwrapAlgorithm, unwrappedKeyAlgorithm, extractable, usages)`
    /// → Promise<CryptoKey>. Mirrors `wrapKey`'s algorithm coverage;
    /// the resulting CryptoKey is registered with the
    /// `unwrappedKeyAlgorithm` so subsequent encrypt/decrypt calls
    /// resolve the right primitive.
    WebCryptoUnwrapKey {
        format: Box<Expr>,
        wrapped_key: Box<Expr>,
        unwrapping_key: Box<Expr>,
        unwrap_algorithm: Box<Expr>,
        unwrapped_key_algorithm: Box<Expr>,
        extractable: Box<Expr>,
        usages: Box<Expr>,
    },
    /// `crypto.randomFillSync(buffer, offset?, size?)` — fills the
    /// provided Buffer/TypedArray with random bytes in-place and
    /// returns the same buffer. `offset` and `size` are optional
    /// JS values (undefined sentinels OK).
    CryptoRandomFillSync {
        buffer: Box<Expr>,
        offset: Box<Expr>,
        size: Box<Expr>,
    },

    // OS operations
    OsPlatform,          // os.platform() -> string ("darwin", "linux", "win32")
    OsArch,              // os.arch() -> string ("x64", "arm64", etc.)
    OsHostname,          // os.hostname() -> string
    OsHomedir,           // os.homedir() -> string
    OsTmpdir,            // os.tmpdir() -> string
    OsTotalmem,          // os.totalmem() -> number (bytes)
    OsFreemem,           // os.freemem() -> number (bytes)
    OsUptime,            // os.uptime() -> number (seconds)
    OsType,              // os.type() -> string ("Darwin", "Linux", "Windows_NT")
    OsRelease,           // os.release() -> string
    OsCpus,              // os.cpus() -> array of CPU info objects
    OsNetworkInterfaces, // os.networkInterfaces() -> object
    OsUserInfo,          // os.userInfo() -> object
    OsEOL,               // os.EOL -> string ("\n" or "\r\n")

    // Buffer operations
    BufferFrom {
        // Buffer.from(data, encoding?) -> Buffer
        data: Box<Expr>,
        encoding: Option<Box<Expr>>,
    },
    BufferAlloc {
        // Buffer.alloc(size, fill?) -> Buffer
        size: Box<Expr>,
        fill: Option<Box<Expr>>,
    },
    BufferAllocUnsafe(Box<Expr>), // Buffer.allocUnsafe(size) -> Buffer
    BufferConcat(Box<Expr>),      // Buffer.concat(list) -> Buffer
    BufferIsBuffer(Box<Expr>),    // Buffer.isBuffer(obj) -> boolean
    BufferByteLength(Box<Expr>),  // Buffer.byteLength(string) -> number
    BufferToString {
        // buffer.toString(encoding?) -> string
        buffer: Box<Expr>,
        encoding: Option<Box<Expr>>,
    },
    BufferLength(Box<Expr>), // buffer.length -> number
    BufferSlice {
        // buffer.slice(start?, end?) -> Buffer
        buffer: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
    },
    BufferCopy {
        // buffer.copy(target, tStart?, sStart?, sEnd?) -> number
        source: Box<Expr>,
        target: Box<Expr>,
        target_start: Option<Box<Expr>>,
        source_start: Option<Box<Expr>>,
        source_end: Option<Box<Expr>>,
    },
    BufferWrite {
        // buffer.write(string, offset?, encoding?) -> number
        buffer: Box<Expr>,
        string: Box<Expr>,
        offset: Option<Box<Expr>>,
        encoding: Option<Box<Expr>>,
    },
    BufferFill {
        // buffer.fill(value) -> Buffer (same buffer)
        buffer: Box<Expr>,
        value: Box<Expr>,
    },
    BufferEquals {
        // buffer.equals(other) -> boolean
        buffer: Box<Expr>,
        other: Box<Expr>,
    },
    BufferIndexGet {
        // buffer[i] -> number
        buffer: Box<Expr>,
        index: Box<Expr>,
    },
    BufferIndexSet {
        // buffer[i] = value
        buffer: Box<Expr>,
        index: Box<Expr>,
        value: Box<Expr>,
    },

    // Typed array operations
    Uint8ArrayNew(Option<Box<Expr>>), // new Uint8Array() or new Uint8Array(length) or new Uint8Array(array)
    Uint8ArrayFrom(Box<Expr>),        // Uint8Array.from(arrayLike) -> Uint8Array
    Uint8ArrayLength(Box<Expr>),      // uint8array.length -> number
    Uint8ArrayGet {
        // uint8array[i] -> number
        array: Box<Expr>,
        index: Box<Expr>,
    },
    Uint8ArraySet {
        // uint8array[i] = value
        array: Box<Expr>,
        index: Box<Expr>,
        value: Box<Expr>,
    },

    /// Generic typed array constructor: `new Int32Array([1, 2, 3])` etc.
    /// `kind` is one of the `TYPED_ARRAY_KIND_*` constants.
    /// `arg` is `None` for `new Int32Array()`, `Some(expr)` for `(length)` or `(arrayLike)`.
    TypedArrayNew {
        kind: u8,
        arg: Option<Box<Expr>>,
    },

    // Child Process operations
    ChildProcessExecSync {
        // execSync(cmd, opts?) -> Buffer | string
        command: Box<Expr>,
        options: Option<Box<Expr>>,
    },
    ChildProcessSpawnSync {
        // spawnSync(cmd, args?, opts?) -> SpawnSyncResult
        command: Box<Expr>,
        args: Option<Box<Expr>>,
        options: Option<Box<Expr>>,
    },
    ChildProcessSpawn {
        // spawn(cmd, args?, opts?) -> ChildProcess
        command: Box<Expr>,
        args: Option<Box<Expr>>,
        options: Option<Box<Expr>>,
    },
    ChildProcessExec {
        // exec(cmd, opts?, callback?) -> ChildProcess
        command: Box<Expr>,
        options: Option<Box<Expr>>,
        callback: Option<Box<Expr>>,
    },
    ChildProcessSpawnBackground {
        // child_process.spawnBackground(cmd, args, logFile, envJson?) -> {pid, handleId}
        command: Box<Expr>,
        args: Option<Box<Expr>>,
        log_file: Box<Expr>,
        env_json: Option<Box<Expr>>,
    },
    ChildProcessGetProcessStatus(Box<Expr>), // child_process.getProcessStatus(handleId) -> {alive, exitCode}
    ChildProcessKillProcess(Box<Expr>),      // child_process.killProcess(handleId) -> void

    // Fetch operations
    FetchWithOptions {
        // fetch(url, {method, body, headers}) -> Promise<Response>
        url: Box<Expr>,
        method: Box<Expr>,
        body: Box<Expr>,
        headers: Vec<(String, Expr)>,
    },
    FetchGetWithAuth {
        // fetchWithAuth(url, authHeader) -> Promise<Response>
        url: Box<Expr>,
        auth_header: Box<Expr>,
    },
    FetchPostWithAuth {
        // fetchPostWithAuth(url, authHeader, body) -> Promise<Response>
        url: Box<Expr>,
        auth_header: Box<Expr>,
        body: Box<Expr>,
    },

    // Net operations
    NetCreateServer {
        // net.createServer(options?, connectionListener?) -> Server
        options: Option<Box<Expr>>,
        connection_listener: Option<Box<Expr>>,
    },
    NetCreateConnection {
        // net.createConnection(port, host?, connectListener?) -> Socket
        port: Box<Expr>,
        host: Option<Box<Expr>>,
        connect_listener: Option<Box<Expr>>,
    },
    NetConnect {
        // net.connect(port, host?, connectListener?) -> Socket
        port: Box<Expr>,
        host: Option<Box<Expr>>,
        connect_listener: Option<Box<Expr>>,
    },

    // Array methods
    ArrayPush {
        array_id: LocalId,
        value: Box<Expr>,
    }, // arr.push(value) -> new length
    ArrayPushSpread {
        array_id: LocalId,
        source: Box<Expr>,
    }, // arr.push(...src) -> new length
    ArrayPop(LocalId),   // arr.pop() -> removed element
    ArrayShift(LocalId), // arr.shift() -> removed element
    ArrayUnshift {
        array_id: LocalId,
        value: Box<Expr>,
    }, // arr.unshift(value) -> new length
    ArrayIndexOf {
        array: Box<Expr>,
        value: Box<Expr>,
    }, // arr.indexOf(value) -> index
    ArrayIncludes {
        array: Box<Expr>,
        value: Box<Expr>,
    }, // arr.includes(value) -> boolean
    ArraySlice {
        array: Box<Expr>,
        start: Box<Expr>,
        end: Option<Box<Expr>>,
    }, // arr.slice(start, end?) -> new array
    ArraySplice {
        array_id: LocalId,
        start: Box<Expr>,
        delete_count: Option<Box<Expr>>,
        items: Vec<Expr>,
    }, // arr.splice(start, deleteCount?, ...items) -> deleted elements array

    // Array higher-order function methods
    ArrayForEach {
        array: Box<Expr>,
        callback: Box<Expr>,
    }, // arr.forEach(fn) -> void
    ArrayMap {
        array: Box<Expr>,
        callback: Box<Expr>,
    }, // arr.map(fn) -> new array
    ArrayFilter {
        array: Box<Expr>,
        callback: Box<Expr>,
    }, // arr.filter(fn) -> new array
    ArrayFind {
        array: Box<Expr>,
        callback: Box<Expr>,
    }, // arr.find(fn) -> element | undefined
    ArrayFindIndex {
        array: Box<Expr>,
        callback: Box<Expr>,
    }, // arr.findIndex(fn) -> index | -1
    ArrayFindLast {
        array: Box<Expr>,
        callback: Box<Expr>,
    }, // arr.findLast(fn) -> element | undefined
    ArrayFindLastIndex {
        array: Box<Expr>,
        callback: Box<Expr>,
    }, // arr.findLastIndex(fn) -> index | -1
    ArrayAt {
        array: Box<Expr>,
        index: Box<Expr>,
    }, // arr.at(i) -> element (negative index OK)
    ArraySome {
        array: Box<Expr>,
        callback: Box<Expr>,
    }, // arr.some(fn) -> boolean
    ArrayEvery {
        array: Box<Expr>,
        callback: Box<Expr>,
    }, // arr.every(fn) -> boolean
    ArrayFlatMap {
        array: Box<Expr>,
        callback: Box<Expr>,
    }, // arr.flatMap(fn) -> new array
    ArraySort {
        array: Box<Expr>,
        comparator: Box<Expr>,
    }, // arr.sort(fn) -> same array (in-place)
    ArrayReduce {
        array: Box<Expr>,
        callback: Box<Expr>,
        initial: Option<Box<Expr>>,
    }, // arr.reduce(fn, init?) -> value
    ArrayReduceRight {
        array: Box<Expr>,
        callback: Box<Expr>,
        initial: Option<Box<Expr>>,
    }, // arr.reduceRight(fn, init?) -> value
    ArrayJoin {
        array: Box<Expr>,
        separator: Option<Box<Expr>>,
    }, // arr.join(separator?) -> string
    ArrayFlat {
        array: Box<Expr>,
    }, // arr.flat() -> flattened array
    ArrayToReversed {
        array: Box<Expr>,
    }, // arr.toReversed() -> new reversed array
    ArrayToSorted {
        array: Box<Expr>,
        comparator: Option<Box<Expr>>,
    }, // arr.toSorted(fn?) -> new sorted array
    ArrayToSpliced {
        array: Box<Expr>,
        start: Box<Expr>,
        delete_count: Box<Expr>,
        items: Vec<Expr>,
    }, // arr.toSpliced(start, deleteCount, ...items) -> new array
    ArrayWith {
        array: Box<Expr>,
        index: Box<Expr>,
        value: Box<Expr>,
    }, // arr.with(index, value) -> new array
    ArrayCopyWithin {
        array_id: LocalId,
        target: Box<Expr>,
        start: Box<Expr>,
        end: Option<Box<Expr>>,
    }, // arr.copyWithin(target, start, end?) -> same array
    ArrayEntries(Box<Expr>), // arr.entries() -> Array<[index, value]> (eager materialization)
    ArrayKeys(Box<Expr>),    // arr.keys() -> Array<index>
    ArrayValues(Box<Expr>),  // arr.values() -> Array<value> (essentially clone)

    // String methods
    StringSplit(Box<Expr>, Box<Expr>), // string.split(delimiter) -> string[]
    StringFromCharCode(Box<Expr>),     // String.fromCharCode(code) -> single-char string
    StringFromCodePoint(Box<Expr>),    // String.fromCodePoint(code) -> string
    StringAt {
        string: Box<Expr>,
        index: Box<Expr>,
    }, // str.at(i) -> string | undefined (negative supported)
    StringCodePointAt {
        string: Box<Expr>,
        index: Box<Expr>,
    }, // str.codePointAt(i) -> number | undefined

    // Map operations
    MapNew,                     // new Map() -> empty map
    MapNewFromArray(Box<Expr>), // new Map([[k,v], ...]) -> map from entries
    MapSet {
        map: Box<Expr>,
        key: Box<Expr>,
        value: Box<Expr>,
    }, // map.set(key, value) -> map
    MapGet {
        map: Box<Expr>,
        key: Box<Expr>,
    }, // map.get(key) -> value | undefined
    MapHas {
        map: Box<Expr>,
        key: Box<Expr>,
    }, // map.has(key) -> boolean
    MapDelete {
        map: Box<Expr>,
        key: Box<Expr>,
    }, // map.delete(key) -> boolean
    MapSize(Box<Expr>),         // map.size -> number
    MapClear(Box<Expr>),        // map.clear() -> void
    MapEntries(Box<Expr>),      // map.entries() -> Array<[key, value]>
    MapKeys(Box<Expr>),         // map.keys() -> Array<key>
    MapValues(Box<Expr>),       // map.values() -> Array<value>
    /// `js_map_entry_key_at(map, idx)` — read the key at flat entry
    /// index `idx`. Used by the `for (const [k, v] of mapExpr)` fast
    /// path so the loop reads entries directly without allocating a
    /// pair Array per iteration. Caller bounds the loop with `MapSize`.
    MapEntryKeyAt {
        map: Box<Expr>,
        idx: Box<Expr>,
    },
    /// Companion to `MapEntryKeyAt` — read the value at `idx`.
    MapEntryValueAt {
        map: Box<Expr>,
        idx: Box<Expr>,
    },

    // Set operations
    SetNew,                     // new Set() -> empty set
    SetNewFromArray(Box<Expr>), // new Set(array) -> set from iterable
    SetAdd {
        set_id: LocalId,
        value: Box<Expr>,
    }, // set.add(value) -> set (updates local)
    SetHas {
        set: Box<Expr>,
        value: Box<Expr>,
    }, // set.has(value) -> boolean
    SetDelete {
        set: Box<Expr>,
        value: Box<Expr>,
    }, // set.delete(value) -> boolean
    SetSize(Box<Expr>),         // set.size -> number
    SetClear(Box<Expr>),        // set.clear() -> void
    SetValues(Box<Expr>),       // set.values() -> Array (via js_set_to_array)
    /// `js_set_value_at(set, idx)` — read the i-th element in insertion
    /// order. Used by the `for (const x of setExpr)` fast path so the loop
    /// reads elements directly without materializing the buffer into an
    /// Array via `js_set_to_array`. Caller bounds the loop with `SetSize`.
    SetValueAt {
        set: Box<Expr>,
        idx: Box<Expr>,
    },

    // Sequence expression (comma operator)
    Sequence(Vec<Expr>),

    // Date operations
    DateNow,                        // Date.now() -> number (timestamp in ms)
    DateNew(Vec<Expr>), // new Date() / new Date(ts) / new Date(year, month, day, h?, m?, s?, ms?) -> Date object
    DateGetTime(Box<Expr>), // date.getTime() -> number
    DateToISOString(Box<Expr>), // date.toISOString() -> string
    DateGetFullYear(Box<Expr>), // date.getFullYear() -> number
    DateGetMonth(Box<Expr>), // date.getMonth() -> number (0-11)
    DateGetDate(Box<Expr>), // date.getDate() -> number (1-31)
    DateGetDay(Box<Expr>), // date.getDay() -> number (0-6, Sunday=0)
    DateGetHours(Box<Expr>), // date.getHours() -> number (0-23)
    DateGetMinutes(Box<Expr>), // date.getMinutes() -> number (0-59)
    DateGetSeconds(Box<Expr>), // date.getSeconds() -> number (0-59)
    DateGetMilliseconds(Box<Expr>), // date.getMilliseconds() -> number (0-999)

    // Date static methods
    DateParse(Box<Expr>), // Date.parse(isoString) -> number
    DateUtc(Vec<Expr>),   // Date.UTC(year, month, day, h?, m?, s?) -> number

    // Date getters (UTC variants - for Perry these are the same since we store UTC timestamps)
    DateGetUtcDay(Box<Expr>),          // date.getUTCDay() -> number (0-6)
    DateGetUtcFullYear(Box<Expr>),     // date.getUTCFullYear() -> number
    DateGetUtcMonth(Box<Expr>),        // date.getUTCMonth() -> number (0-11)
    DateGetUtcDate(Box<Expr>),         // date.getUTCDate() -> number (1-31)
    DateGetUtcHours(Box<Expr>),        // date.getUTCHours() -> number (0-23)
    DateGetUtcMinutes(Box<Expr>),      // date.getUTCMinutes() -> number (0-59)
    DateGetUtcSeconds(Box<Expr>),      // date.getUTCSeconds() -> number (0-59)
    DateGetUtcMilliseconds(Box<Expr>), // date.getUTCMilliseconds() -> number (0-999)

    // Date setters (UTC variants) — return the new timestamp
    DateSetUtcFullYear {
        date: Box<Expr>,
        value: Box<Expr>,
    },
    DateSetUtcMonth {
        date: Box<Expr>,
        value: Box<Expr>,
    },
    DateSetUtcDate {
        date: Box<Expr>,
        value: Box<Expr>,
    },
    DateSetUtcHours {
        date: Box<Expr>,
        value: Box<Expr>,
    },
    DateSetUtcMinutes {
        date: Box<Expr>,
        value: Box<Expr>,
    },
    DateSetUtcSeconds {
        date: Box<Expr>,
        value: Box<Expr>,
    },
    DateSetUtcMilliseconds {
        date: Box<Expr>,
        value: Box<Expr>,
    },

    // Date misc
    DateValueOf(Box<Expr>),      // date.valueOf() -> number (same as getTime)
    DateToDateString(Box<Expr>), // date.toDateString() -> string
    DateToTimeString(Box<Expr>), // date.toTimeString() -> string
    DateToLocaleDateString(Box<Expr>), // date.toLocaleDateString() -> string
    DateToLocaleTimeString(Box<Expr>), // date.toLocaleTimeString() -> string
    DateToLocaleString(Box<Expr>), // date.toLocaleString() -> string
    DateGetTimezoneOffset(Box<Expr>), // date.getTimezoneOffset() -> number
    DateToJSON(Box<Expr>),       // date.toJSON() -> string

    // Error operations
    ErrorNew(Option<Box<Expr>>), // new Error() or new Error(message) -> Error object
    ErrorMessage(Box<Expr>),     // error.message -> string
    /// new Error(message, { cause })
    ErrorNewWithCause {
        message: Box<Expr>,
        cause: Box<Expr>,
    },
    /// new TypeError(message)
    TypeErrorNew(Box<Expr>),
    /// new RangeError(message)
    RangeErrorNew(Box<Expr>),
    /// new ReferenceError(message)
    ReferenceErrorNew(Box<Expr>),
    /// new SyntaxError(message)
    SyntaxErrorNew(Box<Expr>),
    /// new AggregateError(errors, message)
    AggregateErrorNew {
        errors: Box<Expr>,
        message: Box<Expr>,
    },

    // URL operations
    /// new URL(url) or new URL(url, base) -> URL object (stored as pointer)
    UrlNew {
        url: Box<Expr>,
        base: Option<Box<Expr>>,
    },
    /// url.href -> string (full URL)
    UrlGetHref(Box<Expr>),
    /// url.pathname -> string (path portion)
    UrlGetPathname(Box<Expr>),
    /// url.protocol -> string (e.g., "https:")
    UrlGetProtocol(Box<Expr>),
    /// url.host -> string (hostname:port)
    UrlGetHost(Box<Expr>),
    /// url.hostname -> string (hostname without port)
    UrlGetHostname(Box<Expr>),
    /// url.port -> string (port number as string)
    UrlGetPort(Box<Expr>),
    /// url.search -> string (query string including ?)
    UrlGetSearch(Box<Expr>),
    /// url.hash -> string (fragment including #)
    UrlGetHash(Box<Expr>),
    /// url.origin -> string (protocol + host)
    UrlGetOrigin(Box<Expr>),
    /// url.searchParams -> URLSearchParams object
    UrlGetSearchParams(Box<Expr>),
    /// URL.canParse(input) -> boolean. Issue #650: spec'd static method
    /// added in Node 18. Returns true if `input` parses as a valid URL.
    UrlCanParse(Box<Expr>),
    /// URL.parse(input) -> URL | null. Issue #650: non-throwing variant
    /// of `new URL()` added in Node 22. Returns null when parsing fails.
    UrlParse(Box<Expr>),
    /// `urlInstance.toString()` -> string. Issue #650: WHATWG `URL.prototype.toString`
    /// is `URL.prototype.toJSON` is alias for `href`. Without this variant the
    /// call fell through to the generic Object.prototype.toString and returned
    /// `[object Object]`.
    UrlInstanceToString(Box<Expr>),
    /// `urlInstance.toJSON()` -> string. Issue #650: returns the same value as
    /// `href`; this is what `JSON.stringify(url)` uses to serialize a URL.
    UrlInstanceToJSON(Box<Expr>),
    /// `urlInstance.pathname = value`. Issue #650: setter mutates the URL's
    /// pathname field and re-derives href so subsequent reads see the new
    /// composed URL string.
    UrlSetPathname {
        url: Box<Expr>,
        value: Box<Expr>,
    },
    /// `urlInstance.search = value`. Issue #650: setter normalizes leading
    /// `?` and re-parses the query string into the URL's searchParams.
    UrlSetSearch {
        url: Box<Expr>,
        value: Box<Expr>,
    },
    /// `urlInstance.hash = value`. Issue #650: setter normalizes leading `#`.
    UrlSetHash {
        url: Box<Expr>,
        value: Box<Expr>,
    },

    // URLSearchParams operations
    /// new URLSearchParams(init?)
    UrlSearchParamsNew(Option<Box<Expr>>),
    /// params.get(name) -> string | null
    UrlSearchParamsGet {
        params: Box<Expr>,
        name: Box<Expr>,
    },
    /// params.has(name) -> boolean
    UrlSearchParamsHas {
        params: Box<Expr>,
        name: Box<Expr>,
    },
    /// params.set(name, value)
    UrlSearchParamsSet {
        params: Box<Expr>,
        name: Box<Expr>,
        value: Box<Expr>,
    },
    /// params.append(name, value)
    UrlSearchParamsAppend {
        params: Box<Expr>,
        name: Box<Expr>,
        value: Box<Expr>,
    },
    /// params.delete(name)
    UrlSearchParamsDelete {
        params: Box<Expr>,
        name: Box<Expr>,
    },
    /// params.toString() -> string
    UrlSearchParamsToString(Box<Expr>),
    /// params.getAll(name) -> string[]
    UrlSearchParamsGetAll {
        params: Box<Expr>,
        name: Box<Expr>,
    },
    /// params.entries() / iteration source for `for (const [k, v] of params)` —
    /// returns an array of `[key, value]` pair arrays. The receiver itself is
    /// an iterable per spec; the for-of lowering wraps the receiver in this
    /// node so the standard array-iter path handles the rest. Refs #575.
    UrlSearchParamsEntries(Box<Expr>),

    // Delete operator
    Delete(Box<Expr>), // delete obj.prop or delete obj["prop"] -> bool

    // Closure (inline function/arrow function)
    Closure {
        /// Unique ID for this closure's underlying function
        func_id: FuncId,
        /// Parameter definitions
        params: Vec<Param>,
        /// Return type
        return_type: Type,
        /// Function body
        body: Vec<Stmt>,
        /// Variables captured from enclosing scope
        captures: Vec<LocalId>,
        /// Captured variables that are modified (need boxing)
        mutable_captures: Vec<LocalId>,
        /// Whether this closure captures `this` from the enclosing scope (arrow function semantics)
        captures_this: bool,
        /// The enclosing class name if this closure captures `this` (for field access during codegen)
        enclosing_class: Option<String>,
        /// Whether this is an async closure
        is_async: bool,
    },

    // RegExp operations
    /// RegExp literal: /pattern/flags
    RegExp {
        pattern: String,
        flags: String,
    },
    /// Dynamic RegExp construction: `RegExp(pattern)` /
    /// `RegExp(pattern, flags)` / `new RegExp(pattern, flags?)` where the
    /// pattern (and optional flags) are runtime values, not string
    /// literals. lodash 4 builds half a dozen of these at module init
    /// from `someLiteralRegex.source`:
    ///   var reHasEscapedHtml = RegExp(reEscapedHtml.source);
    /// Pre-fix the bare `RegExp` ident lowered to `Expr::GlobalGet(0)`
    /// and the function-call form dispatched to a null closure, which
    /// `js_closure_call1` rejected as
    /// `TypeError: value is not a function` at module init. The
    /// `new RegExp(<non-literal>)` arm in `expr_new.rs` similarly fell
    /// through to the generic class-instantiation placeholder. Both now
    /// fold to this variant which lowers to `js_regexp_new(pattern,
    /// flags)` (the same runtime entrypoint the static `/foo/` arm
    /// uses). Followup to #957 / PR #959.
    RegExpDynamic {
        pattern: Box<Expr>,
        flags: Option<Box<Expr>>,
    },
    /// regex.test(string) -> boolean
    RegExpTest {
        regex: Box<Expr>,
        string: Box<Expr>,
    },
    /// string.match(regex) -> string[] | null
    StringMatch {
        string: Box<Expr>,
        regex: Box<Expr>,
    },
    /// string.matchAll(regex) -> Array<Array<string>>
    StringMatchAll {
        string: Box<Expr>,
        regex: Box<Expr>,
    },
    /// string.replace(regex, replacement) -> string
    StringReplace {
        string: Box<Expr>,
        pattern: Box<Expr>,
        replacement: Box<Expr>,
    },

    // Object operations
    /// Object.fromEntries(entries) -> object
    ObjectFromEntries(Box<Expr>),
    /// Object.is(a, b) -> boolean (SameValue algorithm)
    ObjectIs(Box<Expr>, Box<Expr>),
    /// Object.hasOwn(obj, key) -> boolean
    ObjectHasOwn(Box<Expr>, Box<Expr>),

    /// Object.keys(obj) -> string[]
    /// Returns an array of the object's own enumerable property names
    ObjectKeys(Box<Expr>),
    /// Object.values(obj) -> any[]
    /// Returns an array of the object's own enumerable property values
    ObjectValues(Box<Expr>),
    /// Object.entries(obj) -> [string, any][]
    /// Returns an array of the object's own enumerable [key, value] pairs
    ObjectEntries(Box<Expr>),
    /// Object.groupBy(items, keyFn) -> { [key]: items[] }
    /// Walks `items` and groups each element by the string key returned
    /// from `keyFn(item, index)`. Lowered through `js_object_group_by`.
    ObjectGroupBy {
        items: Box<Expr>,
        key_fn: Box<Expr>,
    },
    /// Object rest destructuring: copies all properties except the excluded keys
    /// Used for `const { a, b, ...rest } = obj` → rest = ObjectRest(obj, ["a", "b"])
    ObjectRest {
        object: Box<Expr>,
        exclude_keys: Vec<String>,
    },

    // Array static methods
    /// Array.isArray(value) -> boolean
    /// Returns true if the value is an array
    ArrayIsArray(Box<Expr>),
    /// Array.from(iterable) -> Array
    /// Creates a new array from an iterable (e.g., Map.entries(), Map.keys(), another array)
    ArrayFrom(Box<Expr>),

    /// Tagged-template strings literal — codegen builds the cooked-strings
    /// array AND a parallel raw-strings array, registers the (cooked, raw)
    /// pair via `js_tagged_template_register_raw`, and returns the cooked
    /// pointer (NaN-boxed). The raw entries are always known at compile
    /// time (each quasi's `.raw` text), so they're stored as `String` rather
    /// than `Expr`. Used by `lower_tagged_tpl` for the non-`String.raw`
    /// fast-path tag-function call.
    TaggedTemplateStrings {
        cooked: Vec<Expr>,
        raw: Vec<String>,
    },

    /// `strings.raw` on a tagged-template strings array — looks up the
    /// registered raw-strings array via `js_template_raw`. Returns
    /// undefined for non-tagged-template receivers (matches the JS
    /// semantics `[].raw === undefined`).
    TemplateRaw(Box<Expr>),
    IteratorToArray(Box<Expr>), // collect iterator (.next() loop) into array
    /// Array.from(iterable, mapFn) -> Array
    /// Creates a new array by applying mapFn to each element of the iterable.
    ArrayFromMapped {
        iterable: Box<Expr>,
        map_fn: Box<Expr>,
    },

    // Global built-in functions
    /// parseInt(string, radix?) -> number
    /// Parses a string and returns an integer
    ParseInt {
        string: Box<Expr>,
        radix: Option<Box<Expr>>,
    },
    /// parseFloat(string) -> number
    /// Parses a string and returns a floating-point number
    ParseFloat(Box<Expr>),
    /// Number(value) -> number
    /// Type coercion to number
    NumberCoerce(Box<Expr>),
    /// BigInt(value) -> bigint
    /// Type coercion to bigint
    BigIntCoerce(Box<Expr>),
    /// String(value) -> string
    /// Type coercion to string
    StringCoerce(Box<Expr>),
    /// Boolean(value) -> boolean
    /// Type coercion to boolean via JS truthiness rules
    BooleanCoerce(Box<Expr>),
    /// isNaN(value) -> boolean
    /// Check if value is NaN
    IsNaN(Box<Expr>),
    /// Internal: check if a value is TAG_UNDEFINED or a bare IEEE NaN
    /// (emitted by the lowerer for destructuring defaults). Returns a
    /// NaN-boxed boolean.
    IsUndefinedOrBareNan(Box<Expr>),
    /// isFinite(value) -> boolean
    /// Check if value is finite
    IsFinite(Box<Expr>),
    /// Number.isNaN(value) -> boolean (stricter than isNaN — doesn't coerce)
    NumberIsNaN(Box<Expr>),
    /// Number.isFinite(value) -> boolean (stricter than isFinite — doesn't coerce)
    NumberIsFinite(Box<Expr>),
    /// Number.isInteger(value) -> boolean
    NumberIsInteger(Box<Expr>),
    /// Number.isSafeInteger(value) -> boolean
    NumberIsSafeInteger(Box<Expr>),

    /// perryResolveStaticPlugin(path) -> value
    /// Look up a pre-compiled plugin by source path in the static plugin registry.
    /// Returns the plugin's default export or undefined if not found.
    StaticPluginResolve(Box<Expr>),

    // V8 JavaScript Runtime interop
    // These expressions are used for modules loaded via the V8 interpreter
    /// Load a JavaScript module via V8 runtime
    /// Returns a module handle (u64) for subsequent calls
    JsLoadModule {
        /// Path to the JavaScript module
        path: String,
    },

    /// Get an export from a V8-loaded module
    JsGetExport {
        /// Module handle from JsLoadModule
        module_handle: Box<Expr>,
        /// Name of the export to retrieve
        export_name: String,
    },

    /// Call a function from a V8-loaded module
    JsCallFunction {
        /// Module handle from JsLoadModule
        module_handle: Box<Expr>,
        /// Name of the function to call
        func_name: String,
        /// Arguments to pass to the function
        args: Vec<Expr>,
    },

    /// Call a method on a V8 JavaScript object
    JsCallMethod {
        /// The object to call the method on
        object: Box<Expr>,
        /// Name of the method to call
        method_name: String,
        /// Arguments to pass to the method
        args: Vec<Expr>,
    },

    /// Call a V8 JavaScript function value
    JsCallValue {
        /// JS handle to the function value
        callee: Box<Expr>,
        /// Arguments to pass to the function
        args: Vec<Expr>,
    },

    /// Get a property from a V8 JavaScript object
    JsGetProperty {
        /// The object to get the property from
        object: Box<Expr>,
        /// Name of the property to get
        property_name: String,
    },

    /// Set a property on a V8 JavaScript object
    JsSetProperty {
        /// The object to set the property on
        object: Box<Expr>,
        /// Name of the property to set
        property_name: String,
        /// Value to set
        value: Box<Expr>,
    },

    /// Create a new instance of a V8 JavaScript class
    JsNew {
        /// Module handle from JsLoadModule
        module_handle: Box<Expr>,
        /// Name of the class to instantiate
        class_name: String,
        /// Arguments to pass to the constructor
        args: Vec<Expr>,
    },

    /// Create a new instance from a V8 JS handle to a constructor
    JsNewFromHandle {
        /// JS handle to the constructor function
        constructor: Box<Expr>,
        /// Arguments to pass to the constructor
        args: Vec<Expr>,
    },

    /// Create a V8 function that wraps a native callback
    JsCreateCallback {
        /// The closure expression to wrap
        closure: Box<Expr>,
        /// Number of parameters the callback expects
        param_count: usize,
    },

    /// import.meta.url - returns the URL of the current module
    /// The string is the file:// URL of the source file
    ImportMetaUrl(String),

    // --- Proxy / Reflect (metaprogramming) -----------------------------
    ProxyNew {
        target: Box<Expr>,
        handler: Box<Expr>,
    },
    ProxyGet {
        proxy: Box<Expr>,
        key: Box<Expr>,
    },
    ProxySet {
        proxy: Box<Expr>,
        key: Box<Expr>,
        value: Box<Expr>,
    },
    ProxyHas {
        proxy: Box<Expr>,
        key: Box<Expr>,
    },
    ProxyDelete {
        proxy: Box<Expr>,
        key: Box<Expr>,
    },
    ProxyApply {
        proxy: Box<Expr>,
        args: Vec<Expr>,
    },
    ProxyConstruct {
        proxy: Box<Expr>,
        args: Vec<Expr>,
    },
    ProxyRevocable {
        target: Box<Expr>,
        handler: Box<Expr>,
    },
    ProxyRevoke(Box<Expr>),
    ReflectGet {
        target: Box<Expr>,
        key: Box<Expr>,
    },
    ReflectSet {
        target: Box<Expr>,
        key: Box<Expr>,
        value: Box<Expr>,
    },
    ReflectHas {
        target: Box<Expr>,
        key: Box<Expr>,
    },
    ReflectDelete {
        target: Box<Expr>,
        key: Box<Expr>,
    },
    ReflectOwnKeys(Box<Expr>),
    ReflectApply {
        func: Box<Expr>,
        this_arg: Box<Expr>,
        args: Box<Expr>,
    },
    ReflectConstruct {
        target: Box<Expr>,
        args: Box<Expr>,
    },
    ReflectDefineProperty {
        target: Box<Expr>,
        key: Box<Expr>,
        descriptor: Box<Expr>,
    },
    ReflectGetPrototypeOf(Box<Expr>),
    ReflectDefineMetadata {
        key: Box<Expr>,
        value: Box<Expr>,
        target: Box<Expr>,
        property_key: Option<Box<Expr>>,
    },
    ReflectGetMetadata {
        key: Box<Expr>,
        target: Box<Expr>,
        property_key: Option<Box<Expr>>,
    },
    ReflectGetOwnMetadata {
        key: Box<Expr>,
        target: Box<Expr>,
        property_key: Option<Box<Expr>>,
    },
    ReflectHasMetadata {
        key: Box<Expr>,
        target: Box<Expr>,
        property_key: Option<Box<Expr>>,
    },
    ReflectHasOwnMetadata {
        key: Box<Expr>,
        target: Box<Expr>,
        property_key: Option<Box<Expr>>,
    },
    ReflectGetMetadataKeys {
        target: Box<Expr>,
        property_key: Option<Box<Expr>>,
    },
    ReflectGetOwnMetadataKeys {
        target: Box<Expr>,
        property_key: Option<Box<Expr>>,
    },
    ReflectDeleteMetadata {
        key: Box<Expr>,
        target: Box<Expr>,
        property_key: Option<Box<Expr>>,
    },

    /// Issue #100: dynamic `import()` call whose path argument the
    /// const-folder resolved to a finite set of module sources. Lowered
    /// to dispatch code (single-path or string switch) that returns a
    /// Promise of the target module's namespace object. `paths` is
    /// always non-empty after the resolver pass runs in `collect_modules`
    /// (initial lowering leaves it empty; an Unresolved/over-cap argument
    /// raises a compile error before codegen sees it). `arg` is the
    /// lowered original argument, kept for runtime dispatch on
    /// multi-path sites.
    DynamicImport {
        paths: Vec<String>,
        arg: Box<Expr>,
    },
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    Pos,
}

/// Comparison operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,      // ===
    Ne,      // !==
    LooseEq, // ==
    LooseNe, // !=
    Lt,      // <
    Le,      // <=
    Gt,      // >
    Ge,      // >=
}

/// Logical operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And,      // &&
    Or,       // ||
    Coalesce, // ??
}

/// Update operators (++/--)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOp {
    Increment, // ++
    Decrement, // --
}

/// Element in an array literal with spread support
#[derive(Debug, Clone)]
pub enum ArrayElement {
    /// Regular element: [1, 2, 3]
    Expr(Expr),
    /// Spread element: [...arr]
    Spread(Expr),
}

/// Argument in a function call with spread support
#[derive(Debug, Clone)]
pub enum CallArg {
    /// Regular argument: fn(x, y)
    Expr(Expr),
    /// Spread argument: fn(...arr)
    Spread(Expr),
}

impl Module {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            imports: Vec::new(),
            exports: Vec::new(),
            classes: Vec::new(),
            interfaces: Vec::new(),
            type_aliases: Vec::new(),
            enums: Vec::new(),
            globals: Vec::new(),
            functions: Vec::new(),
            init: Vec::new(),
            exported_native_instances: Vec::new(),
            exported_func_return_native_instances: Vec::new(),
            exported_objects: Vec::new(),
            exported_functions: Vec::new(),
            widgets: Vec::new(),
            uses_fetch: false,
            uses_webassembly: false,
            extern_funcs: Vec::new(),
            init_was_unrolled: false,
            has_top_level_await: false,
            init_kind: ModuleInitKind::Eager,
            async_step_closures: std::collections::HashSet::new(),
        }
    }
}
