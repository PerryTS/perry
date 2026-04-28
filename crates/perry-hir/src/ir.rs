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
pub const NATIVE_MODULES: &[&str] = &[
    "mysql2",
    "mysql2/promise",
    "pg",
    "uuid",
    "bcrypt",
    // ioredis is now in NATIVE_MODULES — the prior workaround (class-name-only
    // tracking in lower.rs:910) was needed when `import { Redis } from 'ioredis'`
    // was expected to fall through to a JS interpreter, but Perry's native Rust
    // ioredis impl is the canonical path and the JS fallback path no longer
    // runs anything. Keeping it out of NATIVE_MODULES forced `requires_stdlib`
    // to return false, which made `Linking (runtime-only)` skip the stdlib
    // archive — every direct `js_ioredis_*` reference (e.g. from the new
    // `lower_builtin_new` "Redis" branch below) link-failed with `Undefined
    // symbols: _js_ioredis_new`. Listing it here lets the linker pull in
    // perry-stdlib (gated on the `database-redis` feature via stdlib_features.rs).
    "ioredis",
    "axios",
    "node-fetch",
    "ws",
    "zlib",
    "crypto",
    // Tier 3
    "dotenv",
    "dotenv/config",  // Side-effect import that auto-calls dotenv.config()
    "jsonwebtoken",
    "nanoid",
    "slugify",
    "validator",
    // ethers utility functions (formatUnits, parseUnits, getAddress, etc.) have native stubs.
    // Contract/Provider are NOT implemented natively — use raw JSON-RPC fetch instead.
    "ethers",
    // Database native libraries
    "mongodb",
    "better-sqlite3",
    // Job scheduler
    "node-cron",
    // Node.js built-ins
    "http",
    "https",
    "events",
    "os",
    "buffer",
    "child_process",
    "net",
    "tls",
    "stream",
    "fs",
    "path",
    "util",
    "url",
    // Utility libraries
    "lru-cache",
    "commander",
    "decimal.js",
    "bignumber.js",
    "exponential-backoff",
    // Lodash utility functions (named import form: import { chunk } from 'lodash')
    "lodash",
    // Date/time libraries
    "dayjs",
    "moment",
    // Image processing
    "sharp",
    // HTML parsing
    "cheerio",
    // Job scheduling (npm 'cron' package; 'node-cron' is a separate alias below)
    "cron",
    // HTTP framework
    "fastify",
    // Node.js built-in modules
    "async_hooks",
    // Perry native UI
    "perry/ui",
    // Perry system APIs
    "perry/system",
    // Perry plugin system
    "perry/plugin",
    // Perry widget extensions (WidgetKit / Glance)
    "perry/widget",
    // Perry i18n
    "perry/i18n",
    // Node.js worker threads
    "worker_threads",
    // Perry threading primitives (parallelMap, spawn)
    "perry/thread",
    // Perry auto-updater (compareVersions, verifyHash, installUpdate, …)
    "perry/updater",
    // SQLite
    "better-sqlite3",
];

/// Check if a module path refers to a native stdlib module
pub fn is_native_module(path: &str) -> bool {
    let normalized = path.strip_prefix("node:").unwrap_or(path);
    NATIVE_MODULES.contains(&normalized)
}

/// Check if a module path refers to a native module, including external native libraries.
/// External modules are provided by packages with `perry.nativeLibrary` in package.json.
pub fn is_native_module_with_externals(path: &str, externals: &[String]) -> bool {
    let normalized = path.strip_prefix("node:").unwrap_or(path);
    NATIVE_MODULES.contains(&normalized) || externals.iter().any(|ext| ext == normalized)
}

/// Modules that are handled by perry-runtime alone (no stdlib needed).
/// These are Node.js builtins and perry-specific modules implemented in the runtime crate.
const RUNTIME_ONLY_MODULES: &[&str] = &[
    // `net` moved to perry-stdlib (event-driven async TCP) in A1/A1.5 —
    // deliberately NOT in this list so `requires_stdlib("net")` returns true
    // and the auto-optimizer enables the `net` feature on perry-stdlib.
    "fs", "path", "os", "buffer", "child_process", "stream", "url", "util",
    "perry/ui",
    "perry/system",
    "perry/widget",
    "perry/i18n",
    "perry/thread",
];

/// Check if a native module import requires linking perry-stdlib.
/// Returns false for modules that are handled entirely by perry-runtime.
pub fn requires_stdlib(module: &str) -> bool {
    let normalized = module.strip_prefix("node:").unwrap_or(module);
    if !is_native_module(normalized) {
        return false;
    }
    !RUNTIME_ONLY_MODULES.contains(&normalized)
}

