use perry_types::{LocalId, Type};

use crate::ir::*;
use crate::lower::LoweringContext;

use super::class_members::collect_method_captures;

pub fn synthesize_class_captures(
    ctx: &mut LoweringContext,
    name: &str,
    extends_name: Option<&str>,
    has_heritage: bool,
    fields: &mut Vec<ClassField>,
    methods: &mut Vec<Function>,
    getters: &mut Vec<(String, Function)>,
    setters: &mut Vec<(String, Function)>,
    computed_members: &mut Vec<ClassComputedMember>,
    constructor: &mut Option<Function>,
    static_methods: &mut Vec<Function>,
) {
    let cap_salt = ctx.cap_salt();
    let module_level_ids = ctx.module_level_ids.clone();
    let outer_scope_ids: std::collections::HashSet<LocalId> =
        ctx.locals.iter().map(|(_, id, _)| *id).collect();
    let mut union_captures: std::collections::BTreeSet<LocalId> = std::collections::BTreeSet::new();
    for m in methods.iter() {
        for id in collect_method_captures(m, &outer_scope_ids, &module_level_ids) {
            union_captures.insert(id);
        }
    }
    for (_, g) in getters.iter() {
        for id in collect_method_captures(g, &outer_scope_ids, &module_level_ids) {
            union_captures.insert(id);
        }
    }
    for (_, s) in setters.iter() {
        for id in collect_method_captures(s, &outer_scope_ids, &module_level_ids) {
            union_captures.insert(id);
        }
    }
    // Both instance AND static computed members (`static [k]() {}` and the
    // static methods next emits as computed members, e.g. `NextResponse.json`)
    // can reference enclosing-fn locals — and even when they don't, a
    // `new <Self>(…)` inside them still needs the constructor's capture args
    // appended at the call site. Collect from all of them so the union (and
    // thus the decl-site snapshot) is complete. Refs #5199.
    for member in computed_members.iter() {
        for id in collect_method_captures(&member.function, &outer_scope_ids, &module_level_ids) {
            union_captures.insert(id);
        }
    }
    if let Some(ctor) = constructor.as_ref() {
        for id in collect_method_captures(ctor, &outer_scope_ids, &module_level_ids) {
            union_captures.insert(id);
        }
    }
    // STATIC methods reference enclosing-fn locals too (vendored zod's
    // `static create(...)` reads the ZodFirstPartyTypeKind enum local).
    // Their refs join the union so the decl-site snapshot includes them;
    // the rewrite below reads the snapshot instead of instance fields.
    for sm in static_methods.iter() {
        for id in collect_method_captures(sm, &outer_scope_ids, &module_level_ids) {
            union_captures.insert(id);
        }
    }
    // Issue #740: field initializers (`readonly _tag = tag` declared on
    // a class nested inside a function) also capture outer-scope locals.
    // Without this, `LocalGet(outer_id)` inside a field's init expression
    // would read a non-existent local in the ctor's scope when
    // `apply_field_initializers_recursive` lowers the initializer.
    // Collect refs from both the init expr and the computed key_expr.
    for field in fields.iter() {
        if let Some(init) = &field.init {
            let mut refs = Vec::new();
            let mut visited = std::collections::HashSet::new();
            crate::analysis::collect_local_refs_expr(init, &mut refs, &mut visited);
            for id in refs {
                if outer_scope_ids.contains(&id) && !module_level_ids.contains(&id) {
                    union_captures.insert(id);
                }
            }
        }
        if let Some(key) = &field.key_expr {
            let mut refs = Vec::new();
            let mut visited = std::collections::HashSet::new();
            crate::analysis::collect_local_refs_expr(key, &mut refs, &mut visited);
            for id in refs {
                if outer_scope_ids.contains(&id) && !module_level_ids.contains(&id) {
                    union_captures.insert(id);
                }
            }
        }
    }
    // Inherited captures: if this class extends a parent that registered
    // captures, the parent's instance methods read from
    // `this.__perry_cap_<inherited_id>` fields the parent ctor would have
    // initialized. With our synthesized constructor on this child class,
    // the parent ctor is no longer called automatically (lower_new only
    // walks parents when the child has *no* own constructor). Union the
    // parent's captures into our captures_vec so the child's synthesized
    // ctor takes the inherited capture as a param too — and the
    // `Expr::New { class_name: <child> }` site appends `LocalGet(id)`
    // for every captured id (own + inherited). The fields themselves are
    // still deduplicated below — the child only declares the OWN-not-
    // inherited subset, so a single keys-array entry exists per capture.
    if let Some(pname) = extends_name {
        if let Some(parent_caps) = ctx.lookup_class_captures(pname) {
            for id in parent_caps {
                union_captures.insert(*id);
            }
        }
    }
    let captures_vec: Vec<LocalId> = union_captures.into_iter().collect();

    if captures_vec.is_empty() {
        return;
    }

    // Walk the parent chain to find which `__perry_cap_<id>` fields
    // are already declared by an ancestor. Inherited fields share the
    // same instance slot via the runtime's by-name lookup; declaring
    // them again here would leave two same-named entries in the keys
    // array at different offsets and the parent's method body would
    // read the parent's index while the child's ctor wrote to the
    // child's index — the inherited-class-with-shared-capture case.
    // Parent classes also synthesize a constructor that takes the
    // capture as a param, so the child's constructor needs to
    // forward inherited capture args to `super(...)` rather than
    // store them itself.
    let mut inherited_cap_field_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    if let Some(pname) = extends_name {
        if let Some(parent_fields) = ctx.lookup_class_field_names(pname) {
            for f in parent_fields {
                if f.starts_with("__perry_cap_") {
                    inherited_cap_field_names.insert(f.clone());
                }
            }
        }
    }
    let inherited_cap_ids: std::collections::HashSet<LocalId> = captures_vec
        .iter()
        .copied()
        .filter(|cid| {
            inherited_cap_field_names.contains(&crate::cap_fields::cap_field_name(cap_salt, *cid))
        })
        .collect();

    // 1. Hidden fields keyed by outer id, skipping inherited.
    for &cid in &captures_vec {
        if inherited_cap_ids.contains(&cid) {
            continue;
        }
        fields.push(ClassField {
            name: crate::cap_fields::cap_field_name(cap_salt, cid),
            key_expr: None,
            ty: Type::Any,
            init: None,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        });
    }
    if let Some(existing) = ctx.lookup_class_field_names(name) {
        let mut updated: Vec<String> = existing.to_vec();
        for &cid in &captures_vec {
            let field_name = crate::cap_fields::cap_field_name(cap_salt, cid);
            if !updated.contains(&field_name) {
                updated.push(field_name);
            }
        }
        ctx.register_class_field_names(name.to_string(), updated);
    }

    // Look up the outer-scope type for each captured id so the
    // rebind let can preserve typed-array fast paths (`out.length`,
    // `out[i]`, etc.). Without this the rebind defaults to
    // `Type::Any`, the codegen `local_types` map records the rebind
    // as Any, and `out.length` on a `string[]` capture falls off the
    // typed-array fast path into generic object-field-by-name dispatch
    // — which on an array silently returns undefined or crashes.
    let captured_outer_types: std::collections::HashMap<LocalId, Type> = captures_vec
        .iter()
        .map(|&cid| {
            let ty = ctx
                .locals
                .iter()
                .rev()
                .find(|(_, id, _)| *id == cid)
                .map(|(_, _, t)| t.clone())
                .unwrap_or(Type::Any);
            (cid, ty)
        })
        .collect();

    // Field-propagation map keyed by OUTER ids. Every `LocalSet(outer_id, v)`
    // and `Expr::Update { id: outer_id, .. }` at a top-level expression
    // position inside a method body is rewritten to also propagate the
    // new value to `this.__perry_cap_<id>`. Without this, a setter
    // writing to a captured primitive (`set value(v) { stored = v; }`)
    // would only update the method-local rebind slot, and the next
    // getter call would re-read the field's stale snapshot. The
    // propagation only fires at top-level positions (statement-level
    // expression, return value, condition); nested captured writes
    // like `(stored = v).toString()` only update the local — rare
    // enough to defer to a follow-up.
    let field_propagation: std::collections::HashMap<LocalId, String> = captures_vec
        .iter()
        .map(|&cid| (cid, crate::cap_fields::cap_field_name(cap_salt, cid)))
        .collect();

    // Helper closure: build a fresh-id map for one function's body,
    // rewrite the body refs (with field-write propagation), and
    // prepend the rebinding lets.
    let rewrite_method_body = |ctx: &mut LoweringContext,
                               body: &mut Vec<Stmt>|
     -> std::collections::HashMap<LocalId, LocalId> {
        let mut id_map: std::collections::HashMap<LocalId, LocalId> =
            std::collections::HashMap::new();
        let mut prologue: Vec<Stmt> = Vec::new();
        for (index, &outer_id) in captures_vec.iter().enumerate() {
            let new_id = ctx.fresh_local();
            id_map.insert(outer_id, new_id);
            let ty = captured_outer_types
                .get(&outer_id)
                .cloned()
                .unwrap_or(Type::Any);
            // FIELD-FIRST with a decl-site-snapshot fallback: the
            // `this.__perry_cap_*` stash is written by the constructor AFTER
            // `super()` returns, but a method can run EARLIER — a base-class
            // constructor may virtual-dispatch into this class's override
            // (Next.js: base `Server`'s ctor calls `this.getHasStaticDir()`,
            // the `NextNodeServer` override, which reads the module-level
            // `_fs` interop binding through its cap field → it read
            // `undefined` and threw at boot, #5437). When the field is still
            // undefined, fall back to the class's decl-site capture snapshot
            // (same machinery as the ctor param rebinds above).
            prologue.push(Stmt::Let {
                id: new_id,
                name: crate::cap_fields::cap_field_name(cap_salt, outer_id),
                ty,
                mutable: true,
                init: Some(Expr::ClassCaptureValue {
                    class_name: name.to_string(),
                    index: index as u32,
                    fallback: Some(Box::new(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::This),
                        property: crate::cap_fields::cap_field_name(cap_salt, outer_id),
                    })),
                    prefer_fallback: true,
                }),
            });
        }
        // Rewrite first (so closure captures lists pick up the new ids
        // at the same time as the body's refs), then prepend the let.
        crate::analysis::remap_local_ids_in_stmts_with_field_propagation(
            body,
            &id_map,
            &field_propagation,
        );
        prologue.append(body);
        *body = prologue;
        id_map
    };

    // SELF-construction inside this class's own members: `new <Self>(…)`
    // sites in method bodies were lowered BEFORE this class registered its
    // captures, so the `Expr::New` Ident arm appended nothing (vendored
    // zod's `_addCheck(e){ return new ZodString({…this._def…}) }`). After
    // `rewrite_method_body` runs, the method prologue rebinds every capture
    // under a fresh id — append those rebind ids here. Nested closure
    // bodies are walked too; their capture lists already include the
    // prologue ids when the closure body references them, and a closure
    // whose ONLY reference is the appended arg gets the id added to its
    // captures list below.
    fn append_self_new_args_expr(
        expr: &mut Expr,
        class_name: &str,
        cap_args: &[(LocalId, LocalId)],
    ) {
        append_new_args_expr(expr, class_name, cap_args, false)
    }
    fn append_self_new_args_stmt(
        stmt: &mut Stmt,
        class_name: &str,
        cap_args: &[(LocalId, LocalId)],
    ) {
        append_new_args_stmt(stmt, class_name, cap_args, false)
    }

    // 2. Methods / getters / setters. After each body's capture rebind,
    //    append the rebind ids to any SELF-construction `new <Self>(…)`
    //    sites the body contains (lowered before this class registered).
    let append_self_sites =
        |body: &mut Vec<Stmt>, id_map: &std::collections::HashMap<LocalId, LocalId>| {
            let cap_args: Vec<(LocalId, LocalId)> = captures_vec
                .iter()
                .filter_map(|oid| id_map.get(oid).map(|f| (*oid, *f)))
                .collect();
            for stmt in body.iter_mut() {
                append_self_new_args_stmt(stmt, name, &cap_args);
            }
        };
    for m in methods.iter_mut() {
        let id_map = rewrite_method_body(ctx, &mut m.body);
        append_self_sites(&mut m.body, &id_map);
    }
    for (_, g) in getters.iter_mut() {
        let id_map = rewrite_method_body(ctx, &mut g.body);
        append_self_sites(&mut g.body, &id_map);
    }
    for (_, s) in setters.iter_mut() {
        let id_map = rewrite_method_body(ctx, &mut s.body);
        append_self_sites(&mut s.body, &id_map);
    }
    for member in computed_members
        .iter_mut()
        .filter(|member| !member.is_static)
    {
        let id_map = rewrite_method_body(ctx, &mut member.function.body);
        append_self_sites(&mut member.function.body, &id_map);
    }

    // 2b. STATIC methods: no instance carries `__perry_cap_*` fields, so
    // the prologue rebinds read the decl-site snapshot instead
    // (`ClassCaptureValue { class_name, index }` →
    // `js_class_capture_value(class_id, index)` at codegen). The snapshot
    // is written by the `RegisterClassCaptures` statement emitted at the
    // class's declaration position, which runs before any user code can
    // reference the class (TDZ).
    for sm in static_methods.iter_mut() {
        let mut id_map: std::collections::HashMap<LocalId, LocalId> =
            std::collections::HashMap::new();
        let mut prologue: Vec<Stmt> = Vec::new();
        for (index, &outer_id) in captures_vec.iter().enumerate() {
            let new_id = ctx.fresh_local();
            id_map.insert(outer_id, new_id);
            prologue.push(Stmt::Let {
                id: new_id,
                name: crate::cap_fields::cap_field_name(cap_salt, outer_id),
                ty: captured_outer_types
                    .get(&outer_id)
                    .cloned()
                    .unwrap_or(Type::Any),
                mutable: true,
                init: Some(Expr::ClassCaptureValue {
                    class_name: name.to_string(),
                    index: index as u32,
                    fallback: None,
                    prefer_fallback: false,
                }),
            });
        }
        crate::analysis::remap_local_ids_in_stmts(&mut sm.body, &id_map);
        prologue.append(&mut sm.body);
        sm.body = prologue;
        append_self_sites(&mut sm.body, &id_map);
    }

    // 2c. STATIC computed methods (`static [k]() {}`, and the static methods
    // bundlers emit as computed members — e.g. next's `NextResponse.json` /
    // `redirect` / `rewrite` / `next`). These get the SAME decl-site snapshot
    // rebind as the plain static methods in 2b. Previously they were skipped
    // entirely: a `new <Self>(…)` inside such a method (`return new
    // NextResponse(response.body, response)`) never had the constructor's
    // trailing `__perry_cap_*` args appended, so `inline_constructor_param_values`
    // mis-split the user args into the capture slots and the constructor read
    // its captures (here the `INTERNALS` symbol) as the uninitialized/garbage
    // tail — segfaulting when that garbage was a fetch handle keyed into a
    // property set. Refs #5199 (next/server NextResponse.json SIGSEGV).
    for member in computed_members
        .iter_mut()
        .filter(|member| member.is_static)
    {
        let mut id_map: std::collections::HashMap<LocalId, LocalId> =
            std::collections::HashMap::new();
        let mut prologue: Vec<Stmt> = Vec::new();
        for (index, &outer_id) in captures_vec.iter().enumerate() {
            let new_id = ctx.fresh_local();
            id_map.insert(outer_id, new_id);
            prologue.push(Stmt::Let {
                id: new_id,
                name: crate::cap_fields::cap_field_name(cap_salt, outer_id),
                ty: captured_outer_types
                    .get(&outer_id)
                    .cloned()
                    .unwrap_or(Type::Any),
                mutable: true,
                init: Some(Expr::ClassCaptureValue {
                    class_name: name.to_string(),
                    index: index as u32,
                    fallback: None,
                    prefer_fallback: false,
                }),
            });
        }
        crate::analysis::remap_local_ids_in_stmts(&mut member.function.body, &id_map);
        prologue.append(&mut member.function.body);
        member.function.body = prologue;
        append_self_sites(&mut member.function.body, &id_map);
    }

    // 3. Constructor.
    //
    // Issue #4972: when the class has heritage and NO user-written ctor,
    // the synthesized capture-stashing ctor must open with `super()` —
    // mirroring the spec default ctor `constructor(...args) {
    // super(...args) }`. Without it, codegen's static derived-ctor
    // TDZ check (`new.rs`: own ctor + heritage + no `super()` call ⇒
    // unconditional "Must call super constructor" throw) fires for a
    // class the user never wrote a ctor for — `class FakeAgent extends
    // http.Agent { createConnection() { new Duplex() } }` threw at
    // `new FakeAgent()` purely because the captured `Duplex` binding
    // forced a ctor into existence. The SuperCall also routes known
    // user-class parents through the inline-parent-ctor arm so the
    // parent body runs, matching the no-own-ctor `new` path.
    let mut ctor = match constructor.take() {
        Some(c) => c,
        None => {
            // #5957: synthesize the REAL spec default ctor,
            // `constructor(...args) { super(...args) }`. The previous
            // fixed-arity approximation walked the nearest pending-ancestor
            // ctor's USER arity and minted that many positional params — a
            // REST-param ancestor counted as arity 1 (`new Drain("a","b","c")`
            // forwarded only "a"), and an extends-EXPR / non-pending parent
            // walked to arity 0 (ALL user args dropped: the #806 mixin's
            // `seed` was undefined). One rest param + `SuperCallSpread`
            // forwards everything for every parent shape; the spread-super
            // dispatchers split user/cap slots by the registered signature cap
            // count and pack ancestor rest params via the closure-rest table.
            let (params, body) = if has_heritage {
                let pid = ctx.fresh_local();
                let params = vec![Param {
                    id: pid,
                    name: "__perry_dflt_args".to_string(),
                    ty: Type::Any,
                    default: None,
                    decorators: Vec::new(),
                    is_rest: true,
                    arguments_object: None,
                }];
                let body = vec![Stmt::Expr(Expr::SuperCallSpread(vec![CallArg::Spread(
                    Expr::LocalGet(pid),
                )]))];
                (params, body)
            } else {
                (Vec::new(), Vec::new())
            };
            Function {
                id: ctx.fresh_func(),
                name: format!("{}::constructor", name),
                type_params: Vec::new(),
                params,
                return_type: Type::Void,
                body,
                is_async: false,
                is_generator: false,
                is_strict: true,
                was_plain_async: false,
                was_unrolled: false,
                is_exported: false,
                captures: Vec::new(),
                decorators: Vec::new(),
            }
        }
    };
    let mut ctor_id_map: std::collections::HashMap<LocalId, LocalId> =
        std::collections::HashMap::new();
    // #5437 (cross-module member-`new`): recover each capture from the
    // class's decl-site snapshot at ctor entry. A SAME-module bare-`new
    // C(...)` / inline construct appends the live cap arg, so the param
    // holds the correct value and — because the snapshot for this class was
    // registered with that same value at decl-time —
    // `js_class_capture_value_or` returns the identical value (no behavior
    // change). But a CROSS-MODULE `new ns.C(...)` (Next's `new
    // w.AppRouteRouteModule(...)`, where the class lives in a different
    // compiled module) cannot resolve the class statically, so it routes to
    // the runtime construct path (`construct_registered_class_ref`) which
    // supplies NO cap args — the param is then garbage/undefined. Rebinding
    // the param FROM the class's own decl-site snapshot (the ctor body is
    // compiled in the class's home module, where `class_name` → its real
    // `class_id`) recovers the captured value before EITHER the user ctor
    // body reads it (`this.methods = r_(e)` — `r_` is remapped to this param)
    // OR the `this.__perry_cap_*` field is stashed from it. This generalizes
    // the W6 inline-construct snapshot fix
    // (`inline_constructor_param_values_with_class`) — which only covered the
    // statically-inlined construct — to EVERY construction path, including
    // the runtime cross-module one.
    let mut rebind_stmts: Vec<Stmt> = Vec::with_capacity(captures_vec.len());
    let mut assignment_stmts: Vec<Stmt> = Vec::with_capacity(captures_vec.len());
    for (index, &outer_id) in captures_vec.iter().enumerate() {
        let fresh_param_id = ctx.fresh_local();
        ctor_id_map.insert(outer_id, fresh_param_id);
        let ty = captured_outer_types
            .get(&outer_id)
            .cloned()
            .unwrap_or(Type::Any);
        ctor.params.push(Param {
            id: fresh_param_id,
            name: crate::cap_fields::cap_field_name(cap_salt, outer_id),
            ty,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        });
        // param = js_param_or_class_capture_value(param, class_id, slot)
        // — PARAM-FIRST: the live `new`-site cap arg wins whenever present; the
        // decl-site snapshot is only used when the param is `undefined` (the
        // cross-module construct path drops the cap arg → undefined). This
        // avoids overriding a SAME-module `new C(...)`'s current (possibly
        // mutated) outer with the stale decl-site snapshot.
        rebind_stmts.push(Stmt::Expr(Expr::LocalSet(
            fresh_param_id,
            Box::new(Expr::ClassCaptureValue {
                class_name: name.to_string(),
                index: index as u32,
                fallback: Some(Box::new(Expr::LocalGet(fresh_param_id))),
                prefer_fallback: true,
            }),
        )));
        assignment_stmts.push(Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: crate::cap_fields::cap_field_name(cap_salt, outer_id),
            value: Box::new(Expr::LocalGet(fresh_param_id)),
        }));
    }
    // Rewrite user-written ctor body BEFORE inserting the rebind + assignment
    // stmts (which already reference the fresh ids directly).
    crate::analysis::remap_local_ids_in_stmts(&mut ctor.body, &ctor_id_map);
    append_self_sites(&mut ctor.body, &ctor_id_map);
    // Finding #2: the param REBINDS (`param = param-or-snapshot`) go at
    // FUNCTION ENTRY (index 0), BEFORE any pre-`super()` user code — a derived
    // ctor may read a captured outer before calling `super()`, and that read
    // must already see the recovered value. Only the `this.__perry_cap_* =
    // param` field STASHES must wait until after `super()` (no `this` exists
    // before super) AND after all user stmts (see comment below).
    let rebind_count = rebind_stmts.len();
    for (i, stmt) in rebind_stmts.into_iter().enumerate() {
        ctor.body.insert(i, stmt);
    }
    // The field STASHES (`this.__perry_cap_* = param`) must come AFTER
    // `super()` (no `this` exists before super in derived classes) AND after
    // ALL user body stmts — user code may mutate the outer-local after
    // `super()` (e.g. `++called` in the TemporalHelpers sub-check pattern).
    // Inserting immediately after `super()` captured the pre-mutation value.
    // Insert stashes before each explicit `return` so they run on every exit
    // path, then append at the end for the fall-through path.
    //
    // BUT the stashes must ALSO run right after `super()` (or at entry for a
    // non-derived ctor): the ctor body may invoke instance methods
    // (`this.has = this.getHas()`), and a method reading a captured outer
    // resolves it through the `this.__perry_cap_*` field. Stashing only at
    // the end left those reads undefined — Next.js's base `Server`
    // constructor calls `this.getHasStaticDir()`, which reads the
    // module-level `_fs` interop binding via its cap field and threw
    // "Cannot read properties of undefined (reading 'default')" at boot
    // (#5437). So stash EARLY for intra-ctor method calls AND re-stash at
    // the end / before returns so post-`super()` mutations still win in the
    // final state. The assignments are idempotent.
    let super_pos = ctor
        .body
        .iter()
        .position(|s| matches!(s, Stmt::Expr(Expr::SuperCall(_) | Expr::SuperCallSpread(_))));
    let early_insert_at = super_pos.map(|p| p + 1).unwrap_or(rebind_count);
    for (i, stmt) in assignment_stmts.iter().cloned().enumerate() {
        ctor.body.insert(early_insert_at + i, stmt);
    }
    insert_stashes_before_returns(&mut ctor.body, &assignment_stmts);
    for stmt in assignment_stmts {
        ctor.body.push(stmt);
    }
    *constructor = Some(ctor);

    // Issue #740: rewrite field initializers and computed-key
    // expressions using the same `ctor_id_map`. Field initializers
    // are lowered inside the constructor body by
    // `apply_field_initializers_recursive`, so `LocalGet(outer_id)`
    // inside a field's init must be rewritten to read the fresh
    // ctor-local param that holds the captured value (synthesized
    // above). The ctor param is bound at every `new X(...)` call
    // site by `Expr::New`'s capture-args appending logic.
    for field in fields.iter_mut() {
        if let Some(init) = field.init.as_mut() {
            crate::analysis::remap_local_ids_in_expr(init, &ctor_id_map);
        }
        if let Some(key) = field.key_expr.as_mut() {
            crate::analysis::remap_local_ids_in_expr(key, &ctor_id_map);
        }
    }

    // 4. Register so `Expr::New { class_name }` appends
    //    `LocalGet(outer_id)` per captured outer id at every
    //    construction site.
    ctx.register_class_captures(name.to_string(), captures_vec);
}

