//! Constructor `this.<name> = …` field inference, shared by class
//! declarations (`lower_class_decl`) and class expressions
//! (`lower_class_from_ast`).
//!
//! Issue #10499: the scan used to live inline in `lower_class_decl` only, so
//! a plain-JS class EXPRESSION (`var NodeObject = class {…}`,
//! `module.exports = class X {…}`, esbuild/rollup `var X = class {…}`) was
//! lowered with zero fields and every constructor store became a property
//! ADD through the full `[[Set]]` path (~4,800 instructions each) instead of
//! a slot store — `new` on typescript.js's `NodeObject` ran 159× slower than
//! Node against 7× for the identical declaration.

use super::*;

/// Append an own instance field for every top-level constructor
/// `this.<ident> = …` assignment not already covered by a declared field, an
/// inherited field, an (own or inherited) accessor, or an (own or inherited)
/// method, then register the class's complete field / accessor / method
/// name sets under `name` so classes lowered later that extend it see the
/// whole chain in one lookup.
///
/// `parent_name` is the class-registry name of the direct parent, if known.
/// When it is not (the heritage took the dynamic `extends_expr` path, e.g. a
/// lexically-local `const Base = class {…}` binding), a plain-identifier
/// heritage is still looked up under its alias-resolved name to EXCLUDE
/// whatever that class registered: an extra exclusion only leaves a name on
/// the generic property path, while a missed one makes an own slot shadow
/// the value the parent's constructor stored (#10499 — a declaration
/// subclass of a class-expression base read `this.side` as `undefined`).
///
/// With `infer_fields == false` nothing is appended and no field-name set is
/// registered (a class whose parent layout is unknown must not publish an
/// incomplete one); the accessor and method sets are still registered.
pub(super) fn infer_ctor_this_fields(
    ctx: &mut LoweringContext,
    class: &ast::Class,
    name: &str,
    parent_name: Option<&str>,
    infer_fields: bool,
    fields: &mut Vec<ClassField>,
) {
    // JavaScript classes (e.g., transpiled from TypeScript) often don't have ClassProp
    // declarations; instead they assign to `this` in the constructor body.
    //
    // IMPORTANT: Also exclude fields inherited from parent classes. If the parent already
    // declares `kind` and the subclass writes `this.kind = ...`, the subclass must NOT
    // add `kind` as a new own field. Otherwise, codegen's resolve_class_fields later
    // merges parent and own indices and the subclass's shadow `kind` gets a different
    // offset from the parent's, leaving TWO `kind` slots that disagree at runtime.
    //
    // Collect inherited field names by walking the parent chain via the parent name.
    // Previous lowerings have registered each class's full (own+inherited) field set,
    // so a single lookup on the direct parent yields the complete chain.
    let heritage_ident_name: Option<String> = match (parent_name, class.super_class.as_deref()) {
        (None, Some(ast::Expr::Ident(ident))) => {
            let raw = ident.sym.to_string();
            Some(ctx.resolve_class_alias(&raw).unwrap_or(raw))
        }
        _ => None,
    };
    let parent_name = parent_name.or(heritage_ident_name.as_deref());
    let mut inherited_field_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    if let Some(parent_name) = parent_name {
        if let Some(parent_fields) = ctx.lookup_class_field_names(parent_name) {
            for f in parent_fields {
                inherited_field_names.insert(f.clone());
            }
        }
    }

    // Issue #665 (sixth pass): collect own + inherited accessor (getter+setter)
    // property names. Real-world packages like rate-limiter-flexible
    // declare a `set points(v)` accessor AND write `this.points = opts.points`
    // from the constructor body. Pre-fix the bare-this scan below
    // mis-categorised `points` as an own data field, allocating an
    // inline slot that surfaced via `Object.keys` and shadowed the
    // accessor when a subclass instance's `.points` was read across
    // modules (the runtime's setter dispatch walks the class vtable
    // chain correctly, but the spurious own-data slot wins lookup).
    let mut accessor_names = runtime_instance_accessor_names(&class.body);
    // Pull in accessor names from the parent chain. The parent's
    // registration stored the own+inherited union, so a single lookup
    // on the direct parent suffices.
    if let Some(parent_name) = parent_name {
        if let Some(parent_accessors) = ctx.lookup_class_accessor_names(parent_name) {
            accessor_names.extend_from(parent_accessors);
        }
    }

    // Own instance method names. A constructor `this.method = this.method.bind(this)`
    // (zod's `ZodType` ctor self-binds ~20 methods; React class components do the
    // same) is a METHOD OVERRIDE, not a new data field — the assignment creates a
    // runtime own property handled by the method-override dispatch path. Allocating
    // an inline field slot for it makes the codegen field branch shadow the method on
    // every read, so `this.method` reads the uninitialised slot (`undefined`) BEFORE
    // the assignment runs — exactly what made `this.parse.bind(this)` throw "Bind must
    // be called on a function" in zod. Mirrors the accessor exclusion (#665).
    let mut method_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for member in &class.body {
        match member {
            ast::ClassMember::Method(m) if matches!(m.kind, ast::MethodKind::Method) => {
                let key = match &m.key {
                    ast::PropName::Ident(i) => i.sym.to_string(),
                    ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                    _ => continue,
                };
                method_names.insert(key);
            }
            ast::ClassMember::PrivateMethod(m) if matches!(m.kind, ast::MethodKind::Method) => {
                method_names.insert(format!("#{}", m.key.name));
            }
            _ => {}
        }
    }
    // Issue #10487: pull in the parent chain's own+inherited method
    // names too, mirroring the accessor union just above. A subclass
    // constructor's `this.close = …` overriding a PARENT method (not
    // redeclared on this class) must be recognized as a method
    // override, not a new own data field, or the field wins the
    // dynamic-dispatch lookup and instance reads see `undefined`
    // until the assignment statement runs.
    if let Some(parent_name) = parent_name {
        if let Some(parent_methods) = ctx.lookup_class_method_names(parent_name) {
            for m in parent_methods {
                method_names.insert(m.clone());
            }
        }
    }

    if infer_fields {
        infer_and_register_fields(
            class,
            name,
            ctx,
            inherited_field_names,
            &accessor_names,
            &method_names,
            fields,
        );
    }

    // Issue #665: register own+inherited accessor names so subclasses
    // lowered after this one can also skip them when scanning ctor
    // bodies. `accessor_names` already contains the getter/setter names
    // from the parent-chain lookup above. For a class EXPRESSION this
    // also lets the assignment recogniser in `expr_assign.rs` treat
    // `C.prototype.<accessor> = v` as a setter INVOCATION instead of a
    // prototype-method monkey-patch (test262 accessor-name-inst setters).
    ctx.register_class_accessor_names(name.to_string(), accessor_names);

    // Issue #10487: register this class's complete (own + inherited)
    // method-name set, mirroring the accessor registration just above,
    // so a further subclass lowered after this one sees the full
    // chain in one lookup.
    ctx.register_class_method_names(name.to_string(), method_names.into_iter().collect());
}

