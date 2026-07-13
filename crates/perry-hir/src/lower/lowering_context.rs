//! `LoweringContext` struct definition.
//!
//! Extracted from `lower/mod.rs` so the entry-point file stays under the
//! 2,000-LOC soft cap. The `impl LoweringContext { ... }` blocks live in
//! `lower/context.rs` (this file holds *only* the struct shape). All
//! field visibility stays `pub(crate)`; downstream code keeps reaching
//! the struct via `crate::lower::LoweringContext`.

use perry_types::{FuncId, GlobalId, LocalId, Type, TypeParam};
use std::collections::{HashMap, HashSet};

use crate::ir::*;
use crate::ClassAccessorNames;

#[derive(Debug, Clone, Copy)]
pub(crate) struct WithEnvFrame {
    pub(crate) local_id: LocalId,
    pub(crate) local_mark: usize,
}

/// Kind of a private class element, for the read/write legality checks that
/// `obj.#name` access performs (in addition to the brand check). The numeric
/// values are the wire codes passed to the `js_private_guard` runtime helper —
/// keep them in sync with `crates/perry-runtime/src/object/field_get_set.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrivKind {
    Field = 0,
    Method = 1,
    Get = 2,
    Set = 3,
    GetSet = 4,
}

/// One private element declared by a class: its kind and whether it is static.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PrivMember {
    pub(crate) kind: PrivKind,
    pub(crate) is_static: bool,
}

/// Private-name scope for one class body. `members` is keyed by the
/// `#name` string (with the leading `#`).
#[derive(Debug, Clone)]
pub(crate) struct PrivateScope {
    pub(crate) class_name: String,
    /// The declaring class's unique HIR id. Carried alongside `class_name`
    /// because the name alone is ambiguous: minified bundles reuse class names,
    /// and codegen's `class_ids` name→id map is last-writer-wins, so resolving a
    /// private member's declaring class by name can bind to the wrong same-named
    /// class and make the runtime brand check reject a legal `this.#x` access.
    pub(crate) class_id: u32,
    pub(crate) members: HashMap<String, PrivMember>,
}

/// A `function Mixin(B) { return class [Name] extends B { … } }` recorded by
/// `pre_scan_mixin_functions`, so a `const M = Mixin(Base)` call site can
/// synthesize a real class extending the concrete base.
#[derive(Debug, Clone)]
pub(crate) struct MixinFn {
    /// The mixin function's single parameter — the name the returned class
    /// `extends`.
    pub(crate) param_name: String,
    /// The returned class EXPRESSION's own name, if it has one (`return class
    /// Named extends B {}`). `None` for the anonymous form, whose `.name` is
    /// the empty string per spec — a directly-returned class expression gets
    /// no NamedEvaluation from the call site's binding.
    pub(crate) class_expr_name: Option<String>,
    /// The returned class's AST, copied verbatim; the call site rewrites its
    /// `extends` clause to the concrete base.
    pub(crate) class_ast: Box<swc_ecma_ast::Class>,
}