/// Recursively insert `stashes` immediately before every `Stmt::Return` in
/// `body` so that cap-field stash assignments run on EVERY early-exit path,
/// not just the fall-through.  Does not descend into nested function
/// expressions (closures defined inside the constructor) — only direct
/// control-flow of the constructor body itself.
fn insert_stashes_before_returns(body: &mut Vec<Stmt>, stashes: &[Stmt]) {
    let mut i = 0;
    while i < body.len() {
        // Check via shared ref first to decide what to do.
        let is_return = matches!(body[i], Stmt::Return(_));
        if is_return {
            for (j, s) in stashes.iter().enumerate() {
                body.insert(i + j, s.clone());
            }
            i += stashes.len() + 1;
            continue;
        }
        // Recurse into nested control-flow via mutable ref.
        match &mut body[i] {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                insert_stashes_before_returns(then_branch, stashes);
                if let Some(eb) = else_branch {
                    insert_stashes_before_returns(eb, stashes);
                }
            }
            Stmt::While { body: wb, .. } | Stmt::DoWhile { body: wb, .. } => {
                insert_stashes_before_returns(wb, stashes);
            }
            Stmt::For { body: fb, .. } => {
                insert_stashes_before_returns(fb, stashes);
            }
            Stmt::Labeled { body: lb, .. } => match lb.as_mut() {
                Stmt::While { body: wb, .. }
                | Stmt::DoWhile { body: wb, .. }
                | Stmt::For { body: wb, .. } => {
                    insert_stashes_before_returns(wb, stashes);
                }
                _ => {}
            },
            Stmt::Try {
                body: tb,
                catch,
                finally,
            } => {
                insert_stashes_before_returns(tb, stashes);
                if let Some(c) = catch {
                    insert_stashes_before_returns(&mut c.body, stashes);
                }
                if let Some(f) = finally {
                    insert_stashes_before_returns(f, stashes);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    insert_stashes_before_returns(&mut case.body, stashes);
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Append `cap_args` (the `.1` ids) to every `new <class_name>(…)` site in
/// `expr`, descending nested closures (patching their capture lists when the
/// appended id is otherwise unreferenced). With `skip_if_present`, a site
/// whose args already END with exactly the `.1` id sequence is left alone —
/// used by the post-body pass, where sites lowered AFTER the class
/// registered already carry the appends.
pub(crate) fn append_new_args_expr(
    expr: &mut Expr,
    class_name: &str,
    cap_args: &[(LocalId, LocalId)],
    skip_if_present: bool,
) {
    if let Expr::New {
        class_name: cn,
        args,
        ..
    } = expr
    {
        if cn == class_name {
            let already = skip_if_present
                && args.len() >= cap_args.len()
                && args[args.len() - cap_args.len()..]
                    .iter()
                    .zip(cap_args.iter())
                    .all(|(a, (_, fresh))| matches!(a, Expr::LocalGet(id) if id == fresh));
            if !already {
                for (_, fresh) in cap_args {
                    args.push(Expr::LocalGet(*fresh));
                }
            }
        }
    }
    if let Expr::Closure { body, captures, .. } = expr {
        for stmt in body.iter_mut() {
            append_new_args_stmt(stmt, class_name, cap_args, skip_if_present);
        }
        let mut refs = Vec::new();
        let mut visited = std::collections::HashSet::new();
        for stmt in body.iter() {
            crate::analysis::collect_local_refs_stmt(stmt, &mut refs, &mut visited);
        }
        for (_, fresh) in cap_args {
            if refs.contains(fresh) && !captures.contains(fresh) {
                captures.push(*fresh);
            }
        }
        return;
    }
    crate::walker::walk_expr_children_mut(expr, &mut |child| {
        append_new_args_expr(child, class_name, cap_args, skip_if_present)
    });
}

/// Statement-level driver for [`append_new_args_expr`].
pub(crate) fn append_new_args_stmt(
    stmt: &mut Stmt,
    class_name: &str,
    cap_args: &[(LocalId, LocalId)],
    skip_if_present: bool,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                append_new_args_expr(e, class_name, cap_args, skip_if_present);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => {
            append_new_args_expr(e, class_name, cap_args, skip_if_present)
        }
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                append_new_args_expr(e, class_name, cap_args, skip_if_present);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            append_new_args_expr(condition, class_name, cap_args, skip_if_present);
            for s in then_branch {
                append_new_args_stmt(s, class_name, cap_args, skip_if_present);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    append_new_args_stmt(s, class_name, cap_args, skip_if_present);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            append_new_args_expr(condition, class_name, cap_args, skip_if_present);
            for s in body {
                append_new_args_stmt(s, class_name, cap_args, skip_if_present);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                append_new_args_stmt(s, class_name, cap_args, skip_if_present);
            }
            if let Some(e) = condition {
                append_new_args_expr(e, class_name, cap_args, skip_if_present);
            }
            if let Some(e) = update {
                append_new_args_expr(e, class_name, cap_args, skip_if_present);
            }
            for s in body {
                append_new_args_stmt(s, class_name, cap_args, skip_if_present);
            }
        }
        Stmt::Labeled { body, .. } => {
            append_new_args_stmt(body, class_name, cap_args, skip_if_present)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                append_new_args_stmt(s, class_name, cap_args, skip_if_present);
            }
            if let Some(c) = catch {
                for s in &mut c.body {
                    append_new_args_stmt(s, class_name, cap_args, skip_if_present);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    append_new_args_stmt(s, class_name, cap_args, skip_if_present);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            append_new_args_expr(discriminant, class_name, cap_args, skip_if_present);
            for c in cases {
                if let Some(t) = &mut c.test {
                    append_new_args_expr(t, class_name, cap_args, skip_if_present);
                }
                for s in &mut c.body {
                    append_new_args_stmt(s, class_name, cap_args, skip_if_present);
                }
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
    }
}