fn infer_and_register_fields(
    class: &ast::Class,
    name: &str,
    ctx: &mut LoweringContext,
    inherited_field_names: std::collections::HashSet<String>,
    accessor_names: &crate::class_accessors::ClassAccessorNames,
    method_names: &std::collections::HashSet<String>,
    fields: &mut Vec<ClassField>,
) {
    let declared_field_names: std::collections::HashSet<String> =
        fields.iter().map(|f| f.name.clone()).collect();
    for member in &class.body {
        if let ast::ClassMember::Constructor(ctor) = member {
            if let Some(ref body) = ctor.body {
                for stmt in &body.stmts {
                    if let ast::Stmt::Expr(expr_stmt) = stmt {
                        let mut names: Vec<String> = Vec::new();
                        collect_this_field_assigns(&expr_stmt.expr, &mut names);
                        for fname in names {
                            if !declared_field_names.contains(&fname)
                                && !inherited_field_names.contains(&fname)
                                && !accessor_names.contains_any(&fname)
                                && !method_names.contains(&fname)
                            {
                                fields.push(ClassField {
                                    name: fname,
                                    key_expr: None,
                                    ty: Type::Any,
                                    init: None,
                                    is_private: false,
                                    is_readonly: false,
                                    decorators: Vec::new(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    // Dedup fields: keep first occurrence of each name
    let mut seen = std::collections::HashSet::new();
    fields.retain(|f| seen.insert(f.name.clone()));

    // Register this class's complete field set (own + inherited) so subclasses that
    // extend it can see the full inheritance chain during their own lowering.
    let mut complete_field_names: Vec<String> = inherited_field_names.into_iter().collect();
    for f in fields.iter() {
        if !complete_field_names.contains(&f.name) {
            complete_field_names.push(f.name.clone());
        }
    }
    ctx.register_class_field_names(name.to_string(), complete_field_names);
}

// Pull each top-level `this.<ident> = …` field name out of one ctor
// statement-expression. Minified bundles (Next.js `BaseNextRequest`'s
// `constructor(a,b,c){this.method=a,this.url=b,this.body=c}`) collapse
// every ctor assignment into ONE comma-`Seq` expression-statement, so a
// scan that only matched `Expr::Assign` detected ZERO fields — the
// parent's `method`/`url`/`body` never entered `packed_keys`, leaving
// the subclass instance allocated with too-few inline slots so the
// captured-class shape prepends `__perry_cap_*` over the (missing) real
// slots and `e.url` reads undefined ("Invalid URL" 500 on dynamic page
// routes). Descend through `Seq` (and the `Paren`/`Assign`-result-chain
// wrappers minifiers emit) so each comma-separated `this.x = …` is
// recognised the same as a standalone assignment statement.
fn collect_this_field_assigns(expr: &ast::Expr, out: &mut Vec<String>) {
    match expr {
        ast::Expr::Assign(assign) => {
            // A chained assignment's RHS can itself be `this.x = …`
            // (`this.a = this.b = v`): the inner `this.b = v` evaluates
            // (and creates `b`'s slot) BEFORE the outer assignment to
            // `this.a`, so collect the RHS first to keep Object.keys in
            // the same insertion order Node produces (`b` then `a`).
            collect_this_field_assigns(&assign.right, out);
            if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Member(mem)) = &assign.left {
                if let ast::Expr::This(_) = &*mem.obj {
                    if let ast::MemberProp::Ident(prop_ident) = &mem.prop {
                        out.push(prop_ident.sym.to_string());
                    }
                }
            }
        }
        ast::Expr::Seq(seq) => {
            for e in &seq.exprs {
                collect_this_field_assigns(e, out);
            }
        }
        ast::Expr::Paren(p) => collect_this_field_assigns(&p.expr, out),
        _ => {}
    }
}