/// The kind of module being imported, determining how it's executed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    /// Native TypeScript compiled to machine code (default for .ts/.tsx files)
    NativeCompiled,
    /// Native Rust stdlib implementation (mysql2, pg, etc.)
    NativeRust,
    /// V8-interpreted JavaScript (fallback for .js modules)
    /// This requires explicit opt-in and user confirmation
    Interpreted,
}

impl Default for ModuleKind {
    fn default() -> Self {
        ModuleKind::NativeCompiled
    }
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
    /// External FFI function declarations (name, param_types, return_type)
    /// Populated from `declare function` statements with no body.
    pub extern_funcs: Vec<(String, Vec<Type>, Type)>,
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
    Enum { values: Vec<String>, default: String },
    Bool { default: bool },
    String { default: String },
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
    Frame { width: Option<f64>, height: Option<f64> },
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
    PaddingEdge { edge: String, value: f64 },
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
}

/// Import specifier
#[derive(Debug, Clone)]
pub enum ImportSpecifier {
    /// Named import: import { foo, bar as baz } from "..."
    Named {
        imported: String,
        local: String,
    },
    /// Default import: import foo from "..."
    Default {
        local: String,
    },
    /// Namespace import: import * as foo from "..."
    Namespace {
        local: String,
    },
}

/// An export declaration
#[derive(Debug, Clone)]
pub enum Export {
    /// Named export: export { foo, bar as baz }
    Named {
        local: String,
        exported: String,
    },
    /// Re-export: export { foo } from "..."
    ReExport {
        source: String,
        imported: String,
        exported: String,
    },
    /// Export all: export * from "..."
    ExportAll {
        source: String,
    },
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
    /// Whether this class is exported from the module
    pub is_exported: bool,
}

/// A class field
#[derive(Debug, Clone)]
pub struct ClassField {
    pub name: String,
    pub ty: Type,
    pub init: Option<Expr>,
    pub is_private: bool,
    pub is_readonly: bool,
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
}

