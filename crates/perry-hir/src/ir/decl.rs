//! Top-level HIR declarations: Enum, Interface, TypeAlias, Import, Export,
//! Class, ClassField, Global, Decorator, Function, Param. Re-exported from
//! `super`.

use super::*;
use perry_types::{FuncId, GlobalId, LocalId, Type, TypeParam};

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
    /// Issue #1672: this source is the target of at least one dynamic
    /// `import()` site in the module, but the edge could NOT be a
    /// dedicated `is_dynamic` synthetic edge because a *static* import of
    /// the same source already exists (the fold in `collect_modules`
    /// keeps the static edge for binding materialization + init order).
    /// The static edge therefore stays `is_dynamic = false`, and this
    /// flag is set on it so the driver still registers the source in the
    /// dynamic-import dispatch map (`dynamic_import_path_to_prefix`) and
    /// marks the target module as a namespace-emitting dynamic target.
    /// Always `false` on `is_dynamic` synthetic edges (those are already
    /// dynamic targets by virtue of `is_dynamic`).
    pub is_dynamic_target: bool,
    /// Next.js lazy-require: this `import _req_N from 'S'` was synthesized by
    /// the CJS→ESM wrap from a `require('S')` whose every call site is inside a
    /// FUNCTION body (never module top-level). Node loads such a module lazily
    /// — only when the enclosing function runs — so it must NOT pin the target
    /// eager. Like `is_dynamic`, the target still enters the compile graph but
    /// is left `Deferred` unless some other (top-level) edge reaches it; the
    /// require shim triggers the target's `__init` on first `require()` call.
    pub is_deferred_require: bool,
    /// Issue #5257: this import was synthesized by the CJS→ESM wrap from a
    /// `require('S')` — i.e. `import _req_N from 'S'` (or an adopted alias /
    /// `_lazyreq_N`). Under CommonJS, `require('S')` returns the module's
    /// *exports object* (its namespace), so a default-import shape here must
    /// NOT be held to Node's static-ESM "does not provide an export named
    /// 'default'" rule when the target is a named-only / CJS module: the
    /// default-export gate skips these and codegen routes the local through
    /// the namespace machinery (member reads resolve per-export, a whole-value
    /// read materializes the exports object). Genuine user `import X from
    /// 'pkg'` (this flag `false`) still errors like Node.
    pub is_adopted_require: bool,
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
    /// #5437: the parent name resolves to an in-scope LEXICAL LOCAL that shadows
    /// a same-named native/built-in/module-global parent (`const Error = class
    /// {…}; class X extends Error {}`). When set, `super()` codegen must use the
    /// dynamic `extends_expr` value (the local) and MUST NOT take any built-in
    /// special-case path keyed on the parent NAME (Error/Request/Response/Event/
    /// CustomEvent/stream family) — that would run the built-in initializer
    /// instead of the lexical local's constructor.
    pub heritage_lexically_shadowed: bool,
    /// Instance fields
    pub fields: Vec<ClassField>,
    /// Constructor (if any)
    pub constructor: Option<Function>,
    /// Instance methods
    pub methods: Vec<Function>,
    /// Instance getter methods (property_name -> function that returns the value)
    pub getters: Vec<(String, Function)>,
    /// Instance setter methods (property_name -> function that takes the value)
    pub setters: Vec<(String, Function)>,
    /// Property names of accessors that are `static` (`static get x()` /
    /// `static set x(v)`). The accessor functions themselves live in `getters`
    /// / `setters` alongside instance accessors (so every IR pass — async
    /// lowering, finally-inline, generator id-scan, inlining — processes their
    /// bodies uniformly); codegen consults this set to register them on the
    /// class constructor (`CLASS_STATIC_ACCESSORS`) rather than the instance
    /// vtable, since a static accessor is an own property of `C`, not of
    /// `C.prototype`/instances.
    pub static_accessor_names: Vec<String>,
    /// Function ids of the accessor entries in `getters` / `setters` that are
    /// `static`. `static_accessor_names` alone cannot disambiguate a name that
    /// is BOTH a static and an instance accessor (`static get 0(){} get 0(){}`)
    /// — the by-name check classified the instance entry as static too, so both
    /// emitted under the same `perry_static_…__get_0` symbol (LLVM "invalid
    /// redefinition"). Keying the static/instance split on the accessor
    /// function's unique id is unambiguous. Preserved across monomorphization
    /// (specialize.rs copies `f.id` verbatim).
    pub static_accessor_fn_ids: Vec<FuncId>,
    /// Static fields
    pub static_fields: Vec<ClassField>,
    /// Static methods
    pub static_methods: Vec<Function>,
    /// Computed-key methods/accessors, preserved in source order so
    /// declaration-time key side effects fire in the same order as JS.
    pub computed_members: Vec<ClassComputedMember>,
    /// Legacy TypeScript decorators applied to the class.
    pub decorators: Vec<Decorator>,
    /// Whether this class is exported from the module
    pub is_exported: bool,
    /// Self-binding aliases for class-expression bindings:
    /// `var X = class _X { ... new _X() ... }` records `_X` here so codegen
    /// can look it up as the same class. Refs #486.
    pub aliases: Vec<String>,
    /// Whether this class was declared/expressed INSIDE a function body (not at
    /// module top level), even though HIR hoists it into `module.classes`. A
    /// nested class's static-field initializers must run when the enclosing
    /// function evaluates the class — NOT at module init. Running a nested
    /// class's side-effectful static initializer (e.g. `static #a = new Self()`)
    /// eagerly at module init both mistimes it and can crash before any user
    /// code (Next.js wall 54: NextResponse's `static #a = this.EMPTY = new z()`
    /// inside a turbopack factory threw at module init). Codegen
    /// (`init_static_fields_*`) skips module-init static init for these.
    pub is_nested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassComputedMemberKind {
    Method,
    Getter,
    Setter,
}

#[derive(Debug, Clone)]
pub struct ClassComputedMember {
    pub key_expr: Expr,
    pub function: Function,
    pub is_static: bool,
    pub kind: ClassComputedMemberKind,
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
    pub is_strict: bool,
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
pub struct ArgumentsObjectMeta {
    /// Whether the containing function body is strict.
    pub strict: bool,
    /// Whether the containing function has a simple parameter list.
    pub simple_parameters: bool,
    /// Sloppy mapped arguments bind numeric indices to these parameter locals.
    pub mapped_parameter_ids: Vec<(u32, LocalId)>,
    /// Whether `arguments.callee` is the restricted throwing accessor.
    pub restricted_callee: bool,
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
    /// Metadata for the hidden raw-arguments binding used to materialize the
    /// ECMAScript `arguments` object in the callee prologue.
    pub arguments_object: Option<ArgumentsObjectMeta>,
}