pub struct LoweringContext {
    /// Counter for generating unique local IDs
    pub(crate) next_local_id: LocalId,
    /// Counter for generating unique global IDs
    // #854: initialized in `new` but not yet read by the lowerer (globals are
    // allocated through a different path today). Kept for the ID-counter set.
    #[allow(dead_code)]
    pub(crate) next_global_id: GlobalId,
    /// Counter for generating unique function IDs
    pub(crate) next_func_id: FuncId,
    /// Counter for generating unique class IDs
    pub(crate) next_class_id: ClassId,
    /// Counter for generating unique enum IDs
    pub(crate) next_enum_id: EnumId,
    /// Counter for generating unique interface IDs
    pub(crate) next_interface_id: InterfaceId,
    /// Counter for generating unique type alias IDs
    pub(crate) next_type_alias_id: TypeAliasId,
    /// Deterministic per-module salt for tagged-template call-site cache keys.
    pub(crate) tagged_template_site_salt: u64,
    /// Counter for generating unique tagged-template call-site IDs.
    pub(crate) next_tagged_template_site_id: u32,
    /// Current scope's local variables: name -> (id, type), with an O(1)
    /// name→position index for innermost-binding resolution (#5267).
    pub(crate) locals: crate::lower::Locals,
    /// LocalIds that represent immutable bindings (`const`, imports, and
    /// other lexical bindings that must throw when assigned).
    pub(crate) immutable_locals: HashSet<LocalId>,
    /// Global variables: name -> (id, type)
    // #854: initialized in `new` but currently unread (globals tracked
    // elsewhere). Retained alongside `next_global_id` for the global table.
    #[allow(dead_code)]
    pub(crate) globals: Vec<(String, GlobalId, Type)>,
    /// Functions: name -> id
    pub(crate) functions: Vec<(String, FuncId)>,
    /// Function parameter defaults: func_id -> (defaults, param_local_ids)
    /// Per-function param-default info used by the call-site padding pass:
    /// `(func_id, [Option<default> per param], [LocalId per param], Option<rest_param_index>, has_synthetic_arguments)`.
    /// The rest-param index (if any) is the position of `...rest`; the padding
    /// loop must stop before it because rest params get bundled at runtime
    /// from trailing positional args, not filled with `undefined`. Missing
    /// fixed params are padded with `undefined`; the callee body applies
    /// default expressions inside the function boundary.
    /// `has_synthetic_arguments` is true when the trailing rest param was
    /// inserted by `append_synthetic_arguments_param` because the body
    /// references the magic `arguments` identifier — in that case the call
    /// site must NOT pad missing fixed-param slots with `undefined`, because
    /// the codegen synth-args bundle path uses `args.len()` to size the
    /// runtime `arguments` array. Padding inflates `arguments.length` to
    /// the declared fixed-param count regardless of how many args the
    /// caller actually passed (issue #1069).
    pub(crate) func_defaults: Vec<(FuncId, Vec<Option<Expr>>, Vec<LocalId>, Option<usize>, bool)>,
    /// Classes: name -> id
    pub(crate) classes: Vec<(String, ClassId)>,
    /// Static members of classes: class_name -> (static_field_names, static_method_names)
    pub(crate) class_statics: Vec<(String, Vec<String>, Vec<String>)>,
    /// Instance field names per class: class_name -> list of DECLARED field names (from
    /// ClassProp and parameter properties, NOT inferred from constructor body `this.x = ...`).
    /// Used by the "infer fields from ctor body" pass to skip fields inherited from parents,
    /// avoiding the creation of shadow fields that cause later index shift bugs after
    /// inheritance resolution in codegen.
    pub(crate) class_field_names: Vec<(String, Vec<String>)>,
    /// Issue #665 (sixth pass): per-class set of getter and setter property names.
    /// Used by the "infer fields from ctor body `this.x = ...`" pass to avoid
    /// mis-categorising a setter assignment as an own data field — the
    /// rate-limiter-flexible `set points(v)` / `this.points = opts.points`
    /// shape, where the bare-`this` field-detection block in
    /// `lower_class_decl` would otherwise allocate an `Object.keys`-visible
    /// `points` slot that shadows the inherited getter at runtime.
    /// Populated alongside `register_class_field_names`; looked up via
    /// `lookup_class_accessor_names` and walked across the parent chain when
    /// processing a subclass's ctor body.
    pub(crate) class_accessor_names: Vec<(String, ClassAccessorNames)>,
    /// Issue #562: class name → `(module, class)` tuple from
    /// `native_extends`. Populated when lowering each class, consumed by
    /// `destructuring.rs` to register `let x = new SubclassOfStream()`
    /// locals under the parent stream module so subsequent
    /// `x.pipeTo(...)` / `x.getWriter()` etc. dispatch through the
    /// streams arms in `lower_call.rs`.
    pub(crate) class_native_extends: Vec<(String, String, String)>,
    /// Issue #302 (v0.5.388): instance field types per class so the
    /// for-of arm can detect `for (const [k, v] of this.someMap)` —
    /// the iterable is an `ast::Expr::Member { obj: This, prop: "someMap" }`,
    /// not an `ast::Expr::Ident`, so the existing `lookup_local_type`
    /// path doesn't apply. Parallel to `class_field_names` but stores
    /// `(field_name, declared_type)` pairs. Populated by
    /// `register_class_field_types` next to `register_class_field_names`.
    pub(crate) class_field_types: Vec<(String, Vec<(String, Type)>)>,
    /// Enums: name -> (id, members with values)
    pub(crate) enums: Vec<(String, EnumId, Vec<(String, EnumValue)>)>,
    /// Interfaces: name -> id
    pub(crate) interfaces: Vec<(String, InterfaceId)>,
    /// Type aliases: name -> (id, type_params, aliased_type)
    pub(crate) type_aliases: Vec<(String, TypeAliasId, Vec<TypeParam>, Type)>,
    /// Issue #179 typed-parse: interface name → field names in AST
    /// source order. Populated alongside `interfaces` during
    /// `lower_interface_decl`. `ObjectType::properties` is a HashMap
    /// that loses source order; this side table preserves it so
    /// `JSON.parse<Item[]>` codegen can emit a shape hint whose order
    /// matches typical `JSON.stringify` output (source order ≈
    /// insertion order ≈ what we see on the wire). Lost order would
    /// still be correct, just not fast-path friendly.
    pub(crate) interface_source_keys: std::collections::HashMap<String, Vec<String>>,
    /// Issue #179 typed-parse: interface name → resolved `ObjectType`.
    /// `resolve_typed_parse_ty` uses this so `JSON.parse<Item[]>`
    /// lowers to `Array<Object{fields}>` instead of `Array<Named("Item")>`.
    /// Without this, codegen sees only `Named` and can't extract the
    /// shape, so the specialized parse path never fires.
    pub(crate) interface_object_types: std::collections::HashMap<String, perry_types::ObjectType>,
    /// Imported functions: local_name -> original_name (the exported name in the source module)
    pub(crate) imported_functions: Vec<(String, String)>,
    /// Built-in named imports: local_name -> (module_name, exported_name).
    /// Kept separate from `imported_functions`, whose identity mapping is part
    /// of cross-module codegen symbol disambiguation.
    pub(crate) builtin_named_imports: Vec<(String, String, String)>,
    /// Native module imports: local_name -> (module_name, method_name)
    /// For namespace imports (import * as x), method_name is None
    /// For named imports (import { v4 as uuid }), method_name is Some("v4")
    pub(crate) native_modules: Vec<(String, String, Option<String>)>,
    /// Built-in module aliases from require(): local_name -> module_name (e.g., "myFs" -> "fs")
    pub(crate) builtin_module_aliases: Vec<(String, String)>,
    /// Stack of type parameter scopes (for nested generics)
    pub(crate) type_param_scopes: Vec<HashSet<String>>,
    /// Stack of type parameter CONSTRAINTS, paired with `type_param_scopes`.
    /// `type_param_constraints[i]` maps type-parameter name → its declared
    /// upper-bound constraint (`<T extends string>` → `String`). Used to
    /// resolve a `Named(T)`/`TypeVar(T)` reference to the *runtime* type
    /// the value must satisfy, so callers and the body see e.g.
    /// `Type::String` instead of `Type::Named("T")`.
    ///
    /// Why this matters at lowering (not codegen): perry monomorphizes
    /// `FuncRef`-targeted generic calls (issue #321 scaffolding) but does
    /// NOT monomorphize closures/arrow functions and does NOT monomorphize
    /// generic functions reached through a function-typed local. The
    /// emitted body of such a function therefore lowers `self[0]` (for
    /// `<T extends string>(self: T)`) with no string-typing on `self`,
    /// and codegen falls through to `js_object_get_index_polymorphic`
    /// which reads a `StringHeader*` as `ArrayHeader*` and returns the
    /// header bytes as a subnormal f64 — surfacing as the `1.5E-323oo`
    /// pattern in effect's `Str.capitalize`/`Capitalize<T>` utilities.
    /// Resolving the constraint at the `TsTypeRef` site for `T` makes the
    /// param type `String` everywhere downstream, so the IndexGet/Member
    /// fast paths fire.
    pub(crate) type_param_constraints: Vec<HashMap<String, Type>>,
    /// Native class instances: local_name -> (module_name, class_name)
    /// Tracks variables that hold instances of native module classes (e.g., EventEmitter)
    pub(crate) native_instances: Vec<(String, String, String)>,
    /// Cross-function native-instance hints: (fn_ident_span_lo, param_index) ->
    /// (module_name, class_name). Populated by `pre_scan_cross_fn_native_params`
    /// BEFORE any function body is lowered, so `lower_fn_decl` can tag an
    /// otherwise-untyped parameter as a native instance when a known native
    /// handle (e.g. the `("ws","Client")` upgrade `wsId`) is passed across the
    /// call boundary into it. Without this, `wsId.send(...)` inside a helper
    /// reached as `handleConnection(req, wsId)` lowers to a generic (silent
    /// no-op) dispatch instead of `js_ws_send_client_i64`.
    ///
    /// Keyed by the function declaration's identifier span (its `lo` byte
    /// offset) — a stable AST identity — rather than its name, so a hint seeded
    /// for one declaration never leaks onto an unrelated same-named (shadowed or
    /// redeclared) function.
    pub(crate) param_native_hints: HashMap<(u32, usize), (String, String)>,
    /// True while lowering code governed by ECMAScript strict mode.
    pub(crate) current_strict: bool,
    /// #1483: type-only perry/ui widget import aliases — local_name ->
    /// canonical widget name. `import { type Canvas as CanvasType }` records
    /// `CanvasType -> Canvas` so a `canvas: CanvasType` parameter can be
    /// tagged as a perry/ui native instance (handle-based method dispatch),
    /// exactly like a `canvas: Canvas` param or a `const canvas = Canvas(...)`
    /// local. Type-only native specifiers are otherwise dropped at import.
    pub(crate) ui_widget_type_aliases: HashMap<String, String>,
    /// Current class being lowered (for arrow function `this` capture)
    pub(crate) current_class: Option<String>,
    /// Source-level inner name of the class currently being lowered — the
    /// binding visible *inside* the class body (`class C {...}` -> `C`,
    /// `const K = class Named {...}` -> `Named`). Unlike `current_class`
    /// (which for a class expression may be a synthetic dedup key), this is
    /// the identifier the user writes. Assigning to it inside the body is a
    /// `const`-binding violation and must throw a TypeError.
    pub(crate) current_class_inner_name: Option<String>,
    /// Set by a class-EXPRESSION caller to the source ident of the class
    /// about to be lowered, so `lower_class_from_ast` can record the inner
    /// binding name (the synthetic dedup key it receives is not the
    /// user-visible name). Consumed (taken) at the start of lowering.
    pub(crate) pending_class_inner_name: Option<String>,
    /// True while lowering a static class member body.
    pub(crate) current_class_member_is_static: bool,
    /// Lexical stack of private-name scopes — one entry per enclosing class
    /// body, innermost last. Each access of `obj.#name` resolves `#name` to
    /// the nearest enclosing class that DECLARES it, yielding the declaring
    /// class (for the brand check) and the member kind (for the
    /// read-on-setter-only / write-on-method etc. TypeError checks). This is
    /// kept on the context (not derived from `current_class`) so it survives
    /// nested plain-function / arrow lowering — a private name referenced from
    /// an inner function still lexically resolves to its declaring class.
    pub(crate) private_scopes: Vec<PrivateScope>,
    /// Home-object local for object-literal methods while their bodies are
    /// lowered. Used to preserve `super` in object methods.
    pub(crate) object_super_home_stack: Vec<LocalId>,
    /// Extern function types: name -> (param_types, return_type)
    /// Stores type information for declare function statements (FFI)
    pub(crate) extern_func_types: Vec<(String, Vec<Type>, Type)>,
    /// Source file path (for import.meta.url)
    pub(crate) source_file_path: String,
    /// Variables that hold closures or other values needing cross-module export globals
    /// (arrow functions, object literals, call expressions, arrays, new expressions)
    // #854: initialized in `new` but not yet read on this lowering path.
    #[allow(dead_code)]
    pub(crate) exportable_object_vars: HashSet<String>,
    /// Functions created during expression lowering (e.g., object literal methods)
    /// These are flushed to the module after the enclosing statement is lowered.
    pub(crate) pending_functions: Vec<Function>,
    /// Issue #2076: display-name overrides keyed by FuncId. Populated for
    /// named function expressions (own ident) and object-literal methods
    /// (static property key); flushed into `Module.closure_display_names`
    /// alongside `pending_functions`.
    pub(crate) closure_display_names: HashMap<FuncId, String>,
    /// #5592: class `.name` overrides keyed by ClassId. Populated when a
    /// class expression's HIR registration key is uniquified away from its
    /// JS name (two anonymous class expressions on the same binding); flushed
    /// into `Module.class_display_names`. See that field's docs.
    pub(crate) class_display_names: HashMap<ClassId, String>,
    /// Per-generator count of leading parameter-prologue statements (default
    /// guards + destructuring binding stmts) prepended to the body. Flushed
    /// into `Module.gen_param_prologue_len`; the generator transform uses it to
    /// run param binding synchronously at call time. See that field's docs.
    pub(crate) gen_param_prologue_len: HashMap<FuncId, usize>,
    /// Test262 assignment name inference: when lowering `lhs = rhs`, a bare
    /// identifier lhs can provide the `NamedEvaluation` name for an anonymous
    /// function/class rhs. This slot is set only while lowering that rhs.
    pub(crate) assignment_inferred_name: Option<String>,
    /// Binding names whose class registration was created BY the binding's
    /// own class-expression init (`const E2 = class extends Event {}` — the
    /// anonymous expression claimed the inferred binding name as its
    /// registration key, and `var K = class Inner {}` registers under the
    /// bind name too). At a `new <name>()` site, such a name's local provably
    /// holds that same class, so the static construct path (with its exact
    /// builtin-parent handling) is correct; any OTHER in-scope local shadows
    /// whatever same-named class exists and must construct dynamically.
    pub(crate) inferred_class_bindings: std::collections::HashSet<String>,
    /// #4101: original source text keyed by FuncId, captured by slicing the
    /// module source against each function's AST span at lowering time.
    /// Flushed into `Module.closure_source_text` alongside `pending_functions`.
    pub(crate) closure_source_text: HashMap<FuncId, String>,
    /// Functions that return native module instances: func_name -> (module_name, class_name)
    /// Tracks user-defined functions whose return type annotation is a native module type
    /// (e.g., initializePool(): mysql.Pool -> ("mysql2/promise", "Pool"))
    pub(crate) func_return_native_instances: Vec<(String, String, String)>,
    /// Classes created during expression lowering (e.g., class expressions in `new (class extends X {})()`)
    /// These are flushed to the module after the enclosing statement is lowered.
    pub(crate) pending_classes: Vec<Class>,
    /// Function return types: func_name -> return_type
    /// Tracks return types of user-defined functions for call-site type inference
    pub(crate) func_return_types: Vec<(String, Type)>,
    /// Resolved types from external type checker (tsgo): byte_position -> Type
    /// Populated before lowering when --type-check is enabled
    pub resolved_types: Option<std::collections::HashMap<u32, Type>>,
    /// Module-level variable names pre-registered in the forward-declaration pass.
    /// Used to avoid duplicate define_local calls when the actual declaration is lowered.
    pub(crate) pre_registered_module_vars: HashSet<String>,
    /// Subset of `pre_registered_module_vars` that came from syntactic `var`
    /// declarations. Sloppy assignments before a later `var` need an early
    /// backing slot; `let`/`const` should not use that path.
    pub(crate) pre_registered_module_var_decls: HashSet<String>,
    /// LocalIds that are defined at module top level (outside any function or
    /// block). Closure `captures` referencing these IDs are filtered out at
    /// lowering time because codegen loads module-level bindings from their
    /// global data slot inside the closure body — passing them via the
    /// capture-slot would race with self-referential `const f = () => f(...)`
    /// and double-book state shared between sibling closures.
    pub(crate) module_level_ids: HashSet<LocalId>,
    /// Sloppy assignments to unresolvable identifiers create properties on
    /// the global object. We model the subset Perry can compile by minting a
    /// module-level LocalId plus a synthetic top-level `var` slot after module
    /// lowering, so closures and later top-level reads share storage.
    pub(crate) sloppy_implicit_globals: Vec<(String, LocalId)>,
    pub(crate) sloppy_implicit_global_ids: HashSet<LocalId>,
    /// Sloppy implicit globals minted as `with`-set FALLBACKS. Whether the
    /// binding ever materialises is a runtime question (the with-env may own
    /// the property and take the write), so these locals start as a HOLE
    /// sentinel and bare reads route through `js_with_implicit_read` which
    /// throws ReferenceError while unset. id → identifier name.
    pub(crate) with_sloppy_implicit_ids: std::collections::HashMap<LocalId, String>,
    /// (id, name) pairs whose sentinel-init `Stmt::Let` still needs to be
    /// emitted ahead of the with statement currently being lowered.
    pub(crate) pending_with_implicit_inits: Vec<(LocalId, String)>,
    /// Current function/closure nesting depth (`enter_scope` bumps this,
    /// `exit_scope` decrements). 0 == still at module top level.
    pub(crate) scope_depth: usize,
    /// Stack of local-vector marks for active function/closure/catch scopes.
    /// Function-body var prebinding uses the top mark to distinguish
    /// parameters/current-scope locals from outer captures with the same name.
    pub(crate) scope_local_marks: Vec<usize>,
    /// #wall5: per-scope marks into `module_shadow_stack`, pushed in
    /// `enter_scope` and popped in `exit_scope` to restore native-module
    /// shadowing when a scope that re-bound a module name exits.
    pub(crate) scope_module_shadow_marks: Vec<usize>,
    /// Block scope nesting counter (for bare `{}`, `if`, loops, try/finally).
    /// A local only counts as module-level when both `scope_depth == 0` and
    /// `inside_block_scope == 0`; `const captured = i` inside a top-level for
    /// loop must still be per-iteration box, not a shared global slot.
    pub(crate) inside_block_scope: usize,
    /// Namespace exported variables: (namespace_name, member_name, local_id)
    /// Used to resolve Namespace.member access to module-level LocalGet
    pub(crate) namespace_vars: Vec<(String, String, LocalId)>,
    /// Current namespace being lowered (for resolving internal function calls as StaticMethodCall)
    pub(crate) current_namespace: Option<String>,
    /// Module-level native instances that survive scope exits.
    /// Used for variables assigned from native calls inside functions (e.g., `mongoClient = await MongoClient.connect(uri)`).
    pub(crate) module_native_instances: Vec<(String, String, String)>,
    /// Whether this module uses fetch() — requires perry-stdlib
    pub(crate) uses_fetch: bool,
    /// Issue #76 — set when any `WebAssembly.*` HIR variant is lowered.
    pub(crate) uses_webassembly: bool,
    /// #4950: local binding name of a default import from the npm `react`
    /// package (`import React from 'react'`). When set, JSX lowers to
    /// `<local>.createElement(type, props)` so REACT controls when function
    /// components run (the reconciler installs the hooks dispatcher first).
    /// Perry's native `js_jsx` adapter calls function components EAGERLY
    /// (SSR-to-HTML semantics, hono-style) — under a real React renderer
    /// (ink, react-three-fiber, …) that ran `<Text>`'s `useContext` outside
    /// any render and threw React's "Invalid hook call" / null-dispatcher
    /// TypeError.
    pub(crate) react_default_import_local: Option<String>,
    /// #1723 — one-shot flag set by an enclosing `ns[dynamicKey].staticMember`
    /// access to tell the *immediately-nested* computed-member lowering to skip
    /// the #503 dynamic-stdlib-dispatch refusal. The dynamic index there only
    /// selects a stdlib SUB-namespace (e.g. `path.win32` / `path.posix`) and the
    /// actual member is a source-visible static name, so it is auditable — not
    /// the `ns[runtimeVar]()` obfuscation #503 targets. The guard consumes
    /// (clears) it on read so a dynamic key *inside the index* (`ns[fs[evil]]`)
    /// is still refused.
    pub(crate) suppress_stdlib_dispatch_guard_once: bool,
    /// #3896: set while lowering the *callee* of a call expression. A namespace
    /// read of an absent Node-core member (`ns.foo`) is an ordinary property
    /// miss → `undefined` (matching Node), but `ns.foo()` must still reject via
    /// the #463 gate. The expr_member read-gate consults this flag to relax only
    /// value reads, not call callees. Captured-and-cleared at the top of
    /// `lower_member_inner` so it scopes to the immediate callee member only.
    pub(crate) lowering_call_callee: bool,
    /// Compatibility escape hatch for legacy member/global lowering. Bare
    /// unresolvable identifiers throw ReferenceError, but member receivers are
    /// still allowed to use the existing GlobalGet sentinel path.
    pub(crate) unresolved_ident_as_global: bool,
    /// Active `with` object environment records. Each frame points at the
    /// synthetic local that stores the object value and the locals length when
    /// the frame was entered, so declarations inside the block/function can
    /// continue to lexically shadow the object environment.
    pub(crate) with_env_stack: Vec<WithEnvFrame>,
    pub(crate) var_hoisted_ids: HashSet<LocalId>,
    /// LocalIds of lexical let/const bindings that are forward-referenced
    /// (read before their declaration, directly or via a closure) in the
    /// current function/module body. These get a TDZ-seeded box via
    /// Stmt::PreallocateTdzBoxes so a read-before-declaration throws a spec
    /// ReferenceError. A subset of var_hoisted_ids restricted to lexical
    /// (non-var) bindings. Populated by pre_register_forward_captured_lets
    /// and drained when the body's prealloc set is assembled.
    pub(crate) tdz_forward_ids: HashSet<LocalId>,
    /// Names of lexical `let`/`const`/`class` bindings declared in the current
    /// (or an enclosing, same-function) block scope that are still resolvable as
    /// forward references — i.e. a bare read of the name before its declarator
    /// lowers to a throwing get. Used ONLY by the `typeof` lowering (#6062): a
    /// `typeof <name>` whose operand isn't yet a live local suppresses the throw
    /// (spec `GetValue`-skips for unresolvable refs → `"undefined"`), but a
    /// declared-but-uninitialized lexical in its TDZ must throw. Presence here
    /// distinguishes a forward lexical (→ throw) from a genuinely undeclared
    /// global name (→ `"undefined"`). Populated per block by
    /// `lower_stmts_using_aware` (pre-scan of the block's direct lexical decls)
    /// and cleared across function boundaries in `enter_scope`/`exit_scope`,
    /// since a `typeof` inside a nested closure is runtime-timing-dependent and
    /// must not statically throw. A post-declaration `typeof` never consults this
    /// — by then the name is a live local and never reaches the suppressing arm.
    pub(crate) forward_lexical_names: HashSet<String>,
    /// Function-boundary save stack for `forward_lexical_names` (see above).
    pub(crate) forward_lexical_saves: Vec<HashSet<String>>,
    /// Names bound by an enclosing `catch (e)` parameter that is currently in
    /// scope (a stack, innermost last). Annex B B.3.4: a `var e = init;` whose
    /// name collides with a live catch parameter assigns to that *catch
    /// parameter* binding, not the function-scoped hoisted `var` — so the outer
    /// `var e` keeps its pre-catch value (test262 `annexB/language/statements/
    /// try/catch-redeclared-var-statement`). `lower_var_decl_with_destructuring`
    /// consults this to target the shadowing catch binding via `lookup_local`
    /// instead of reusing the hoisted id. Pushed/popped around the catch body.
    pub(crate) catch_param_scopes: Vec<HashSet<String>>,
    /// Annex B B.3.3 (#5297): for the function/program scope currently being
    /// lowered, maps each name declared by a *block-nested* `function f(){}`
    /// (legacy sloppy-mode block-level function declaration) to the enclosing-
    /// scope `var`-style binding it must also write at its declaration point.
    /// `lower_nested_fn_decl` consults this (only when `inside_block_scope > 0`
    /// and the enclosing scope is sloppy) to keep the block-local binding
    /// independent of the hoisted outer `var`, then assigns the closure into the
    /// outer slot. Saved/restored across nested function bodies.
    pub(crate) annexb_block_fn_var_ids: HashMap<String, LocalId>,
    /// Annex B (#5297): names of ALL block-nested function declarations in the
    /// scope currently being lowered (superset of `annexb_block_fn_var_ids`
    /// keys — includes those whose legacy `var` is suppressed by a parameter or
    /// lexical conflict). Every block-level function declaration is block-scoped,
    /// so `lower_nested_fn_decl` gives one a fresh block-local instead of
    /// reusing an enclosing same-named binding (e.g. a parameter). Saved/restored
    /// across nested function bodies alongside `annexb_block_fn_var_ids`.
    pub(crate) annexb_block_fn_names_all: HashSet<String>,
    /// #4973: top-of-function-body `let`/`const` Ident bindings pre-registered
    /// by the function-body hoist pass so hoisted sibling FUNCTIONS that
    /// reference them before their lexical position bind the (boxed) local
    /// instead of falling through to a global read (the classic Node test
    /// shape: `function t() { server.close(); }` … `const server = …`).
    /// Keyed by the declarator-ident span (`span.lo.0`) so ONLY the exact
    /// declarator reuses the id at its Let site — a shadowing `const` in an
    /// inner block still lowers a fresh binding.
    pub(crate) lexical_forward_decls: HashMap<u32, LocalId>,
    /// Ids in `lexical_forward_decls` that were pre-registered for a NESTED
    /// block scope (not the function-body top level) by
    /// `pre_register_forward_captured_lets`. These are lexical bindings, so
    /// unlike top-level pre-registrations they must NOT be name-visible for
    /// the whole body: `lower_block_stmt` / the switch-case lowering re-bind
    /// them into `ctx.locals` exactly when their block scope is entered, and
    /// `pop_block_scope` drops them at block exit (overriding the
    /// `var_hoisted_ids` keep that would otherwise leak them into the
    /// enclosing scope). Without this, a same-named `let` in a sibling block
    /// was skipped (deduped by name) and any post-block reference of the name
    /// resolved to the block's box instead of the outer binding.
    pub(crate) nested_forward_scope_ids: HashSet<LocalId>,
    /// Shadow index: function name -> index in `functions` Vec (last entry for shadowing)
    pub(crate) functions_index: HashMap<String, usize>,
    /// Shadow index: class name -> index in `classes` Vec
    pub(crate) classes_index: HashMap<String, usize>,
    /// Shadow index: local import name -> index in `imported_functions` Vec
    pub(crate) imported_functions_index: HashMap<String, usize>,
    /// Shadow index: local alias name -> index in `builtin_module_aliases` Vec
    pub(crate) builtin_module_aliases_index: HashMap<String, usize>,
    /// Perf index for `native_instances` (which is scope-stack-like: pushed on
    /// scope entry, truncated on scope exit). Maps name -> STACK of indices into
    /// the `native_instances` Vec, innermost (last) on top. `lookup_native_instance`
    /// reads the top index for O(1) last-match-wins resolution (mirrors the old
    /// reverse scan's shadowing); on `truncate(mark)` every index >= mark is
    /// popped off each name's stack (and empty stacks removed), so an inner
    /// binding stops shadowing the moment its scope pops. See
    /// `register_native_instance` / `truncate_native_instances`.
    pub(crate) native_instances_index: HashMap<String, Vec<usize>>,
    /// Perf index for `module_native_instances` (module-level, push-only, never
    /// truncated). Scanned in reverse (last-match-wins) by
    /// `lookup_native_instance`'s fallback arm, so the index stores the LAST
    /// pushed index per name (overwritten on every push). O(1) lookup.
    pub(crate) module_native_instances_index: HashMap<String, usize>,
    /// Perf index for `func_return_native_instances` (push-only, never
    /// truncated). The old lookup scanned FORWARD (first-match-wins), so the
    /// index keeps the FIRST pushed index per name (`entry().or_insert`).
    pub(crate) func_return_native_instances_index: HashMap<String, usize>,
    /// Param names a pre-scan registered as native instances for the callback
    /// it is ABOUT to lower (e.g. the `wsId` of `server.on('upgrade', (req,
    /// wsId, head) => …)` tagged `("ws","Client")`). The callback's own param
    /// binding then calls `shadow_native_instance_if_present(param)` to tombstone
    /// stale leaked native tags — but here the tag is the FRESH, intended one,
    /// so that shadow would wrongly erase it (the `wsId.send`/`.on` dead-channel
    /// bug). One-shot per name: `shadow_native_instance_if_present` consumes the
    /// entry and skips the tombstone exactly once.
    ///
    /// Each entry is anchored to the CALLBACK's param `scope_depth` (one below
    /// the caller scope the pre-scan runs in); the consume matches at exactly
    /// that depth and `exit_scope` drops entries deeper than the surviving depth.
    /// So a protection whose expected consumer never fires (an unusual callback
    /// shape, or a binding path that doesn't route through
    /// `shadow_native_instance_if_present`) is cleared when the callback scope
    /// exits and cannot leak into the caller scope to wrongly skip a later,
    /// unrelated same-named binding's tombstone.
    pub(crate) prescan_protected_native_params: std::collections::HashMap<String, usize>,
    /// Perf index for `native_modules` (push-only, never truncated). The old
    /// `lookup_native_module` scanned FORWARD (first-match-wins), so the index
    /// keeps the FIRST pushed index per name (`entry().or_insert`).
    pub(crate) native_modules_index: HashMap<String, usize>,
    /// #wall5: scope-stack of native-module names currently SHADOWED by a local
    /// binding (param / `const`) of the same name. `lookup_native_module`
    /// returns `None` for shadowed names so a local `url`/`util`/etc. resolves
    /// as a value, not the node module. Pushed at param/var-decl sites, truncated
    /// at scope exit (parallel to `native_instances` scoping).
    pub(crate) module_shadow_stack: Vec<String>,
    /// Perf index for `class_statics` (push-only, never truncated). The old
    /// `has_static_method`/`has_static_field` scanned FORWARD (first-match-wins),
    /// so the index keeps the FIRST pushed index per class name.
    pub(crate) class_statics_index: HashMap<String, usize>,
    /// Local names bound to a `path` sub-namespace (`const w = path.win32`).
    /// Maps the local name -> (root identifier name, sub "win32"|"posix").
    /// Resolution of the root identifier to the `path` module is deferred to
    /// call-lowering time because imports aren't registered yet during the
    /// pre-scan that populates this; see `try_path_subnamespace`. #1750.
    pub(crate) subns_path_aliases: HashMap<String, (String, String)>,
    /// Local names whose value is a `WeakRef` instance (so `x.deref()` routes to
    /// `Expr::WeakRefDeref`). Pragmatic tracking — populated when lowering
    /// `let/const x = new WeakRef(...)`. Cleared on scope exit.
    pub(crate) weakref_locals: HashSet<String>,
    /// Local names whose value is a `FinalizationRegistry` instance (so
    /// `x.register(...)` / `x.unregister(...)` route to the dedicated HIR variants).
    pub(crate) finreg_locals: HashSet<String>,
    /// Local names whose value is a `WeakMap` instance — used to route
    /// `x.set/get/has/delete` to the existing Map HIR variants and to throw
    /// on primitive keys.
    pub(crate) weakmap_locals: HashSet<String>,
    /// Local names whose value is a `WeakSet` instance.
    pub(crate) weakset_locals: HashSet<String>,
    /// Local names bound by a wildcard namespace import (`import * as X from
    /// "mod"`). These are module namespaces, NOT classes — so `X.member(args)`
    /// must resolve+call the namespace member, never lower to a
    /// `StaticMethodCall`. The uppercase-imported-identifier class heuristic in
    /// `expr_call::static_and_instance` consults this set to skip namespaces
    /// (an uppercase `import * as Effect` would otherwise be mistaken for a
    /// class and the member call would return the function uncalled). Named /
    /// default imports of actual classes (`import { MongoClient }`) are NOT in
    /// this set and keep the static-method path.
    pub(crate) namespace_import_locals: HashSet<String>,
    /// #5432: locals initialized from a member-call `.fetch(...)` — the
    /// Fetch-API / WinterCG convention every server framework exposes (Hono
    /// `app.fetch`, itty-router, Cloudflare Workers handlers) returns a native
    /// fetch `Response` whose `.headers` is a Headers handle. This set is
    /// consulted ONLY by the `is_fetch_headers` guard in
    /// `array_only_methods.rs` so `res.headers.forEach(cb)` /
    /// `res.headers.entries()` bail the static array-method fold and route
    /// through the Headers FFI instead of dispatching `js_array_forEach` on a
    /// handle id (a SIGSEGV). It deliberately does NOT use
    /// `register_native_instance`, which would hijack EVERY method/property
    /// access on the local (e.g. a Bookshelf/Backbone `repo.fetch()` returning
    /// a real array would then mis-route `.map`/`.forEach`). The narrow scope —
    /// only `<name>.headers.<method>()` is affected — keeps the false-positive
    /// surface to nil.
    pub(crate) fetch_call_response_locals: HashSet<String>,
    /// Maps a namespace-import local (`import * as z from "src"`) to its source
    /// module. Used so a later bare `export { z }` re-export of that local routes
    /// to `Export::NamespaceReExport` (equivalent to `export * as z from "src"`)
    /// rather than `Export::Named`, which would resolve `z` to a bare function
    /// symbol in the importer and drop every namespace member. Closes the zod
    /// `import { z }` breakage (zod's index does `import * as z; export { z }`).
    pub(crate) namespace_import_sources: std::collections::HashMap<String, String>,
    /// Names of functions declared with `function*` — used to detect generator
    /// calls in `for...of` so the iterator protocol loop is emitted instead of
    /// the array-index loop.
    pub(crate) generator_func_names: HashSet<String>,
    /// Subset of `generator_func_names` that were `async function*`. Used by
    /// the for-of generator-call path so it can wrap `__iter.next()` in
    /// `await` (async generators always return `Promise<{value, done}>`).
    pub(crate) async_generator_func_names: HashSet<String>,
    /// Names of nested `function*` declarations that are referenced by an
    /// EARLIER sibling statement in the same enclosing function/IIFE body
    /// (forward reference). Such a generator cannot use the top-level-Function
    /// hoist path — its `FuncRef` name binding is only registered while lowering
    /// its own declaration, too late for the earlier reference, which would fall
    /// through to a `globalThis` read (`ReferenceError: <name> is not defined`).
    /// Populated by the Phase-1 pre-pass in `lower_fn_body_block_stmt` /
    /// `lower_fn_expr_anon`; consumed in `lower_body_stmt`'s FnDecl arm to route
    /// the generator through the closure path instead.
    pub(crate) nested_generator_forward_referenced: HashSet<String>,
    /// Classes that define `*[Symbol.iterator]()`. Maps class name →
    /// `FuncId` of the synthesized top-level generator function that
    /// takes `this` as its first parameter. Consumed by `for...of` to
    /// dispatch through the iterator protocol via a direct FuncRef call.
    pub(crate) iterator_func_for_class: std::collections::HashMap<String, perry_types::FuncId>,
    pub(crate) proxy_locals: HashSet<String>,
    /// #3144: local name -> builtin prototype method name, for bindings like
    /// `const m = [].map` / `const s = "".slice`. Lets the `.call`/`.apply`
    /// rewrite recognize `m.call(arr, ...)` (the receiver of `.call` is a plain
    /// identifier, not a member/literal) and synthesize `arr.map(...)`.
    pub(crate) builtin_proto_method_locals: HashMap<String, String>,
    /// Issue #76 — locals known to hold a WebAssembly instance handle (i.e.
    /// `const x = WebAssembly.instantiate(...)`). Used to route
    /// `x.exports.<method>(...)` to `Expr::WebAssemblyCallExport` only when
    /// the receiver is a tracked instance, avoiding false matches against
    /// CJS-style `module.exports.foo()` patterns.
    pub(crate) wasm_instance_locals: HashSet<String>,
    /// #809: locals whose initializer is an object literal or
    /// `Object.create(...)` — i.e. provably a plain object, never a Date.
    /// Consulted by `static_receiver_class` so `obj.toJSON()` /
    /// `obj.toString()` / `obj.valueOf()` etc. don't get rewritten to the
    /// Date intrinsics (which would read the object pointer's bits as a
    /// timestamp and print `1970-01-01T00:00:00.000Z`).
    pub(crate) plain_object_locals: HashSet<String>,
    pub(crate) proxy_revoke_locals: HashMap<String, String>,
    /// Alias map for class expressions: `const MyClass = class { ... }`
    /// binds the local `MyClass` to the synthetic class name created
    /// by `lower_class_from_ast`. The `new MyClass(...)` lowering looks
    /// up this map to resolve the alias to the real (synthetic) class
    /// name, so the New expression points at a real HIR class.
    pub(crate) class_expr_aliases: HashMap<String, String>,
    /// Mixin functions: `function withName<T>(B: Constructor<T>) { return class extends B { ... } }`.
    /// Maps mixin name → (param_name, class-expression's own name, captured
    /// class AST). The class-expression name is `None` for the common
    /// anonymous `return class extends B {…}` form and drives the synthesized
    /// class's user-visible `.name` (issue #5952). Stub field added to satisfy
    /// in-tree references; full mixin support is a separate workstream.
    pub(crate) mixin_funcs: HashMap<String, MixinFn>,
    /// Set to the class name when lowering inside a class constructor body.
    /// Used to resolve `new.target` to a placeholder object whose `.name`
    /// returns the class name. None outside any constructor.
    pub(crate) in_constructor_class: Option<String>,
    /// True while lowering inside a class declaration/expression whose
    /// heritage clause is present (`class C extends ... {}`). Combined with
    /// `in_constructor_class` to decide whether a direct `eval` body may
    /// contain `super()` (spec: PerformEval early errors). Saved/restored
    /// alongside `current_class` at both class lowering entry points.
    pub(crate) current_class_is_derived: bool,
    /// True while lowering a class field initializer expression. A direct
    /// `eval` body containing `arguments` in this context is a SyntaxError at
    /// the eval call (field initializers have no arguments object). Set in
    /// `lower_class_prop` / `lower_private_prop`.
    pub(crate) in_class_field_init: bool,
    /// Issue #562 — set to the parent class identifier (e.g. `"WritableStream"`,
    /// `"ReadableStream"`, `"TransformStream"`, or any ident from `class X
    /// extends Y`) when lowering inside a class declaration. Used by the
    /// `super({...})` pre-scan in `expr_call.rs` to register the
    /// `start`/`pull`/`transform`/`flush` callback's controller param as
    /// a `readable_stream` native instance — same shape the
    /// `new TransformStream({...})` pre-scan in `expr_new.rs` does.
    /// Saved/restored across nested class declarations.
    pub(crate) current_class_super_ident: Option<String>,
    /// Phase 3 anon-class registry for closed-shape object literals: shape key
    /// (canonical field-name + type-tag joined) -> synthetic class name. Lets
    /// identical-shape literals within the same module share one synthesized
    /// class — shared class_id, shared keys_array global, shared direct-GEP
    /// field layout. Dedup is per-module only; cross-module dedup would need
    /// a stable hash and is deferred.
    pub(crate) anon_shape_classes: HashMap<String, String>,
    /// Class DECLARATION names at the top level of the function body
    /// currently being lowered. JS resolves a method-body reference to a
    /// sibling class declared LATER in the same function at call time
    /// (vendored zod: `ZodType.optional()` calls `ZodOptional.create(...)`
    /// with ZodOptional declared hundreds of lines below) — without this
    /// set the Ident lowered to the unknown-global sentinel and the member
    /// call dispatched into `Object.create`. Scoped save/restore in
    /// `lower_fn_body_block_stmt`.
    pub(crate) forward_class_names: std::collections::HashSet<String>,
    /// For each name in `forward_class_names`, the `scope_depth` at which the
    /// `class <name>` declaration was registered. A bare-ident reference may
    /// only resolve to the class (shadowing an outer-scope local of the same
    /// name) when the class lexically encloses the reference — i.e. it was
    /// declared at a scope depth no greater than the reference's. Without this,
    /// a class in a SIBLING function factory (its name lingering in the
    /// inherited `forward_class_names` set) wrongly shadowed a legitimate
    /// captured local of the same name (Next.js app-page-turbo: a route-render
    /// closure's captured params local `ej` resolved to the `class ej`
    /// =`NextURL` reference, which then flowed into a WeakMap key and threw
    /// "Invalid value used as weak map key").
    pub(crate) forward_class_decl_depth: std::collections::HashMap<String, usize>,
    /// Scope-local class-name aliases disambiguating distinct same-named classes
    /// across nested function/factory scopes within ONE module (class refs are
    /// name-keyed: `Expr::New { class_name }` / `ClassRef(name)`). When a body
    /// declares `class X` while an outer/prior `class X` is already registered,
    /// the body's X is renamed `X$<n>` and `X -> X$<n>` recorded so every
    /// reference in that body binds to the lexically-correct class. Saved/
    /// restored per body in both `lower_fn_body_block_stmt` and `lower_fn_expr`.
    pub(crate) class_renames: std::collections::HashMap<String, String>,
    /// Monotonic suffix source for `class_renames` unique names.
    pub(crate) next_class_rename_id: u32,
    /// Names of TOP-LEVEL `class X { … }` declarations in the module being
    /// lowered (populated by the module pre-pass). A NAMED class EXPRESSION
    /// nested in a function body — e.g. minimatch's `defaults()` returns
    /// `Object.assign(m, { Minimatch: class Minimatch extends orig.Minimatch
    /// {…} })` — whose name collides with one of these must NOT reuse the
    /// top-level class's ClassId / module-scope registration: per JS spec a
    /// class-expression's name binds only inside its own body. Reusing the id
    /// silently overwrote the real exported class with the (nearly empty)
    /// nested expression, so `new Minimatch(...)` produced a body-less
    /// instance (every field/method undefined). This set lets
    /// `lower_class_from_ast` detect that collision and allocate a fresh,
    /// uniquely-named class instead.
    pub(crate) module_class_decl_names: std::collections::HashSet<String>,
    /// Counter for generating anon-class names (`__AnonShape_N`).
    // #854: initialized in `new` but unread — anon-shape classes are now named
    // by content-addressed FNV hash (see `synthesize_anon_shape_class`), not by
    // this counter. Kept for the struct's field set.
    #[allow(dead_code)]
    pub(crate) next_anon_shape_id: u32,
    /// Phase 4.1: method return types registry keyed by (class_name,
    /// method_name). Populated as methods are lowered so call-site inference
    /// (`infer_call_return_type`'s Member arm) can resolve `obj.method()` to
    /// the method's declared or inferred return type when `obj`'s type is
    /// `Type::Named(class_name)`. Mirrors `func_return_types` but for the
    /// method-dispatch path.
    pub(crate) class_method_return_types: Vec<(String, String, Type)>,
    /// Issue #212: classes nested inside a function whose method bodies
    /// reference enclosing-scope locals. `lower_class_decl` adds hidden
    /// `__perry_cap_<id>` fields, prepends `let id = this.__perry_cap_<id>`
    /// to each capturing instance method, extends the constructor with one
    /// synthesized param per captured id, and registers the captured ids
    /// here so the `Expr::New { class_name }` lowering can append
    /// `LocalGet(id)` for each captured id at every construction site.
    pub(crate) class_captures: Vec<(String, Vec<LocalId>)>,
    /// Issue #740: `let_name → class_name` for `let/const/var <name> = <ClassRef>`
    /// initializers. Lets `Expr::New { class_name }` (where `class_name` is
    /// the source-level identifier of an alias binding) resolve to the
    /// underlying class so its `class_captures` (if any) get appended as
    /// ctor args at the `new` site. Mirrors codegen's `local_class_aliases`,
    /// but built at HIR-lowering time so the captured-arg LocalGets land in
    /// the HIR (where codegen consumes them) rather than being patched in
    /// after lowering.
    pub(crate) let_class_aliases: Vec<(String, String)>,
    /// LocalIds known to hold the global object (`globalThis`). This lets
    /// value-read recognisers treat `const g = globalThis; g.Response` like
    /// `globalThis.Response` without relying on source-level names.
    pub(crate) global_this_aliases: HashSet<LocalId>,
    /// Issue #838: locals whose initializer is `<ClassName>.prototype`.
    /// Lets the assignment lowering recognise `proto.method = fn` as a
    /// prototype-method assignment on the underlying class (rather than
    /// a write to a regular object). dayjs's minified shape is
    /// `var m = M.prototype; m.parse = function(){…}; m.init = function(){…};`
    /// — every subsequent assignment through the alias has to route to
    /// the class's prototype-method registry, otherwise the methods
    /// land in an orphaned object literal and instance reads fall back
    /// to undefined.
    pub(crate) prototype_aliases: HashMap<LocalId, String>,
    /// Issue #838 followup (b): parallel to `prototype_aliases`, but
    /// keyed by FuncId for the function-declaration variant. dayjs's
    /// minified `function M(){…}; var m = M.prototype; m.parse = …`
    /// path stores `m → fn_id` here so the assignment recogniser in
    /// `lower/expr_assign.rs` can route to
    /// `Expr::RegisterFunctionPrototypeMethod` (synthetic class id
    /// allocated at runtime against the closure's bits) instead of
    /// dropping the assignment as a no-op PropertySet.
    pub(crate) prototype_function_aliases: HashMap<LocalId, FuncId>,
    /// Issue #838 followup (b): set of locals whose initialiser is a
    /// `Closure` or `FuncRef` — i.e. local-scope bindings that hold a
    /// callable value at runtime. Babel's class-from-function emit
    /// pattern (and dayjs's minified bundle) puts the inner constructor
    /// as a nested `function M(){}` which the HIR lowers to a
    /// `Let { name: "M", init: Some(Closure{…}) }`. Subsequent
    /// `M.prototype.x = fn` or `var m = M.prototype; m.x = fn` patterns
    /// resolve `M` as a `LocalGet(M_id)` — this set is the test that
    /// gate the function-classic prototype-method route.
    pub(crate) function_valued_locals: HashSet<LocalId>,
    /// Issue #838 followup (b): parallel to `prototype_aliases` /
    /// `prototype_function_aliases`, but for the local-scope shape
    /// (`var m = M.prototype` where `M` is a `Let M = Closure{…}`).
    /// Stores `m_id → M_id` so the assignment recogniser can emit
    /// `RegisterFunctionPrototypeMethod { func: LocalGet(M_id), … }`
    /// — codegen then dispatches through the same singleton-closure
    /// path the matching `new M(args)` site uses.
    pub(crate) prototype_function_locals: HashMap<LocalId, LocalId>,
    /// Issue #886: locals whose initializer is `Object.<staticMethod>`
    /// (e.g. `const __defProp = Object.defineProperty;`). esbuild's
    /// CJS-bundle prelude aliases the static method to a short local
    /// and calls it indirectly (`__defProp(target, name, descriptor)`).
    /// Pre-fix the indirect call fell through to a generic
    /// `LocalGet(__defProp)(...)` dispatch where the captured value
    /// resolves to undefined (the recogniser only fires on the literal
    /// `Object.<method>` AST shape at the call site) and throws
    /// `TypeError: value is not a function`. Stores `id → "defineProperty"`
    /// etc. so the call lowering can synthesize the matching dedicated
    /// HIR variant when the callee is `LocalGet(id)`. Only populated
    /// for methods that already have a dedicated recogniser arm — see
    /// `lower/expr_call.rs` for the dispatch list.
    pub(crate) object_static_method_aliases: HashMap<LocalId, String>,
    /// Same alias-call repair for selected `Array.<staticMethod>` captures.
    /// Test262's descriptor helper stores `Array.isArray` in a local and calls
    /// that alias; Perry's direct-call intrinsic already works, but the value
    /// read currently lowers to a non-callable sentinel.
    pub(crate) array_static_method_aliases: HashMap<LocalId, String>,
    /// Issue #444: true when this module is the user-supplied entry file.
    /// Drives `import.meta.main` — Node 24+ / Bun semantics where the entry
    /// module reports `true` and every imported module reports `false`. Set
    /// by `lower_module_with_class_id_types_seed_and_entry`; default false.
    pub(crate) is_entry_module: bool,
    /// #5833: true once lowering has produced at least one `Expr::GlobalThisExpr`
    /// from a top-level `this` (global-script mode, `PERRY_GLOBAL_SCRIPT_THIS`).
    /// `Module::references_global_this` gates codegen's reflection of
    /// top-level `function`/`var` declarations onto the global object, but it
    /// was computed by a plain substring scan for the literal token
    /// `globalThis` in the module source — a program that reads the global
    /// object only through top-level `this` (as Test262's `verifyProperty(this,
    /// ...)` idiom does) never contains that literal, so the reflection
    /// silently never ran. OR'd into `references_global_this` once lowering
    /// finishes (test262 `language/global-code/decl-func.js`,
    /// `language/global-code/decl-var.js`).
    pub(crate) saw_global_this_expr: bool,
    /// #5833: names assigned by a **direct top-level** `name = ...`/`name++`
    /// expression statement — see
    /// `collect_direct_top_level_reassigned_identifiers`, which deliberately
    /// does NOT reuse the deeper, name-only (no scope tracking)
    /// `collect_assigned_function_binding_candidates` scan: that would
    /// false-positive on a same-named binding shadowed in a nested block.
    /// Consulted when lowering a top-level `class` declaration: a class name
    /// is normally
    /// resolved purely through the class registry (`ctx.lookup_class` /
    /// `Expr::ClassRef`), with no backing local variable, so every read
    /// re-derives the same class-id value and every write is a silent no-op
    /// special-cased in `expr_assign.rs` (the class binding is otherwise
    /// treated as immutable). GlobalDeclarationInstantiation says a class
    /// declaration creates a MUTABLE binding, so `Foo = 5` must actually take
    /// (test262 `language/global-code/decl-lex.js`). Giving *every* class a
    /// real local would fix that, but it also makes plain reads of the name
    /// resolve to `Expr::LocalGet` instead of `Expr::ClassRef` everywhere —
    /// breaking call sites that pattern-match `Expr::ClassRef` after lowering
    /// a bare class-name identifier (e.g. `export default Widget;`, #665).
    /// Only seeding a local for names already known to be reassigned keeps
    /// the overwhelmingly common (never-reassigned) case byte-for-byte
    /// unchanged.
    pub(crate) reassigned_top_level_identifiers: HashSet<String>,
    /// Strictness inherited from the module/script directive prologue or from
    /// ECMAScript module syntax. Function lowering consults this before
    /// deciding whether its `arguments` object should be mapped.
    pub(crate) module_strict: bool,
    /// Stack of strict-mode state for currently lowered function/arrow bodies.
    /// Nested ordinary functions inherit strictness from strict parents, while
    /// a body-level `"use strict"` directive pushes strictness for children.
    pub(crate) strict_mode_stack: Vec<bool>,
    /// Issue #668: true when this module was reached via an npm-package import
    /// (file lives under a `node_modules/` segment in either the canonical or
    /// the un-canonical resolution path). External libraries are exempt from
    /// the user-facing `require(literal)` compile error in `lower_call.rs`,
    /// preserving the legacy fall-through behavior — packages like
    /// `@perryts/redis` deliberately use `require(literal)` to defer
    /// cycle-breaking imports inside method bodies that may never execute.
    pub(crate) is_external_module: bool,
    /// Depth of nested `try { ... }` bodies currently being lowered.
    /// Optional platform modules in Electron-style code commonly use
    /// `try { var x = require("native-addon") } catch {}`. Let those sites
    /// lower to the existing runtime failure path so the catch block can
    /// observe the failure instead of rejecting the whole user source at
    /// compile time. Outside try blocks, `require(literal)` still hard-errors.
    pub(crate) optional_require_try_depth: u32,
    /// Pre-scanned constant environment for `new Function` / `Function(...)`
    /// argument resolution (single-assignment module vars, `toString`-bearing
    /// object literals, counters). Built once per module in
    /// `lower_module_full`; consumed by `const_fold_fn`.
    pub(crate) fn_ctor_env: super::fn_ctor_env::FnCtorEnv,
    /// Current recursion depth of `lower_expr` (#5259). Incremented on entry,
    /// decremented on exit. Once it exceeds either the broad
    /// `MAX_EXPR_LOWER_DEPTH` ceiling or the lower stack-heavy chain ceiling,
    /// lowering bails with a clean "nested too deeply" diagnostic instead of
    /// letting pathologically-nested expressions overflow the native stack and
    /// SIGABRT.
    pub(crate) expr_lower_depth: u32,
    /// Perf: a single-slot memo of an already-lowered member receiver, keyed by
    /// its source span `(lo, hi)`. Set by the chained-native-method dispatch
    /// helper (`try_static_method_and_instance`) just before it returns
    /// `Err(args)` after lowering `member.obj` to inspect it; consumed once by
    /// `lower_member_inner` when the `lower_call_inner` fall-through tail
    /// re-lowers the same member callee. Without it, a long native-fluent
    /// method chain (`K.name(..).description(..).option(..)…` — commander/minified
    /// CLI builders) re-lowers the entire receiver prefix at every chain level
    /// (the helper lowers it, finds the inner result is not a `NativeMethodCall`,
    /// discards it, and the tail lowers it again — compounding to exponential
    /// blowup). The memo lets the tail reuse the helper's lowering, so each
    /// receiver subtree is lowered exactly once. Lowering a receiver is
    /// idempotent w.r.t. the value produced (the fluent-success path already
    /// reuses the same lowered receiver), so reusing it is semantics-preserving.
    pub(crate) prelowered_member_receiver: Option<((u32, u32), Expr)>,
    /// True when the eval call site is lexically inside a non-arrow function
    /// (ordinary `function` declaration/expression, method, constructor, getter,
    /// setter, or static block). Used by `check_direct_eval_super_private` to
    /// enforce the spec rule: `new.target` in a direct eval body is only legal
    /// when the eval is contained in function code that is NOT an ArrowFunction
    /// (ES2025 §15.2.1.1, early error for `new.target` in eval). ArrowFunction
    /// bodies and module/script top-level both leave this false.
    pub(crate) in_nonarrow_fn: bool,
}