/// A function parameter
#[derive(Debug, Clone)]
pub struct Param {
    pub id: LocalId,
    pub name: String,
    pub ty: Type,
    pub default: Option<Expr>,
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
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    /// Do-while loop (body runs at least once, condition checked at the end)
    DoWhile {
        body: Vec<Stmt>,
        condition: Expr,
    },
    /// For loop (lowered from various JS for loops)
    For {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        update: Option<Expr>,
        body: Vec<Stmt>,
    },
    /// Labeled statement: `label: for/while/do/block`
    Labeled {
        label: String,
        body: Box<Stmt>,
    },
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
    Integer(i64), // Integer literal that fits in i64 (for optimization)
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
        op: BinaryOp,      // Add for ++, Sub for --
        prefix: bool,      // true for ++x, false for x++
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
        op: BinaryOp,      // Add for ++, Sub for --
        prefix: bool,      // true for ++x, false for x++
    },

    // Object literal
    Object(Vec<(String, Expr)>),

    // Object literal with spread: { ...src, key: val, ...src2, key2: val2 }
    // Each part is (None, expr) for a spread source, or (Some(key), expr) for a static prop.
    // Parts are ordered to reflect JavaScript evaluation order (later props override earlier spreads).
    ObjectSpread {
        parts: Vec<(Option<String>, Expr)>,
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
    Yield { value: Option<Box<Expr>>, delegate: bool },

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
    ProcessOn { event: Box<Expr>, handler: Box<Expr> },
    // process.chdir(directory) -> void
    ProcessChdir(Box<Expr>),
    // process.kill(pid, signal?) -> void
    ProcessKill { pid: Box<Expr>, signal: Option<Box<Expr>> },
    // process.exit(code?) -> never. Bare `process.exit()` lowers as
    // `ProcessExit(None)` which the runtime treats as code 0.
    ProcessExit(Option<Box<Expr>>),
    // process.stdin -> stub object { write: fn }
    ProcessStdin,
    // process.stdout -> stub object { write: fn }
    ProcessStdout,
    // process.stderr -> stub object { write: fn }
    ProcessStderr,

    // File system operations
    FsReadFileSync(Box<Expr>),           // fs.readFileSync(path) -> string
    FsWriteFileSync(Box<Expr>, Box<Expr>), // fs.writeFileSync(path, content) -> void
    FsExistsSync(Box<Expr>),             // fs.existsSync(path) -> boolean
    FsMkdirSync(Box<Expr>),              // fs.mkdirSync(path) -> void
    FsUnlinkSync(Box<Expr>),             // fs.unlinkSync(path) -> void
    FsAppendFileSync(Box<Expr>, Box<Expr>), // fs.appendFileSync(path, content) -> void
    FsReadFileBinary(Box<Expr>),         // fs.readFileBuffer(path) -> Buffer (binary-safe)
    FsRmRecursive(Box<Expr>),            // fs.rmRecursive(path) -> boolean

    // Path operations
    PathJoin(Box<Expr>, Box<Expr>),      // path.join(a, b) -> string
    PathDirname(Box<Expr>),              // path.dirname(path) -> string
    PathBasename(Box<Expr>),             // path.basename(path) -> string
    PathBasenameExt(Box<Expr>, Box<Expr>), // path.basename(path, ext) -> string (strips ext suffix)
    PathExtname(Box<Expr>),              // path.extname(path) -> string
    PathResolve(Box<Expr>),              // path.resolve(path) -> string
    PathIsAbsolute(Box<Expr>),           // path.isAbsolute(path) -> boolean
    PathRelative(Box<Expr>, Box<Expr>),  // path.relative(from, to) -> string
    PathNormalize(Box<Expr>),            // path.normalize(path) -> string
    PathParse(Box<Expr>),                // path.parse(path) -> { root, dir, base, ext, name }
    PathFormat(Box<Expr>),               // path.format({ dir, base }) -> string
    PathSep,                             // path.sep constant
    PathDelimiter,                       // path.delimiter constant

    // WeakRef and FinalizationRegistry
    WeakRefNew(Box<Expr>),                                                                     // new WeakRef(obj) -> WeakRef
    WeakRefDeref(Box<Expr>),                                                                   // ref.deref() -> object | undefined
    FinalizationRegistryNew(Box<Expr>),                                                        // new FinalizationRegistry(callback) -> registry
    FinalizationRegistryRegister {                                                             // registry.register(target, held, token?)
        registry: Box<Expr>,
        target: Box<Expr>,
        held: Box<Expr>,
        token: Option<Box<Expr>>,
    },
    FinalizationRegistryUnregister { registry: Box<Expr>, token: Box<Expr> },                  // registry.unregister(token) -> bool

    // Object property descriptor methods
    ObjectDefineProperty(Box<Expr>, Box<Expr>, Box<Expr>), // Object.defineProperty(obj, key, desc)
    ObjectGetOwnPropertyDescriptor(Box<Expr>, Box<Expr>),  // Object.getOwnPropertyDescriptor(obj, key)
    ObjectGetOwnPropertyNames(Box<Expr>),                  // Object.getOwnPropertyNames(obj) -> string[]
    ObjectCreate(Box<Expr>),                               // Object.create(proto)
    ObjectFreeze(Box<Expr>),                               // Object.freeze(obj)
    ObjectSeal(Box<Expr>),                                 // Object.seal(obj)
    ObjectPreventExtensions(Box<Expr>),                    // Object.preventExtensions(obj)
    ObjectIsFrozen(Box<Expr>),                             // Object.isFrozen(obj)
    ObjectIsSealed(Box<Expr>),                             // Object.isSealed(obj)
    ObjectIsExtensible(Box<Expr>),                         // Object.isExtensible(obj)
    ObjectGetPrototypeOf(Box<Expr>),                       // Object.getPrototypeOf(obj)
    ObjectGetOwnPropertySymbols(Box<Expr>),                // Object.getOwnPropertySymbols(obj) -> symbol[]

    // Symbol operations
    SymbolNew(Option<Box<Expr>>),                          // Symbol() / Symbol(description)
    SymbolFor(Box<Expr>),                                  // Symbol.for(key) -> registered symbol
    SymbolKeyFor(Box<Expr>),                               // Symbol.keyFor(sym) -> key | undefined
    SymbolDescription(Box<Expr>),                          // sym.description
    SymbolToString(Box<Expr>),                             // sym.toString()

    // URL operations
    FileURLToPath(Box<Expr>),            // url.fileURLToPath(url) -> string

    // RegExp operations
    RegExpExec { regex: Box<Expr>, string: Box<Expr> },
    RegExpSource(Box<Expr>),
    RegExpFlags(Box<Expr>),
    RegExpLastIndex(Box<Expr>),
    RegExpSetLastIndex { regex: Box<Expr>, value: Box<Expr> },
    RegExpReplaceFn { string: Box<Expr>, regex: Box<Expr>, callback: Box<Expr> },
    RegExpExecIndex,
    RegExpExecGroups,

    // JSON operations
    JsonParse(Box<Expr>),                // JSON.parse(string) -> value
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
    JsonParseTyped { text: Box<Expr>, ty: Type, ordered_keys: Option<Vec<String>> },
    JsonParseReviver { text: Box<Expr>, reviver: Box<Expr> },
    JsonParseWithReviver(Box<Expr>, Box<Expr>),
    JsonStringify(Box<Expr>),            // JSON.stringify(value) -> string
    JsonStringifyPretty { value: Box<Expr>, replacer: Option<Box<Expr>>, space: Box<Expr> },
    JsonStringifyFull(Box<Expr>, Box<Expr>, Box<Expr>),

    // Math operations
    MathFloor(Box<Expr>),                // Math.floor(x) -> number
    MathCeil(Box<Expr>),                 // Math.ceil(x) -> number
    MathRound(Box<Expr>),                // Math.round(x) -> number
    MathAbs(Box<Expr>),                  // Math.abs(x) -> number
    MathSqrt(Box<Expr>),                 // Math.sqrt(x) -> number
    MathLog(Box<Expr>),                  // Math.log(x) -> number
    MathLog2(Box<Expr>),                 // Math.log2(x) -> number
    MathLog10(Box<Expr>),                // Math.log10(x) -> number
    MathPow(Box<Expr>, Box<Expr>),       // Math.pow(base, exp) -> number
    MathMin(Vec<Expr>),                  // Math.min(...values) -> number
    MathMax(Vec<Expr>),                  // Math.max(...values) -> number
    MathMinSpread(Box<Expr>),            // Math.min(...array) -> number (spread from single array)
    MathMaxSpread(Box<Expr>),            // Math.max(...array) -> number (spread from single array)
    MathImul(Box<Expr>, Box<Expr>),      // Math.imul(a, b) -> number (32-bit integer multiply)
    MathRandom,                          // Math.random() -> number
    MathSin(Box<Expr>),                  // Math.sin(x) -> number
    MathCos(Box<Expr>),                  // Math.cos(x) -> number
    MathTan(Box<Expr>),                  // Math.tan(x) -> number
    MathAsin(Box<Expr>),                 // Math.asin(x) -> number
    MathAcos(Box<Expr>),                 // Math.acos(x) -> number
    MathAtan(Box<Expr>),                 // Math.atan(x) -> number
    MathAtan2(Box<Expr>, Box<Expr>),     // Math.atan2(y, x) -> number
    MathCbrt(Box<Expr>),                 // Math.cbrt(x) -> number
    MathHypot(Vec<Expr>),                // Math.hypot(...values) -> number
    MathFround(Box<Expr>),               // Math.fround(x) -> number
    MathClz32(Box<Expr>),                // Math.clz32(x) -> number
    MathExpm1(Box<Expr>),                // Math.expm1(x) -> number
    MathLog1p(Box<Expr>),                // Math.log1p(x) -> number
    MathSinh(Box<Expr>),                 // Math.sinh(x) -> number
    MathCosh(Box<Expr>),                 // Math.cosh(x) -> number
    MathTanh(Box<Expr>),                 // Math.tanh(x) -> number
    MathAsinh(Box<Expr>),                // Math.asinh(x) -> number
    MathAcosh(Box<Expr>),                // Math.acosh(x) -> number
    MathAtanh(Box<Expr>),                // Math.atanh(x) -> number
    MathExp(Box<Expr>),                  // Math.exp(x) -> number (e^x)

    /// performance.now() -> number (high-resolution time in ms)
    PerformanceNow,
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

    // Crypto operations
    CryptoRandomBytes(Box<Expr>),        // crypto.randomBytes(size) -> string (hex)
    CryptoRandomUUID,                    // crypto.randomUUID() -> string
    CryptoSha256(Box<Expr>),             // crypto.sha256(data) -> string (hex)
    CryptoMd5(Box<Expr>),                // crypto.md5(data) -> string (hex)

    // OS operations
    OsPlatform,                          // os.platform() -> string ("darwin", "linux", "win32")
    OsArch,                              // os.arch() -> string ("x64", "arm64", etc.)
    OsHostname,                          // os.hostname() -> string
    OsHomedir,                           // os.homedir() -> string
    OsTmpdir,                            // os.tmpdir() -> string
    OsTotalmem,                          // os.totalmem() -> number (bytes)
    OsFreemem,                           // os.freemem() -> number (bytes)
    OsUptime,                            // os.uptime() -> number (seconds)
    OsType,                              // os.type() -> string ("Darwin", "Linux", "Windows_NT")
    OsRelease,                           // os.release() -> string
    OsCpus,                              // os.cpus() -> array of CPU info objects
    OsNetworkInterfaces,                 // os.networkInterfaces() -> object
    OsUserInfo,                          // os.userInfo() -> object
    OsEOL,                               // os.EOL -> string ("\n" or "\r\n")

    // Buffer operations
    BufferFrom {                         // Buffer.from(data, encoding?) -> Buffer
        data: Box<Expr>,
        encoding: Option<Box<Expr>>,
    },
    BufferAlloc {                        // Buffer.alloc(size, fill?) -> Buffer
        size: Box<Expr>,
        fill: Option<Box<Expr>>,
    },
    BufferAllocUnsafe(Box<Expr>),        // Buffer.allocUnsafe(size) -> Buffer
    BufferConcat(Box<Expr>),             // Buffer.concat(list) -> Buffer
    BufferIsBuffer(Box<Expr>),           // Buffer.isBuffer(obj) -> boolean
    BufferByteLength(Box<Expr>),         // Buffer.byteLength(string) -> number
    BufferToString {                     // buffer.toString(encoding?) -> string
        buffer: Box<Expr>,
        encoding: Option<Box<Expr>>,
    },
    BufferLength(Box<Expr>),             // buffer.length -> number
    BufferSlice {                        // buffer.slice(start?, end?) -> Buffer
        buffer: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
    },
    BufferCopy {                         // buffer.copy(target, tStart?, sStart?, sEnd?) -> number
        source: Box<Expr>,
        target: Box<Expr>,
        target_start: Option<Box<Expr>>,
        source_start: Option<Box<Expr>>,
        source_end: Option<Box<Expr>>,
    },
    BufferWrite {                        // buffer.write(string, offset?, encoding?) -> number
        buffer: Box<Expr>,
        string: Box<Expr>,
        offset: Option<Box<Expr>>,
        encoding: Option<Box<Expr>>,
    },
    BufferFill {                         // buffer.fill(value) -> Buffer (same buffer)
        buffer: Box<Expr>,
        value: Box<Expr>,
    },
    BufferEquals {                       // buffer.equals(other) -> boolean
        buffer: Box<Expr>,
        other: Box<Expr>,
    },
    BufferIndexGet {                     // buffer[i] -> number
        buffer: Box<Expr>,
        index: Box<Expr>,
    },
    BufferIndexSet {                     // buffer[i] = value
        buffer: Box<Expr>,
        index: Box<Expr>,
        value: Box<Expr>,
    },

    // Typed array operations
    Uint8ArrayNew(Option<Box<Expr>>),    // new Uint8Array() or new Uint8Array(length) or new Uint8Array(array)
    Uint8ArrayFrom(Box<Expr>),           // Uint8Array.from(arrayLike) -> Uint8Array
    Uint8ArrayLength(Box<Expr>),         // uint8array.length -> number
    Uint8ArrayGet {                      // uint8array[i] -> number
        array: Box<Expr>,
        index: Box<Expr>,
    },
    Uint8ArraySet {                      // uint8array[i] = value
        array: Box<Expr>,
        index: Box<Expr>,
        value: Box<Expr>,
    },

    /// Generic typed array constructor: `new Int32Array([1, 2, 3])` etc.
    /// `kind` is one of the `TYPED_ARRAY_KIND_*` constants.
    /// `arg` is `None` for `new Int32Array()`, `Some(expr)` for `(length)` or `(arrayLike)`.
    TypedArrayNew { kind: u8, arg: Option<Box<Expr>> },

    // Child Process operations
    ChildProcessExecSync {               // execSync(cmd, opts?) -> Buffer | string
        command: Box<Expr>,
        options: Option<Box<Expr>>,
    },
    ChildProcessSpawnSync {              // spawnSync(cmd, args?, opts?) -> SpawnSyncResult
        command: Box<Expr>,
        args: Option<Box<Expr>>,
        options: Option<Box<Expr>>,
    },
    ChildProcessSpawn {                  // spawn(cmd, args?, opts?) -> ChildProcess
        command: Box<Expr>,
        args: Option<Box<Expr>>,
        options: Option<Box<Expr>>,
    },
    ChildProcessExec {                   // exec(cmd, opts?, callback?) -> ChildProcess
        command: Box<Expr>,
        options: Option<Box<Expr>>,
        callback: Option<Box<Expr>>,
    },
    ChildProcessSpawnBackground {        // child_process.spawnBackground(cmd, args, logFile, envJson?) -> {pid, handleId}
        command: Box<Expr>,
        args: Option<Box<Expr>>,
        log_file: Box<Expr>,
        env_json: Option<Box<Expr>>,
    },
    ChildProcessGetProcessStatus(Box<Expr>), // child_process.getProcessStatus(handleId) -> {alive, exitCode}
    ChildProcessKillProcess(Box<Expr>),  // child_process.killProcess(handleId) -> void

    // Fetch operations
    FetchWithOptions {                   // fetch(url, {method, body, headers}) -> Promise<Response>
        url: Box<Expr>,
        method: Box<Expr>,
        body: Box<Expr>,
        headers: Vec<(String, Expr)>,
    },
    FetchGetWithAuth {                   // fetchWithAuth(url, authHeader) -> Promise<Response>
        url: Box<Expr>,
        auth_header: Box<Expr>,
    },
    FetchPostWithAuth {                  // fetchPostWithAuth(url, authHeader, body) -> Promise<Response>
        url: Box<Expr>,
        auth_header: Box<Expr>,
        body: Box<Expr>,
    },

    // Net operations
    NetCreateServer {                    // net.createServer(options?, connectionListener?) -> Server
        options: Option<Box<Expr>>,
        connection_listener: Option<Box<Expr>>,
    },
    NetCreateConnection {                // net.createConnection(port, host?, connectListener?) -> Socket
        port: Box<Expr>,
        host: Option<Box<Expr>>,
        connect_listener: Option<Box<Expr>>,
    },
    NetConnect {                         // net.connect(port, host?, connectListener?) -> Socket
        port: Box<Expr>,
        host: Option<Box<Expr>>,
        connect_listener: Option<Box<Expr>>,
    },

    // Array methods
    ArrayPush { array_id: LocalId, value: Box<Expr> },    // arr.push(value) -> new length
    ArrayPushSpread { array_id: LocalId, source: Box<Expr> }, // arr.push(...src) -> new length
    ArrayPop(LocalId),                                     // arr.pop() -> removed element
    ArrayShift(LocalId),                                   // arr.shift() -> removed element
    ArrayUnshift { array_id: LocalId, value: Box<Expr> }, // arr.unshift(value) -> new length
    ArrayIndexOf { array: Box<Expr>, value: Box<Expr> },  // arr.indexOf(value) -> index
    ArrayIncludes { array: Box<Expr>, value: Box<Expr> }, // arr.includes(value) -> boolean
    ArraySlice { array: Box<Expr>, start: Box<Expr>, end: Option<Box<Expr>> }, // arr.slice(start, end?) -> new array
    ArraySplice { array_id: LocalId, start: Box<Expr>, delete_count: Option<Box<Expr>>, items: Vec<Expr> }, // arr.splice(start, deleteCount?, ...items) -> deleted elements array

    // Array higher-order function methods
    ArrayForEach { array: Box<Expr>, callback: Box<Expr> },  // arr.forEach(fn) -> void
    ArrayMap { array: Box<Expr>, callback: Box<Expr> },      // arr.map(fn) -> new array
    ArrayFilter { array: Box<Expr>, callback: Box<Expr> },   // arr.filter(fn) -> new array
    ArrayFind { array: Box<Expr>, callback: Box<Expr> },     // arr.find(fn) -> element | undefined
    ArrayFindIndex { array: Box<Expr>, callback: Box<Expr> }, // arr.findIndex(fn) -> index | -1
    ArrayFindLast { array: Box<Expr>, callback: Box<Expr> }, // arr.findLast(fn) -> element | undefined
    ArrayFindLastIndex { array: Box<Expr>, callback: Box<Expr> }, // arr.findLastIndex(fn) -> index | -1
    ArrayAt { array: Box<Expr>, index: Box<Expr> },          // arr.at(i) -> element (negative index OK)
    ArraySome { array: Box<Expr>, callback: Box<Expr> },     // arr.some(fn) -> boolean
    ArrayEvery { array: Box<Expr>, callback: Box<Expr> },    // arr.every(fn) -> boolean
    ArrayFlatMap { array: Box<Expr>, callback: Box<Expr> },  // arr.flatMap(fn) -> new array
    ArraySort { array: Box<Expr>, comparator: Box<Expr> },   // arr.sort(fn) -> same array (in-place)
    ArrayReduce { array: Box<Expr>, callback: Box<Expr>, initial: Option<Box<Expr>> }, // arr.reduce(fn, init?) -> value
    ArrayReduceRight { array: Box<Expr>, callback: Box<Expr>, initial: Option<Box<Expr>> }, // arr.reduceRight(fn, init?) -> value
    ArrayJoin { array: Box<Expr>, separator: Option<Box<Expr>> }, // arr.join(separator?) -> string
    ArrayFlat { array: Box<Expr> },                          // arr.flat() -> flattened array
    ArrayToReversed { array: Box<Expr> },                    // arr.toReversed() -> new reversed array
    ArrayToSorted { array: Box<Expr>, comparator: Option<Box<Expr>> }, // arr.toSorted(fn?) -> new sorted array
    ArrayToSpliced { array: Box<Expr>, start: Box<Expr>, delete_count: Box<Expr>, items: Vec<Expr> }, // arr.toSpliced(start, deleteCount, ...items) -> new array
    ArrayWith { array: Box<Expr>, index: Box<Expr>, value: Box<Expr> }, // arr.with(index, value) -> new array
    ArrayCopyWithin { array_id: LocalId, target: Box<Expr>, start: Box<Expr>, end: Option<Box<Expr>> }, // arr.copyWithin(target, start, end?) -> same array
    ArrayEntries(Box<Expr>),                                 // arr.entries() -> Array<[index, value]> (eager materialization)
    ArrayKeys(Box<Expr>),                                    // arr.keys() -> Array<index>
    ArrayValues(Box<Expr>),                                  // arr.values() -> Array<value> (essentially clone)

    // String methods
    StringSplit(Box<Expr>, Box<Expr>),  // string.split(delimiter) -> string[]
    StringFromCharCode(Box<Expr>),      // String.fromCharCode(code) -> single-char string
    StringFromCodePoint(Box<Expr>),     // String.fromCodePoint(code) -> string
    StringAt { string: Box<Expr>, index: Box<Expr> },         // str.at(i) -> string | undefined (negative supported)
    StringCodePointAt { string: Box<Expr>, index: Box<Expr> }, // str.codePointAt(i) -> number | undefined

    // Map operations
    MapNew,                                                    // new Map() -> empty map
    MapNewFromArray(Box<Expr>),                                // new Map([[k,v], ...]) -> map from entries
    MapSet { map: Box<Expr>, key: Box<Expr>, value: Box<Expr> }, // map.set(key, value) -> map
    MapGet { map: Box<Expr>, key: Box<Expr> },                 // map.get(key) -> value | undefined
    MapHas { map: Box<Expr>, key: Box<Expr> },                 // map.has(key) -> boolean
    MapDelete { map: Box<Expr>, key: Box<Expr> },              // map.delete(key) -> boolean
    MapSize(Box<Expr>),                                        // map.size -> number
    MapClear(Box<Expr>),                                       // map.clear() -> void
    MapEntries(Box<Expr>),                                     // map.entries() -> Array<[key, value]>
    MapKeys(Box<Expr>),                                        // map.keys() -> Array<key>
    MapValues(Box<Expr>),                                      // map.values() -> Array<value>

    // Set operations
    SetNew,                                                    // new Set() -> empty set
    SetNewFromArray(Box<Expr>),                                // new Set(array) -> set from iterable
    SetAdd { set_id: LocalId, value: Box<Expr> },              // set.add(value) -> set (updates local)
    SetHas { set: Box<Expr>, value: Box<Expr> },               // set.has(value) -> boolean
    SetDelete { set: Box<Expr>, value: Box<Expr> },            // set.delete(value) -> boolean
    SetSize(Box<Expr>),                                        // set.size -> number
    SetClear(Box<Expr>),                                       // set.clear() -> void
    SetValues(Box<Expr>),                                      // set.values() -> Array (via js_set_to_array)

    // Sequence expression (comma operator)
    Sequence(Vec<Expr>),

    // Date operations
    DateNow,                              // Date.now() -> number (timestamp in ms)
    DateNew(Option<Box<Expr>>),           // new Date() or new Date(timestamp) -> Date object
    DateGetTime(Box<Expr>),               // date.getTime() -> number
    DateToISOString(Box<Expr>),           // date.toISOString() -> string
    DateGetFullYear(Box<Expr>),           // date.getFullYear() -> number
    DateGetMonth(Box<Expr>),              // date.getMonth() -> number (0-11)
    DateGetDate(Box<Expr>),               // date.getDate() -> number (1-31)
    DateGetHours(Box<Expr>),              // date.getHours() -> number (0-23)
    DateGetMinutes(Box<Expr>),            // date.getMinutes() -> number (0-59)
    DateGetSeconds(Box<Expr>),            // date.getSeconds() -> number (0-59)
    DateGetMilliseconds(Box<Expr>),       // date.getMilliseconds() -> number (0-999)

    // Date static methods
    DateParse(Box<Expr>),                 // Date.parse(isoString) -> number
    DateUtc(Vec<Expr>),                   // Date.UTC(year, month, day, h?, m?, s?) -> number

    // Date getters (UTC variants - for Perry these are the same since we store UTC timestamps)
    DateGetUtcDay(Box<Expr>),             // date.getUTCDay() -> number (0-6)
    DateGetUtcFullYear(Box<Expr>),        // date.getUTCFullYear() -> number
    DateGetUtcMonth(Box<Expr>),           // date.getUTCMonth() -> number (0-11)
    DateGetUtcDate(Box<Expr>),            // date.getUTCDate() -> number (1-31)
    DateGetUtcHours(Box<Expr>),           // date.getUTCHours() -> number (0-23)
    DateGetUtcMinutes(Box<Expr>),         // date.getUTCMinutes() -> number (0-59)
    DateGetUtcSeconds(Box<Expr>),         // date.getUTCSeconds() -> number (0-59)
    DateGetUtcMilliseconds(Box<Expr>),    // date.getUTCMilliseconds() -> number (0-999)

    // Date setters (UTC variants) — return the new timestamp
    DateSetUtcFullYear { date: Box<Expr>, value: Box<Expr> },
    DateSetUtcMonth { date: Box<Expr>, value: Box<Expr> },
    DateSetUtcDate { date: Box<Expr>, value: Box<Expr> },
    DateSetUtcHours { date: Box<Expr>, value: Box<Expr> },
    DateSetUtcMinutes { date: Box<Expr>, value: Box<Expr> },
    DateSetUtcSeconds { date: Box<Expr>, value: Box<Expr> },
    DateSetUtcMilliseconds { date: Box<Expr>, value: Box<Expr> },

    // Date misc
    DateValueOf(Box<Expr>),               // date.valueOf() -> number (same as getTime)
    DateToDateString(Box<Expr>),          // date.toDateString() -> string
    DateToTimeString(Box<Expr>),          // date.toTimeString() -> string
    DateToLocaleDateString(Box<Expr>),    // date.toLocaleDateString() -> string
    DateToLocaleTimeString(Box<Expr>),    // date.toLocaleTimeString() -> string
    DateToLocaleString(Box<Expr>),        // date.toLocaleString() -> string
    DateGetTimezoneOffset(Box<Expr>),     // date.getTimezoneOffset() -> number
    DateToJSON(Box<Expr>),                // date.toJSON() -> string

    // Error operations
    ErrorNew(Option<Box<Expr>>),          // new Error() or new Error(message) -> Error object
    ErrorMessage(Box<Expr>),              // error.message -> string
    /// new Error(message, { cause })
    ErrorNewWithCause { message: Box<Expr>, cause: Box<Expr> },
    /// new TypeError(message)
    TypeErrorNew(Box<Expr>),
    /// new RangeError(message)
    RangeErrorNew(Box<Expr>),
    /// new ReferenceError(message)
    ReferenceErrorNew(Box<Expr>),
    /// new SyntaxError(message)
    SyntaxErrorNew(Box<Expr>),
    /// new AggregateError(errors, message)
    AggregateErrorNew { errors: Box<Expr>, message: Box<Expr> },

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

    // Delete operator
    Delete(Box<Expr>),                    // delete obj.prop or delete obj["prop"] -> bool

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
    ObjectGroupBy { items: Box<Expr>, key_fn: Box<Expr> },
    /// Object rest destructuring: copies all properties except the excluded keys
    /// Used for `const { a, b, ...rest } = obj` → rest = ObjectRest(obj, ["a", "b"])
    ObjectRest { object: Box<Expr>, exclude_keys: Vec<String> },

    // Array static methods
    /// Array.isArray(value) -> boolean
    /// Returns true if the value is an array
    ArrayIsArray(Box<Expr>),
    /// Array.from(iterable) -> Array
    /// Creates a new array from an iterable (e.g., Map.entries(), Map.keys(), another array)
    ArrayFrom(Box<Expr>),
    IteratorToArray(Box<Expr>),          // collect iterator (.next() loop) into array
    /// Array.from(iterable, mapFn) -> Array
    /// Creates a new array by applying mapFn to each element of the iterable.
    ArrayFromMapped { iterable: Box<Expr>, map_fn: Box<Expr> },

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
    ProxyNew { target: Box<Expr>, handler: Box<Expr> },
    ProxyGet { proxy: Box<Expr>, key: Box<Expr> },
    ProxySet { proxy: Box<Expr>, key: Box<Expr>, value: Box<Expr> },
    ProxyHas { proxy: Box<Expr>, key: Box<Expr> },
    ProxyDelete { proxy: Box<Expr>, key: Box<Expr> },
    ProxyApply { proxy: Box<Expr>, args: Vec<Expr> },
    ProxyConstruct { proxy: Box<Expr>, args: Vec<Expr> },
    ProxyRevocable { target: Box<Expr>, handler: Box<Expr> },
    ProxyRevoke(Box<Expr>),
    ReflectGet { target: Box<Expr>, key: Box<Expr> },
    ReflectSet { target: Box<Expr>, key: Box<Expr>, value: Box<Expr> },
    ReflectHas { target: Box<Expr>, key: Box<Expr> },
    ReflectDelete { target: Box<Expr>, key: Box<Expr> },
    ReflectOwnKeys(Box<Expr>),
    ReflectApply { func: Box<Expr>, this_arg: Box<Expr>, args: Box<Expr> },
    ReflectConstruct { target: Box<Expr>, args: Box<Expr> },
    ReflectDefineProperty { target: Box<Expr>, key: Box<Expr>, descriptor: Box<Expr> },
    ReflectGetPrototypeOf(Box<Expr>),
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
    Eq,       // ===
    Ne,       // !==
    LooseEq,  // ==
    LooseNe,  // !=
    Lt,       // <
    Le,       // <=
    Gt,       // >
    Ge,       // >=
}

/// Logical operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And, // &&
    Or,  // ||
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
            extern_funcs: Vec::new(),
        }
    }
}
