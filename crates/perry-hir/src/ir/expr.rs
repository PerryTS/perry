//! The HIR `Expr` enum — every JavaScript/TypeScript expression form Perry
//! recognises. By far the largest type in this module; kept in its own file so
//! mod.rs stays scannable. Re-exported from `super`.

use super::*;
use crate::types::{FuncId, GlobalId, LocalId, Type};

/// Fallback when a dynamic `with` object environment does not bind the
/// assignment target. The object lookup is performed before the RHS is
/// evaluated, matching ECMAScript Reference resolution order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WithSetFallback {
    Local(LocalId),
    ThrowReferenceError,
    ThrowConstAssignment,
    Ignore,
    SloppyImplicit(LocalId),
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
    /// Dynamic object-environment read produced by `with (obj) { name }`.
    /// If `obj` has a non-unscopable property named `property`, read it;
    /// otherwise evaluate `fallback` (outer lexical/global resolution).
    WithGet {
        object: Box<Expr>,
        property: String,
        fallback: Box<Expr>,
    },
    /// Dynamic object-environment write produced by `with (obj) { name = v }`.
    /// Codegen probes the object before lowering `value`; strict PutValue then
    /// re-checks that the property survived RHS side effects.
    WithSet {
        object: Box<Expr>,
        property: String,
        value: Box<Expr>,
        fallback: WithSetFallback,
        strict: bool,
    },

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
        /// #5247: byte offset (`call.span.lo.0`) of this call expression in its
        /// module's source, captured at AST→HIR lowering. Used by codegen (under
        /// `--debug-symbols`) to attach a `file:line` to the runtime "X is not a
        /// function" TypeError thrown by the dynamic method-dispatch path. `0`
        /// when unknown (synthesized calls from transforms/intrinsics, etc.) —
        /// a 0 sentinel resolves to no location, falling back to `<anonymous>`.
        byte_offset: u32,
    },

    /// Function call with spread arguments (e.g., fn(a, ...arr, b))
    CallSpread {
        callee: Box<Expr>,
        args: Vec<CallArg>,
        type_args: Vec<Type>,
    },

    /// `super(...)` with spread arguments (`super(...arguments)` — the tsc
    /// pass-through-ctor emit zod's ZodNumber/ZodBigInt use). The parent
    /// ctor is invoked at runtime through the CLASS_CONSTRUCTORS registry
    /// with the materialized args array (codegen can't inline a dynamic
    /// arg count).
    SuperCallSpread(Vec<CallArg>),

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
        /// #5247: byte offset (`member.span.lo.0`) of this member access in its
        /// module source, captured at AST→HIR lowering. Codegen (under
        /// `--debug-symbols`) resolves it to `file:line` and attaches it to the
        /// runtime "Cannot read properties of null/undefined" TypeError thrown
        /// when the receiver is nullish, so `.stack` shows `at <file>:<line>`
        /// instead of `<anonymous>`. `0` when unknown (nodes synthesized by
        /// transforms/desugaring) — resolves to no location. Mirrors
        /// `Call.byte_offset` / `New::byte_offset`; excluded from stable-hashing.
        byte_offset: u32,
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
    /// Private-name brand check: `#field in obj`.
    ///
    /// This is intentionally separate from `In { property: "#field", ... }`
    /// so ordinary public string keys cannot satisfy private-field syntax.
    PrivateBrandCheck {
        class_name: String,
        field_name: String,
        object: Box<Expr>,
    },

    /// Brand+kind guard wrapping the receiver of a private member access
    /// `obj.#name`. Evaluates `object` exactly once and returns its value
    /// UNCHANGED when the access is legal; otherwise throws a `TypeError`.
    ///
    /// Two checks run, in spec order:
    ///   1. Brand check — `object` must be an instance of `class_name` (the
    ///      class that lexically declares `#name`). A wrong receiver (an
    ///      ordinary object, or an instance of an unrelated/outer class)
    ///      throws.
    ///   2. Kind/op check — reading a setter-only accessor, writing a
    ///      getter-only accessor, or writing a private method all throw.
    ///
    /// Because it returns the receiver, it composes with the existing
    /// `PropertyGet` / `PropertySet` / method-call lowering: those operate on
    /// the guard's result and need no private-specific changes. `kind` and
    /// `op` are the wire codes defined by `PrivKind` / 0=get,1=set.
    PrivateGuard {
        class_name: String,
        /// The declaring class's unique HIR id. Codegen uses this directly as
        /// the brand's `declaring_class_id` rather than resolving `class_name`
        /// through the `class_ids` map, whose name keys collapse duplicate
        /// (minified) class names last-writer-wins and would otherwise bind the
        /// brand to the wrong same-named class. 0 means "unresolved" (guard
        /// degrades to a no-op).
        class_id: u32,
        field_name: String,
        kind: u8,
        op: u8,
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
        /// #5253: byte offset (`new_expr.span.lo.0`) of this `new` expression
        /// in its module's source, captured at AST→HIR lowering. Used by
        /// codegen (under `--debug-symbols`) to attach a `file:line` to the
        /// runtime "X is not a constructor" TypeError. `0` when unknown
        /// (synthesized `new` from transforms/intrinsics) — resolves to no
        /// location, falling back to `<anonymous>`. Mirrors `Call.byte_offset`
        /// (#5247) and is excluded from stable-hashing.
        byte_offset: u32,
        /// #6538: how many of the TRAILING `args` are compiler-appended
        /// class-capture forwards, NOT user arguments. When a class nested in
        /// a function captures enclosing-scope locals, `lower_class_decl`
        /// synthesizes one `__perry_cap_<id>` constructor param per captured
        /// id, and the bare-identifier `new C(...)` / anonymous-class arms
        /// (`expr_new.rs`, `expr_new/non_ident.rs`) push one `LocalGet(<id>)`
        /// per captured id after the user args. This count records that
        /// provenance EXPLICITLY so codegen no longer has to infer it from the
        /// arg shape (the old `new_site_args_carry_appended_caps` heuristic,
        /// which could misfire on a forward-referenced capture class whose
        /// user args happened to be exactly its captured locals). `0` for
        /// every other `new` site — non-capturing classes, member-callee
        /// `new ns.C(...)` (caps filled from the decl-site snapshot instead),
        /// synthesized options-object shapes, and transform-created nodes.
        /// Excluded from stable-hashing (derived metadata, like `byte_offset`;
        /// the appended `LocalGet` args it counts are themselves hashed).
        cap_args_appended: u32,
    },

    /// Dynamic new expression (new with non-identifier callee)
    /// e.g., new (condition ? ClassA : ClassB)()
    /// or new someVariable()
    NewDynamic {
        /// The expression that evaluates to a constructor
        callee: Box<Expr>,
        /// Arguments to pass to the constructor
        args: Vec<Expr>,
        /// #5253: source byte offset of the `new` expression — see
        /// `New::byte_offset`. The `const X: any = undefined; new X()`
        /// not-a-constructor case lowers here (callee is `LocalGet`), so this
        /// is the field that localizes ajv's `undefined is not a constructor`.
        byte_offset: u32,
    },

    /// Dynamic `new` with spread arguments — `new <callee>(...args)`.
    /// Kept distinct from `NewDynamic` so the spread positions survive
    /// lowering (a plain `Vec<Expr>` would collapse `...[1,2]` into a single
    /// array argument). Codegen folds every argument into one JS array
    /// (regular pushed, spread sources expanded) and dispatches through
    /// `js_new_function_construct_apply`.
    NewDynamicSpread {
        callee: Box<Expr>,
        args: Vec<CallArg>,
        /// #5253: source byte offset of the `new` expression — see
        /// `New::byte_offset`.
        byte_offset: u32,
    },

    /// Runtime `new.target` value for ordinary functions.
    NewTarget,

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

    /// Snapshot the CURRENT values of a function-nested class's captured
    /// outer-scope locals into the runtime `CLASS_CAPTURE_VALUES` table.
    /// Emitted at the source-order position of the class declaration
    /// (parallel to `RegisterClassParentDynamic`), so dynamic construction
    /// of the class VALUE (`exports.C = C; … new mod.C()` — the webpack /
    /// zod bundle pattern) can fill the synthesized `__perry_cap_<id>`
    /// constructor params. Static `new C()` sites keep passing captures as
    /// trailing args and don't consult the table.
    RegisterClassCaptures {
        class_name: String,
        captures: Vec<Expr>,
    },

    /// Read slot `index` of a class's decl-site capture snapshot
    /// (`CLASS_CAPTURE_VALUES`, written by `RegisterClassCaptures`). Used by
    /// STATIC method bodies of function-nested capturing classes — statics
    /// have no instance to carry `__perry_cap_*` fields, so their prologue
    /// rebinds read the snapshot instead (vendored zod's
    /// `static create(...) { … typeName: k.ZodRecord … }` where `k` is an
    /// enclosing-function local).
    ClassCaptureValue {
        class_name: String,
        index: u32,
        /// #5437 (cross-module member-`new`): when present, codegen emits
        /// `js_class_capture_value_or(cid, index, <fallback>)` — the decl-site
        /// snapshot wins WHEN IT HOLDS A REAL VALUE, otherwise the fallback
        /// (the `new`-site appended/synthesized cap arg) is used. The
        /// synthesized constructor stashes its `__perry_cap_*` fields through
        /// this form so a CROSS-MODULE `new ns.C(...)` — which routes to the
        /// runtime construct path (`construct_registered_class_ref`) and
        /// supplies NO cap args, leaving the cap params bound to garbage —
        /// still recovers the captured value from the class's own decl-site
        /// snapshot (the ctor body is compiled in the class's home module, so
        /// `class_name` resolves to its real `class_id` there). `None` keeps
        /// the plain snapshot read used by STATIC method prologue rebinds.
        fallback: Option<Box<Expr>>,
        /// #5437 (param-first rebind): when `true`, the `fallback` (the LIVE
        /// `new`-site cap param) WINS whenever it is present; the decl-site
        /// snapshot is consulted ONLY when the param is `undefined` (the
        /// cross-module construct signal — that path drops the cap arg). Codegen
        /// emits `js_param_or_class_capture_value(param, cid, index)`. This is
        /// the correct policy for the synthesized constructor's capture rebind:
        /// a SAME-module `new C(...)` supplies the current (possibly mutated)
        /// outer, which must not be overridden by a stale decl-site snapshot.
        /// `false` (default) keeps the snapshot-first
        /// `js_class_capture_value_or` behavior used by every other caller.
        prefer_fallback: bool,
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

    /// Register a computed class method after evaluating the source key
    /// through runtime `ToPropertyKey` semantics.
    RegisterClassComputedMethod {
        class_name: String,
        key_expr: Box<Expr>,
        method_name: String,
        is_static: bool,
        param_count: u32,
        has_rest: bool,
    },

    /// Register one side of a computed class accessor.
    RegisterClassComputedAccessor {
        class_name: String,
        key_expr: Box<Expr>,
        getter_name: Option<String>,
        setter_name: Option<String>,
        is_static: bool,
    },

    /// Issue #1772: per-evaluation identity for a class EXPRESSION
    /// (`class C { ... }` in value position, e.g. effect's `make(ast) =>
    /// class SchemaClass { static ast = ast }`). Codegen allocates a real
    /// heap "class object" (`js_object_alloc(template_class_id, n)`) stamped
    /// with the compile-time `template`'s class_id — so static methods /
    /// `new` / `instanceof` keep dispatching through the existing class_id
    /// machinery — and writes the per-evaluation static fields as the
    /// object's OWN properties (`named_statics` via
    /// `js_object_set_field_by_name`, `symbol_statics` via
    /// `js_object_set_symbol_property`). The class value is the object
    /// POINTER, so `make(a) !== make(b)` (distinct heap allocations) and
    /// each carries its own `static ast`. Because it is a normal traced
    /// heap object it is collectible — no leak. The static-field
    /// initializers are evaluated against the current scope each time the
    /// expression runs. Top-level class DECLARATIONS keep `INT32(class_id)`.
    ClassExprFresh {
        template: String,
        named_statics: Vec<(String, Expr)>,
        symbol_statics: Vec<(Expr, Expr)>,
        /// #1787: the captured outer-scope values this class expression
        /// closes over, in the synthesized constructor's capture-param
        /// order (see `synthesize_class_captures`). Each entry is a
        /// `LocalGet(outer_id)` evaluated at the class-expression
        /// evaluation site (where the captures are still live). Codegen
        /// snapshots them onto the heap class object so a later
        /// `new <classObjectValue>()` can replay the instance-field
        /// initializers / constructor body with the right captured
        /// environment — which the static `new ClassName()` inlining
        /// can't do once the class escapes its defining scope.
        captured_args: Vec<Expr>,
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

    /// `super.method(...spread)` with one or more spread arguments. Mirrors
    /// `SuperCallSpread` for the method-call shape: the plain `SuperMethodCall`
    /// drops the spread marker and would pass the spread operand (an array) as
    /// ONE positional argument, so a `super.emit(event, ...args)` forwarding a
    /// rest param to a native base (EventEmitter) delivered `[payload]` instead
    /// of `payload`. Codegen flattens every arg (regular + spread-expanded)
    /// into a single args array and dispatches through the runtime super
    /// helper, which already takes an args buffer.
    SuperMethodCallSpread {
        method: String,
        args: Vec<CallArg>,
    },

    // Super property read (value form). super.<prop>. Resolved at
    // codegen by walking the parent class's method table (issue #774).
    SuperPropertyGet {
        property: String,
    },

    // Super property assignment. Codegen resolves the current class's parent
    // prototype at the use site, evaluates key before value, then performs
    // ordinary [[Set]] with receiver=this.
    SuperPropertySet {
        parent_class_id: u32,
        parent_class_name: Option<String>,
        key: Box<Expr>,
        value: Box<Expr>,
    },

    // Object-literal method `super[key]` read. `home` is the hidden home
    // object captured when the method literal is created; `receiver` is the
    // dynamic `this` for the current call.
    ObjectSuperPropertyGet {
        home: Box<Expr>,
        key: Box<Expr>,
        receiver: Box<Expr>,
    },

    // Object-literal method `super[key] = value`.
    ObjectSuperPropertySet {
        home: Box<Expr>,
        key: Box<Expr>,
        value: Box<Expr>,
        receiver: Box<Expr>,
    },

    // Object-literal method `super[key](args...)` call.
    ObjectSuperMethodCall {
        home: Box<Expr>,
        key: Box<Expr>,
        receiver: Box<Expr>,
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
    /// `this` in module top-level code. Node runs the assembled test files
    /// as CJS, where top-level `this` is `module.exports` — a fresh plain
    /// object distinct from `globalThis`. Lowered separately from
    /// `Expr::This` so function-body `this` semantics are untouched.
    ModuleTopThis,
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
    ProcessHrtimeBigint, // process.hrtime.bigint() -> bigint (nanoseconds since arbitrary point)
    ProcessHrtime(Option<Box<Expr>>), // process.hrtime(prior?) -> [secs, nanos] (diff if prior) (#1345)
    // process.nextTick(callback, ...args) -> void.
    // Trailing args are forwarded to the callback when it fires (#1351).
    ProcessNextTick {
        callback: Box<Expr>,
        args: Vec<Expr>,
    },
    // process.on(event, handler) -> void (registers an event listener)
    ProcessOn {
        event: Box<Expr>,
        handler: Box<Expr>,
    },
    // process.once(event, handler) -> void (one-shot listener)
    ProcessOnce {
        event: Box<Expr>,
        handler: Box<Expr>,
    },
    ProcessChdir(Box<Expr>), // process.chdir(directory) -> void
    // process.kill(pid, signal?) -> void
    ProcessKill {
        pid: Box<Expr>,
        signal: Option<Box<Expr>>,
    },
    ProcessExit(Option<Box<Expr>>), // process.exit(code?) -> never; None means code 0
    ProcessAbort,                   // process.abort() -> never; raises SIGABRT
    ProcessUmask(Option<Box<Expr>>), // process.umask(mask?) -> number; no-arg reads, arg sets and returns previous
    ProcessThreadCpuUsage(Option<Box<Expr>>), // process.threadCpuUsage(prior?) -> { user, system } µs
    ProcessAvailableMemory, // process.availableMemory() -> number (free memory bytes)
    ProcessConstrainedMemory, // process.constrainedMemory() -> number (OS limit, 0 if unconstrained)
    ProcessPosixCredential(super::PosixCredentialKind), // process.{getuid,geteuid,getgid,getegid}() (#1408)
    ProcessEmitWarning(Vec<Expr>), // process.emitWarning(warning[, type, code, ctor]) -> undefined (#1375)
    ProcessCpuUsage(Option<Box<Expr>>), // process.cpuUsage(prior?) -> { user, system } µs (diff if prior given)
    ProcessResourceUsage, // process.resourceUsage() -> {userCPUTime, maxRSS, ...} (#1376)
    ProcessActiveResourcesInfo, // process.getActiveResourcesInfo() -> string[] (#1376)
    ProcessTitle,         // process.title getter (#1401)
    ProcessSetTitle(Box<Expr>), // process.title = X setter (#1401)
    ProcessStdin,         // process.stdin -> stub object { write: fn }
    ProcessStdout,        // process.stdout -> stub object { write: fn }
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
    // process.stdin.removeListener/off(event, handler) -> stdin (#3962)
    ProcessStdinRemoveListener {
        event: Box<Expr>,
        handler: Box<Expr>,
    },
    // process.stdin.pause/resume/unref/ref/destroy() -> stdin (#3962)
    ProcessStdinLifecycle(ProcessStdinLifecycleMethod),
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
    PathToNamespacedPath(Box<Expr>), // path.toNamespacedPath(path) -> string, or original non-string
    PathMatchesGlob(Box<Expr>, Box<Expr>), // path.matchesGlob(path, pattern) -> boolean
    PathResolveJoin(Box<Expr>, Box<Expr>), // internal: join with reset-on-absolute (multi-arg resolve)
    PathWin32Join(Box<Expr>, Box<Expr>),   // path.win32.join(a, b) -> string (issue #810)
    /// All other `path.win32.<method>(args...)` calls. One variant for the
    /// whole sub-namespace to avoid 15× boilerplate across walker/codegen
    /// touch sites; the codegen dispatches on `method` to the right
    /// `js_path_win32_*` runtime call. Issue #1162.
    PathWin32 {
        method: PathWin32Method,
        args: Vec<Expr>,
    },

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
    ObjectGetOwnPropertyDescriptors(Box<Expr>), // Object.getOwnPropertyDescriptors(obj) -> { [k]: descriptor }
    ObjectGetOwnPropertyNames(Box<Expr>),       // Object.getOwnPropertyNames(obj) -> string[]
    ObjectCreate(Box<Expr>, Option<Box<Expr>>), // Object.create(proto[, propertiesObject])
    ObjectFreeze(Box<Expr>),                    // Object.freeze(obj)
    ObjectSeal(Box<Expr>),                      // Object.seal(obj)
    ObjectPreventExtensions(Box<Expr>),         // Object.preventExtensions(obj)
    ObjectIsFrozen(Box<Expr>),                  // Object.isFrozen(obj)
    ObjectIsSealed(Box<Expr>),                  // Object.isSealed(obj)
    ObjectIsExtensible(Box<Expr>),              // Object.isExtensible(obj)
    ObjectGetPrototypeOf(Box<Expr>),            // Object.getPrototypeOf(obj)
    ObjectSetPrototypeOf(Box<Expr>, Box<Expr>), // Object.setPrototypeOf(obj, proto) -> obj
    ObjectDefineProperties(Box<Expr>, Box<Expr>), // Object.defineProperties(target, descriptors)
    ObjectGetOwnPropertySymbols(Box<Expr>),     // Object.getOwnPropertySymbols(obj) -> symbol[]

    // Symbol operations
    SymbolNew(Option<Box<Expr>>), // Symbol() / Symbol(description)
    SymbolFor(Box<Expr>),         // Symbol.for(key) -> registered symbol
    SymbolKeyFor(Box<Expr>),      // Symbol.keyFor(sym) -> key | undefined
    SymbolDescription(Box<Expr>), // sym.description
    /// RegExp.escape(str) -> escaped string (TC39 proposal, Node 24+)
    RegExpEscape(Box<Expr>),
    SymbolToString(Box<Expr>), // sym.toString()

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
    /// `JSON.rawJSON(text)` (#2900) -> raw-JSON wrapper object.
    JsonRawJson(Box<Expr>),
    /// `JSON.isRawJSON(value)` (#2900) -> boolean.
    JsonIsRawJson(Box<Expr>),

    // Math operations
    MathFloor(Box<Expr>),            // Math.floor(x) -> number
    MathCeil(Box<Expr>),             // Math.ceil(x) -> number
    MathRound(Box<Expr>),            // Math.round(x) -> number
    MathTrunc(Box<Expr>),            // Math.trunc(x) -> number
    MathSign(Box<Expr>),             // Math.sign(x) -> number
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
    MathF16round(Box<Expr>),         // Math.f16round(x) -> number
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
    /// `WebAssembly.compile(bytes)` -> Promise<WebAssembly.Module>
    WebAssemblyCompile(Box<Expr>),
    /// `new WebAssembly.Module(bytes)` -> WebAssembly.Module wrapper
    WebAssemblyModuleNew(Box<Expr>),
    /// `WebAssembly.Module.exports(module)` -> export descriptors
    WebAssemblyModuleExports(Box<Expr>),
    /// `WebAssembly.Module.imports(module)` -> import descriptors
    WebAssemblyModuleImports(Box<Expr>),
    /// `WebAssembly.Module.customSections(module, name)` -> ArrayBuffer[]
    WebAssemblyModuleCustomSections {
        module: Box<Expr>,
        name: Box<Expr>,
    },
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
    /// encoder.encodeInto(string, Uint8Array) -> { read, written }
    TextEncoderEncodeInto {
        source: Box<Expr>,
        dest: Box<Expr>,
    },
    /// new TextDecoder(label?, { fatal?, ignoreBOM? }) -> opaque handle.
    /// `label` is undefined for the no-arg form; `fatal`/`ignore_bom`
    /// default to `false`.
    TextDecoderNew {
        label: Box<Expr>,
        fatal: Box<Expr>,
        ignore_bom: Box<Expr>,
    },
    /// decoder.decode(buffer) -> string. `decoder` carries the encoding
    /// state (the lowered receiver handle); `input` is the bytes.
    TextDecoderDecode {
        decoder: Box<Expr>,
        input: Box<Expr>,
    },
    /// decoder.encoding / .fatal / .ignoreBOM property reads.
    TextDecoderEncoding(Box<Expr>),
    TextDecoderFatal(Box<Expr>),
    TextDecoderIgnoreBom(Box<Expr>),

    // URI encoding / decoding
    /// encodeURI(string) -> string
    EncodeURI(Box<Expr>),
    /// decodeURI(string) -> string
    DecodeURI(Box<Expr>),
    /// encodeURIComponent(string) -> string
    EncodeURIComponent(Box<Expr>),
    /// decodeURIComponent(string) -> string
    DecodeURIComponent(Box<Expr>),

    /// structuredClone(value[, options]) -> deep-cloned value
    StructuredClone {
        value: Box<Expr>,
        options: Box<Expr>,
    },
    /// #4141: link a freshly-built generator/async-generator instance object
    /// to the spec prototype chain. Emitted ONLY by `transform_generators`
    /// wrapping the `{next,return,throw}` iterator object it returns. At
    /// codegen uses the owning closure when available so the instance points at
    /// the same closure-cached `g.prototype` object exposed by property reads.
    /// The fallback runtime path interposes a fresh intermediate object whose
    /// own `[[Prototype]]` is `%Generator.prototype%` /
    /// `%AsyncGenerator.prototype%`, preserving the two-hop brand-checked
    /// prototype chain.
    /// Evaluates to `obj` unchanged. `is_async` selects the sync vs async
    /// generator prototype tower for the fallback path.
    LinkGeneratorPrototype {
        obj: Box<Expr>,
        is_async: bool,
    },
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

    /// #6709. Async-generator activation entry. Like `AsyncFirstCall`, but
    /// invokes the step closure with a caller-supplied `value` + `is_error`
    /// instead of `(undefined, false)`. Emitted by the async-generator
    /// lowering for each `gen.next(v)` / `gen.throw(e)` so the shared state
    /// machine resumes: a non-error call delivers `value` to `__sent`, an
    /// error call routes `value` as a throw at the current suspend state. The
    /// runtime helper (`js_async_generator_resume`) sets up the
    /// `CURRENT_STEP_CLOSURE` / `trap_next` TLS exactly like `AsyncFirstCall`.
    AsyncGenResume {
        step_closure: Box<Expr>,
        value: Box<Expr>,
        is_error: bool,
    },

    // Crypto operations
    CryptoRandomBytes(Box<Expr>), // crypto.randomBytes(size) -> string (hex)
    CryptoRandomUUID,             // crypto.randomUUID() -> string
    CryptoRandomUUIDv7,           // crypto.randomUUIDv7() -> string (RFC 9562 v7)
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
    /// `crypto.subtle.exportKey(format, key)` -> Promise<ArrayBuffer>
    WebCryptoExportKey {
        format: Box<Expr>,
        key: Box<Expr>,
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
    /// `crypto.subtle.deriveBits(algorithm, baseKey, length)` -> Promise<ArrayBuffer>
    WebCryptoDeriveBits {
        algorithm: Box<Expr>,
        base_key: Box<Expr>,
        length: Box<Expr>,
    },
    /// `crypto.subtle.deriveKey(algorithm, baseKey, derivedKeyAlgorithm, extractable, keyUsages)`
    /// -> Promise<CryptoKey>
    WebCryptoDeriveKey {
        algorithm: Box<Expr>,
        base_key: Box<Expr>,
        derived_key_algorithm: Box<Expr>,
        extractable: Box<Expr>,
        usages: Box<Expr>,
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
    OsPlatform,             // os.platform() -> string ("darwin", "linux", "win32")
    OsArch,                 // os.arch() -> string ("x64", "arm64", etc.)
    OsHostname,             // os.hostname() -> string
    OsHomedir,              // os.homedir() -> string
    OsTmpdir,               // os.tmpdir() -> string
    OsTotalmem,             // os.totalmem() -> number (bytes)
    OsFreemem,              // os.freemem() -> number (bytes)
    OsUptime,               // os.uptime() -> number (seconds)
    OsType,                 // os.type() -> string ("Darwin", "Linux", "Windows_NT")
    OsRelease,              // os.release() -> string
    OsCpus,                 // os.cpus() -> array of CPU info objects
    OsNetworkInterfaces,    // os.networkInterfaces() -> object
    OsUserInfo,             // os.userInfo() -> object
    OsUserInfoBuffer,       // os.userInfo({ encoding: "buffer" }) -> object
    OsEOL,                  // os.EOL -> string ("\n" or "\r\n")
    OsDevNull,              // os.devNull -> string
    OsAvailableParallelism, // os.availableParallelism() -> number
    OsEndianness,           // os.endianness() -> string ("LE" or "BE")
    OsLoadavg,              // os.loadavg() -> number[3]
    OsMachine,              // os.machine() -> string
    OsVersion,              // os.version() -> string

    // Buffer operations
    BufferFrom {
        // Buffer.from(data, encoding?) -> Buffer
        data: Box<Expr>,
        encoding: Option<Box<Expr>>,
    },
    BufferFromArrayBuffer {
        // Buffer.from(arrayBuffer, byteOffset, length?) -> Buffer
        data: Box<Expr>,
        byte_offset: Box<Expr>,
        length: Option<Box<Expr>>,
    },
    BufferAlloc {
        // Buffer.alloc(size, fill?, encoding?) -> Buffer
        size: Box<Expr>,
        fill: Option<Box<Expr>>,
        encoding: Option<Box<Expr>>,
    },
    BufferAllocUnsafe(Box<Expr>), // Buffer.allocUnsafe(size) -> Buffer
    BufferConcat(Box<Expr>),      // Buffer.concat(list) -> Buffer
    BufferConcatWithLength {
        // Buffer.concat(list, totalLength) -> Buffer
        list: Box<Expr>,
        total_length: Box<Expr>,
    },
    BufferIsBuffer(Box<Expr>),   // Buffer.isBuffer(obj) -> boolean
    BufferIsEncoding(Box<Expr>), // Buffer.isEncoding(encoding) -> boolean
    BufferByteLength {
        data: Box<Expr>,
        encoding: Option<Box<Expr>>,
    }, // Buffer.byteLength(value, encoding?) -> number
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

    /// Hidden Perry intrinsic: `__perry_native_arena_alloc(byteLength)`.
    NativeArenaAlloc(Box<Expr>),
    /// Hidden Perry intrinsic:
    /// `__perry_native_arena_view(owner, kind, byteOffset, length)`.
    NativeArenaView {
        owner: Box<Expr>,
        kind: u8,
        byte_offset: Box<Expr>,
        length: Box<Expr>,
    },
    /// Hidden Perry intrinsic:
    /// `__perry_native_pod_view(owner, byteOffset, count)`.
    NativePodView {
        owner: Box<Expr>,
        byte_offset: Box<Expr>,
        count: Box<Expr>,
        view_type: Option<Type>,
    },
    /// Compile-time POD layout constant: `sizeof<T>()`.
    PodLayoutSizeOf {
        ty: Type,
    },
    /// Compile-time POD layout constant: `alignof<T>()`.
    PodLayoutAlignOf {
        ty: Type,
    },
    /// Compile-time POD field offset constant: `offsetof<T>("field.path")`.
    PodLayoutOffsetOf {
        ty: Type,
        field_path: Vec<String>,
    },
    /// Hidden Perry intrinsic: `__perry_native_arena_dispose(owner)`.
    NativeArenaDispose(Box<Expr>),
    /// Public compile-time NativeMemory API:
    /// `NativeMemory.fillU32(view, value)`.
    NativeMemoryFillU32 {
        view: Box<Expr>,
        value: Box<Expr>,
    },
    /// Public compile-time NativeMemory API:
    /// `NativeMemory.copy(dst, src)`.
    NativeMemoryCopy {
        dst: Box<Expr>,
        src: Box<Expr>,
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
    ChildProcessFork {
        // fork(modulePath, args?, opts?) -> ChildProcess with an IPC channel (#1933)
        module: Box<Expr>,
        args: Option<Box<Expr>>,
        options: Option<Box<Expr>>,
    },
    ChildProcessExec {
        // exec(cmd, opts?, callback?) -> ChildProcess
        command: Box<Expr>,
        options: Option<Box<Expr>>,
        callback: Option<Box<Expr>>,
    },
    ChildProcessExecFile {
        // execFile(file, args?, opts?, callback?) -> ChildProcess
        file: Box<Expr>,
        args: Option<Box<Expr>>,
        options: Option<Box<Expr>>,
        callback: Option<Box<Expr>>,
    },
    ChildProcessExecFileSync {
        // execFileSync(file, args?, opts?) -> Buffer | string
        file: Box<Expr>,
        args: Option<Box<Expr>>,
        options: Option<Box<Expr>>,
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
        // Statically-extracted headers from an object *literal* whose keys are
        // all plain (non-computed) string/ident keys: `{ "k": v, ... }`.
        headers: Vec<(String, Expr)>,
        // A dynamically-built headers value (a variable, a spread literal, a
        // call like `Object.assign`/`new Headers`, etc.) that can only be
        // serialized at runtime. When `Some`, it takes precedence over the
        // static `headers` pairs above. See #4932.
        headers_dynamic: Option<Box<Expr>>,
        // The `init.signal` AbortSignal, when present, so the request can be
        // aborted (`controller.abort()` / `AbortSignal.timeout`). Lowered to a
        // `js_fetch_set_pending_signal` call emitted just before the fetch.
        signal: Option<Box<Expr>>,
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
        from_index: Option<Box<Expr>>,
    }, // arr.indexOf(value, fromIndex?) -> index
    ArrayLastIndexOf {
        array: Box<Expr>,
        value: Box<Expr>,
        from_index: Option<Box<Expr>>,
    }, // arr.lastIndexOf(value, fromIndex?) -> index
    ArrayIncludes {
        array: Box<Expr>,
        value: Box<Expr>,
        from_index: Option<Box<Expr>>,
    }, // arr.includes(value, fromIndex?) -> boolean
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
    ArrayReverseValue {
        receiver: Box<Expr>,
    }, // Array.prototype.reverse.call(receiver) -> same receiver
    ArrayCopyWithin {
        array_id: LocalId,
        target: Box<Expr>,
        start: Box<Expr>,
        end: Option<Box<Expr>>,
    }, // arr.copyWithin(target, start, end?) -> same array
    ArrayCopyWithinValue {
        receiver: Box<Expr>,
        target: Box<Expr>,
        start: Box<Expr>,
        end: Option<Box<Expr>>,
    }, // Array.prototype.copyWithin.call(arrayLike, target, start, end?) -> same receiver
    ArrayEntries(Box<Expr>), // arr.entries() -> Array<[index, value]> (eager materialization)
    ArrayKeys(Box<Expr>),    // arr.keys() -> Array<index>
    ArrayValues(Box<Expr>),  // arr.values() -> Array<value> (essentially clone)

    /// `Array.prototype.<method>.call/apply(receiver, ...args)` (and the
    /// bound-local form `const m = [].map; m.call(receiver, ...)`) dispatched
    /// generically over an *array-like* receiver per ECMA-262 §23.1.3 (#4597).
    /// Unlike the specialised `Array*` variants above — which require a genuine
    /// array receiver — this carries the receiver as a raw value so the runtime
    /// applies `ToObject` + `LengthOfArrayLike` + indexed `Get`/`HasProperty`,
    /// preserving receiver identity for the callback's 3rd argument.
    /// `method` is the resolved Array method name; `args` are the post-receiver
    /// positional arguments (already expanded from `.apply`).
    ArrayLikeMethod {
        method: String,
        receiver: Box<Expr>,
        args: Vec<Expr>,
    },

    // String methods
    StringSplit(Box<Expr>, Box<Expr>), // string.split(delimiter) -> string[]
    StringFromCharCode(Box<Expr>),     // String.fromCharCode(code) -> single-char string
    StringFromCharCodeSpread(Box<Expr>), // String.fromCharCode(...arrayLike) -> string
    StringFromCodePoint(Box<Expr>),    // String.fromCodePoint(code) -> string
    StringRaw {
        // Callable String.raw(callSite, ...substitutions) — the non-tagged
        // form. `call_site` is the `{ raw: [...] }` (array-like) object;
        // `substitutions` are the interpolated values. (#2789)
        call_site: Box<Expr>,
        substitutions: Vec<Expr>,
    },
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

    // `new Number(x)` / `new String(x)` / `new Boolean(x)` — wrap a
    // primitive in its boxed-object form. Mirrors the dedicated-variant
    // pattern used by `DateNew` / `MapNew` / `SetNew`. Codegen routes
    // each kind to its `js_boxed_*_new` runtime helper.
    BoxedPrimitiveNew {
        kind: BoxedPrimitiveKind,
        arg: Box<Expr>,
        // Whether an argument was syntactically given (`new String(x)`) vs.
        // omitted (`new String()`). `arg` alone can't disambiguate an omitted
        // argument from a present one that evaluates to `undefined` at
        // runtime (e.g. `new String(x)` where `x` holds `undefined`) — both
        // would otherwise collapse to the same NaN-boxed `undefined` value at
        // the `js_boxed_*_new` runtime boundary. Only `BoxedPrimitiveKind::
        // String` currently reads this (see `js_boxed_string_new`); Number/
        // Boolean disambiguate implicitly because their present-arg coercion
        // (`NumberCoerce`/`BooleanCoerce`) always produces a non-`undefined`
        // primitive.
        arg_present: bool,
    },

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

    // Date setters (UTC variants) — return the new timestamp. `args` carries
    // all call arguments (Node setters accept optional trailing components,
    // e.g. `setUTCHours(h, min?, sec?, ms?)`) — #2851.
    DateSetUtcFullYear {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetUtcMonth {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetUtcDate {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetUtcHours {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetUtcMinutes {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetUtcSeconds {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetUtcMilliseconds {
        date: Box<Expr>,
        args: Vec<Expr>,
    },

    // Date setters (local-time variants) — return the new timestamp (#1187)
    DateSetFullYear {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetMonth {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetDate {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetHours {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetMinutes {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetSeconds {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetMilliseconds {
        date: Box<Expr>,
        args: Vec<Expr>,
    },
    DateSetTime {
        date: Box<Expr>,
        args: Vec<Expr>,
    },

    // Date misc
    DateValueOf(Box<Expr>),      // date.valueOf() -> number (same as getTime)
    DateToString(Box<Expr>),     // date.toString() / String(date) -> full date string
    DateToDateString(Box<Expr>), // date.toDateString() -> string
    DateToTimeString(Box<Expr>), // date.toTimeString() -> string
    DateToUTCString(Box<Expr>),  // date.toUTCString() / toGMTString() -> string
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
    /// #2836: `new <Error-kind>(message, options)` where `options` is an
    /// arbitrary runtime value (variable, dynamic object, or literal) whose
    /// `cause` property is applied at runtime. `kind` is an `ERROR_KIND_*`
    /// discriminant so native subclasses (TypeError/RangeError/…) keep their
    /// `error_kind`. Covers base `Error` plus the four native subclasses with
    /// a non-literal-recognizable options argument.
    ErrorNewWithOptions {
        kind: u32,
        message: Box<Expr>,
        options: Box<Expr>,
    },
    /// new TypeError(message)
    TypeErrorNew(Box<Expr>),
    /// new RangeError(message)
    RangeErrorNew(Box<Expr>),
    /// new ReferenceError(message)
    ReferenceErrorNew(Box<Expr>),
    /// new SyntaxError(message)
    SyntaxErrorNew(Box<Expr>),
    /// new AggregateError(errors, message?, options?)
    ///
    /// #2838: `errors` is passed through as a raw runtime value (NOT
    /// pre-coerced to an array) so the runtime can consume Sets / strings /
    /// generators / any iterable and throw `TypeError` on non-iterables.
    /// #2836: `options` carries the optional `{ cause }` argument.
    AggregateErrorNew {
        errors: Box<Expr>,
        message: Box<Expr>,
        options: Option<Box<Expr>>,
    },

    // URL operations
    /// new URL(url) or new URL(url, base) -> URL object (stored as pointer)
    UrlNew {
        url: Box<Expr>,
        base: Option<Box<Expr>>,
    },
    /// new URLPattern(input?, base?) -> URLPattern object
    UrlPatternNew {
        input: Box<Expr>,
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
    /// URL.canParse(input, base) -> boolean.
    UrlCanParseWithBase {
        input: Box<Expr>,
        base: Box<Expr>,
    },
    /// URL.parse(input) -> URL | null. Issue #650: non-throwing variant
    /// of `new URL()` added in Node 22. Returns null when parsing fails.
    UrlParse(Box<Expr>),
    /// URL.parse(input, base) -> URL | null.
    UrlParseWithBase {
        input: Box<Expr>,
        base: Box<Expr>,
    },
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
    /// `urlInstance.protocol = value` — updates protocol and rebuilds href.
    UrlSetProtocol {
        url: Box<Expr>,
        value: Box<Expr>,
    },
    /// `urlInstance.hostname = value` — updates hostname + reconstructs host
    /// (`hostname[:port]`) and rebuilds href.
    UrlSetHostname {
        url: Box<Expr>,
        value: Box<Expr>,
    },
    /// `urlInstance.port = value` — updates port + reconstructs host and
    /// rebuilds href. Empty/default port collapses host back to hostname.
    UrlSetPort {
        url: Box<Expr>,
        value: Box<Expr>,
    },
    /// `urlInstance.username = value` — updates userinfo and rebuilds href.
    UrlSetUsername {
        url: Box<Expr>,
        value: Box<Expr>,
    },
    /// `urlInstance.password = value` — updates userinfo and rebuilds href.
    UrlSetPassword {
        url: Box<Expr>,
        value: Box<Expr>,
    },
    /// `urlInstance.href = value` — reparses the full URL or throws.
    UrlSetHref {
        url: Box<Expr>,
        value: Box<Expr>,
    },

    // URLSearchParams operations
    /// new URLSearchParams(init?)
    UrlSearchParamsNew(Option<Box<Expr>>),
    /// URLSearchParams method call missing required arguments.
    UrlSearchParamsMissingArgs {
        params: Box<Expr>,
        args: Vec<Expr>,
        name_and_value: bool,
    },
    /// params.get(name) -> string | null
    UrlSearchParamsGet {
        params: Box<Expr>,
        name: Box<Expr>,
    },
    /// params.has(name) -> boolean. Node 19+ also accepts an optional
    /// `value` argument matching only when both the name AND value match.
    UrlSearchParamsHas {
        params: Box<Expr>,
        name: Box<Expr>,
        value: Option<Box<Expr>>,
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
    /// params.delete(name). Node 19+ also accepts an optional `value`
    /// argument deleting only entries matching both name AND value.
    UrlSearchParamsDelete {
        params: Box<Expr>,
        name: Box<Expr>,
        value: Option<Box<Expr>>,
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
    /// params.keys() -> string[]
    UrlSearchParamsKeys(Box<Expr>),
    /// params.values() -> string[]
    UrlSearchParamsValues(Box<Expr>),
    /// params.sort() -> undefined (mutates in place)
    UrlSearchParamsSort(Box<Expr>),
    /// params.forEach(callback[, thisArg]) -> undefined
    UrlSearchParamsForEach {
        params: Box<Expr>,
        callback: Box<Expr>,
        this_arg: Option<Box<Expr>>,
    },

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
        /// Whether this closure captures `new.target` from the enclosing scope.
        captures_new_target: bool,
        /// The enclosing class name if this closure captures `this` (for field access during codegen)
        enclosing_class: Option<String>,
        /// Whether this closure came from an arrow function expression.
        is_arrow: bool,
        /// Whether this is an async closure
        is_async: bool,
        /// Whether this is a generator closure (a `function*(){}` expression).
        /// Set by `lower_fn_expr`; the generator transform rewrites the body of
        /// such closures (reusing the captures-aware generator transform) so
        /// calling the closure returns a `{next,return,throw}` generator, then
        /// clears the flag. Refs #321 (effect's `Effect.gen(function*(){...})`).
        is_generator: bool,
        /// Whether this closure body is strict mode code.
        is_strict: bool,
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
        /// `true` when this came from the *function-call* form `RegExp(x)`
        /// (not `new RegExp(x)`). Per ECMA-262 22.2.4.1, calling `RegExp` as a
        /// function with a RegExp `pattern` and `undefined` `flags` returns the
        /// argument unchanged (object identity) rather than constructing a copy
        /// — so the call form lowers to `js_regexp_construct_call` (identity
        /// shortcut) while `new` keeps `js_regexp_construct` (always a fresh
        /// object). See #5586.
        is_call: bool,
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
    /// string.matchAll(pattern) -> RegExp String Iterator
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
    /// `for (key in obj)` enumeration keys -> string[]
    /// Like `ObjectKeys` but follows ECMA-262 EnumerateObjectProperties:
    /// null/undefined enumerate nothing (no throw) and inherited enumerable
    /// string keys on the prototype chain are included (deduplicated).
    ForInKeys(Box<Expr>),
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
    /// Map.groupBy(items, keyFn) -> Map<key, items[]>
    /// Like ObjectGroupBy but the result is a `Map` and callback keys are
    /// used directly (no string coercion). Lowered through `js_map_group_by`.
    MapGroupBy {
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
    /// Array.prototype generic receiver materialization preserving absent keys as holes.
    ArrayFromArrayLikeHoley(Box<Expr>),

    /// `Iterator.from(iterable)` (#2874) — wrap any iterable/iterator in a lazy
    /// iterator-helper object exposing `.map`/`.filter`/`.take`/`.drop`/
    /// `.flatMap`/`.toArray`/`.forEach`/`.reduce`/`.some`/`.every`/`.find`.
    /// The helper methods themselves dispatch at runtime through
    /// `js_native_call_method` (no dedicated HIR variant).
    IteratorFrom(Box<Expr>),

    /// Tagged-template strings literal — codegen builds the cooked-strings
    /// array AND a parallel raw-strings array, then asks the runtime for the
    /// cached frozen template object for this call site. The raw entries are
    /// always known at compile time (each quasi's `.raw` text), so they're
    /// stored as `String` rather than `Expr`. Used by `lower_tagged_tpl` for
    /// the non-`String.raw` fast-path tag-function call.
    TaggedTemplateStrings {
        site_id: u64,
        cooked: Vec<Expr>,
        raw: Vec<String>,
    },

    /// `strings.raw` on a tagged-template strings array — looks up the
    /// registered raw-strings array via `js_template_raw`. Returns
    /// undefined for non-tagged-template receivers (matches the JS
    /// semantics `[].raw === undefined`).
    TemplateRaw(Box<Expr>),
    IteratorToArray(Box<Expr>), // collect iterator (.next() loop) into array
    /// #1831: resolve the iterator of a `yield*` operand —
    /// `operand[Symbol.iterator]()` when iterable, else the operand itself (a
    /// generator object already *is* its iterator). Lowers to `js_get_iterator`.
    GetIterator(Box<Expr>),
    /// Resolve the iterator for generic `for await...of`: use
    /// `operand[Symbol.asyncIterator]()` when present, otherwise wrap the
    /// synchronous iterator from `operand[Symbol.iterator]()` in Perry's
    /// AsyncFromSyncIterator adapter.
    GetAsyncIterator(Box<Expr>),
    /// #321: materialize an UNTYPED `for...of` receiver into a plain Array
    /// by inspecting its runtime kind. The `for...of` desugar uses an
    /// index loop (`for (i=0; i<arr.length; i++) item = arr[i]`); when the
    /// receiver's static type can't be proven (an `any`-typed Map/Set, an
    /// untyped JS-source value), this routes through `js_for_of_to_array`
    /// so a Map yields `[k,v]` pairs, a Set yields values, an Array is
    /// returned unchanged, a string yields code-point chars, and anything
    /// else drives its `[Symbol.iterator]`. Without it the index loop read
    /// `.length` off a raw Map/Set handle (→ 0) and iterated zero times.
    ForOfToArray(Box<Expr>),
    /// Materialize an untyped `for await...of` receiver into a plain Array.
    /// This routes through the async-iterator protocol first, then falls back
    /// to the existing array/array-like behavior used by `Array.fromAsync`.
    /// The lowering wraps this in `Await` before the index loop reads it.
    ForAwaitToArray(Box<Expr>),
    /// Array.from(iterable, mapFn, thisArg?) -> Array
    /// Creates a new array by applying mapFn to each element of the iterable.
    /// `this_arg` (#2773) binds `this` inside a non-arrow mapFn.
    ArrayFromMapped {
        iterable: Box<Expr>,
        map_fn: Box<Expr>,
        this_arg: Option<Box<Expr>>,
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
    /// `Object(value)` plain-call coercion (#3149). Nullish/primitive → a fresh
    /// `{}`; an existing object/array passes through unchanged.
    ObjectCoerce(Box<Expr>),
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
        /// #2766: optional `receiver` argument (the `this` binding for accessor
        /// getters). Lowering supplies `target` when the call omits it.
        receiver: Box<Expr>,
    },
    ReflectSet {
        target: Box<Expr>,
        key: Box<Expr>,
        value: Box<Expr>,
        /// Optional `receiver` argument (4th): the object actually written
        /// when the target's own/inherited descriptor allows it (observable
        /// for Integer-Indexed exotic targets). Lowering supplies `target`
        /// when the call omits it.
        receiver: Box<Expr>,
    },
    /// Assignment PutValue for property references. Evaluates target/key/value
    /// in source order, performs ordinary [[Set]] with an explicit receiver,
    /// returns the RHS value, and throws when `strict` is true and [[Set]]
    /// reports false.
    PutValueSet {
        target: Box<Expr>,
        key: Box<Expr>,
        value: Box<Expr>,
        receiver: Box<Expr>,
        strict: bool,
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
        /// Optional `newTarget` (the 3rd `Reflect.construct` argument). When the
        /// call omits it, this lowers to `Expr::Undefined` and the runtime
        /// defaults `newTarget` to the target/proxy itself.
        new_target: Box<Expr>,
    },
    ReflectDefineProperty {
        target: Box<Expr>,
        key: Box<Expr>,
        descriptor: Box<Expr>,
    },
    ReflectGetOwnPropertyDescriptor {
        target: Box<Expr>,
        key: Box<Expr>,
    },
    ReflectGetPrototypeOf(Box<Expr>),
    /// #2761: `Reflect.setPrototypeOf(target, proto)` — returns a boolean
    /// (false when rejected), unlike `Object.setPrototypeOf` which returns the
    /// object. Lowered separately so it can report failure / throw on bad args.
    ReflectSetPrototypeOf {
        target: Box<Expr>,
        proto: Box<Expr>,
    },
    // #2762: Reflect.isExtensible / Reflect.preventExtensions have
    // Reflect-specific semantics (boolean result, TypeError on non-object)
    // distinct from the Object.* helpers, so they use dedicated variants.
    ReflectIsExtensible(Box<Expr>),
    ReflectPreventExtensions(Box<Expr>),
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
        /// Byte offset (`span.lo.0`) of the `import(...)` call in its module's
        /// source, captured at lowering time. Used by the driver to resolve a
        /// `file:line` for the #5230 deferred-site notice (HIR `Expr` carries no
        /// span otherwise). `0` when unknown.
        byte_offset: u32,
        /// #5230: when `Some(msg)`, the path argument was non-resolvable
        /// (runtime-computed) and this site was *deferred* (the default,
        /// non-strict policy — the analog of #5206's eval deferral). Codegen
        /// lowers it to a rejected `Promise` carrying an `Error(msg)`, so
        /// `await import(spec)` throws a descriptive error *only if reached*
        /// rather than failing the whole build. `None` is the normal case
        /// (`paths` resolved to a finite set). In strict mode such a site is a
        /// compile error instead and never produces a node with this set.
        deferred_error: Option<String>,
        /// #5389 Tier 2: `true` when this node came from a **synchronous**
        /// CommonJS `require(expr)` in a compiled external/compilePackages
        /// module rather than an ESM `import(expr)`. Codegen returns the target
        /// namespace value directly (no `Promise` wrap) and uses the ambient
        /// createRequire-backed `require` as the no-match / unresolved
        /// fallthrough (builtin-or-throw) instead of a rejected promise. The
        /// compile-time path resolution (`collect_modules`) is identical for
        /// both — only the dispatch shape differs. `false` for ESM `import()`.
        synchronous: bool,
    },
    /// Compile-time-resolved `new Worker(filename, options?)` from
    /// `node:worker_threads`. The filename expression follows the same
    /// deterministic subset as dynamic `import()`: lowering leaves `paths`
    /// empty, and the module collector resolves it before codegen.
    WorkerNew {
        paths: Vec<String>,
        filename: Box<Expr>,
        options: Option<Box<Expr>>,
        /// `true` when the Worker options carry `eval: true` — i.e. the first
        /// argument is JS SOURCE, not a filename (Node eval-mode Worker, used by
        /// e.g. `@perryts/threads`). Parsed from the options object at lowering.
        is_eval: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStdinLifecycleMethod {
    Pause,
    Resume,
    Unref,
    Ref,
    Destroy,
}

/// Which primitive the `new X(...)` form is wrapping. Used by
/// `Expr::BoxedPrimitiveNew`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoxedPrimitiveKind {
    Number,
    String,
    Boolean,
}

/// `path.win32.<method>` dispatch — one variant per supported method.
/// See `Expr::PathWin32`. Used by issue #1162.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathWin32Method {
    Dirname,
    Basename,
    BasenameExt,
    Extname,
    IsAbsolute,
    Normalize,
    Parse,
    Format,
    Relative,
    Resolve,
    ResolveJoin,
    ToNamespacedPath,
    MatchesGlob,
}
