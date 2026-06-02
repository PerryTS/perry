use anyhow::{anyhow, bail, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::analysis::*;
use crate::destructuring::*;
use crate::ir::*;
use crate::lower::{
    collect_for_of_pattern_leaves, emit_for_of_pattern_binding, lower_expr, LoweringContext,
};
use crate::lower_patterns::*;
use crate::lower_types::*;

use super::class_members::collect_method_captures;
use super::class_validation::*;
use super::enum_decl::*;
use super::fn_decl::*;
use super::helpers::*;
use super::interface_decl::*;
use super::private_members::*;
use super::type_alias::*;
use super::*;

pub fn synthesize_class_captures(
    ctx: &mut LoweringContext,
    name: &str,
    extends_name: Option<&str>,
    fields: &mut Vec<ClassField>,
    methods: &mut Vec<Function>,
    getters: &mut Vec<(String, Function)>,
    setters: &mut Vec<(String, Function)>,
    computed_members: &mut Vec<ClassComputedMember>,
    constructor: &mut Option<Function>,
) {
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
    for member in computed_members.iter().filter(|member| !member.is_static) {
        for id in collect_method_captures(&member.function, &outer_scope_ids, &module_level_ids) {
            union_captures.insert(id);
        }
    }
    if let Some(ctor) = constructor.as_ref() {
        for id in collect_method_captures(ctor, &outer_scope_ids, &module_level_ids) {
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
        .filter(|cid| inherited_cap_field_names.contains(&format!("__perry_cap_{}", cid)))
        .collect();

    // 1. Hidden fields keyed by outer id, skipping inherited.
    for &cid in &captures_vec {
        if inherited_cap_ids.contains(&cid) {
            continue;
        }
        fields.push(ClassField {
            name: format!("__perry_cap_{}", cid),
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
            let field_name = format!("__perry_cap_{}", cid);
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
        .map(|&cid| (cid, format!("__perry_cap_{}", cid)))
        .collect();

    // Helper closure: build a fresh-id map for one function's body,
    // rewrite the body refs (with field-write propagation), and
    // prepend the rebinding lets.
    let rewrite_method_body = |ctx: &mut LoweringContext, body: &mut Vec<Stmt>| {
        let mut id_map: std::collections::HashMap<LocalId, LocalId> =
            std::collections::HashMap::new();
        let mut prologue: Vec<Stmt> = Vec::new();
        for &outer_id in &captures_vec {
            let new_id = ctx.fresh_local();
            id_map.insert(outer_id, new_id);
            let ty = captured_outer_types
                .get(&outer_id)
                .cloned()
                .unwrap_or(Type::Any);
            prologue.push(Stmt::Let {
                id: new_id,
                name: format!("__perry_cap_{}", outer_id),
                ty,
                mutable: true,
                init: Some(Expr::PropertyGet {
                    object: Box::new(Expr::This),
                    property: format!("__perry_cap_{}", outer_id),
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
    };

    // 2. Methods / getters / setters.
    for m in methods.iter_mut() {
        rewrite_method_body(ctx, &mut m.body);
    }
    for (_, g) in getters.iter_mut() {
        rewrite_method_body(ctx, &mut g.body);
    }
    for (_, s) in setters.iter_mut() {
        rewrite_method_body(ctx, &mut s.body);
    }
    for member in computed_members
        .iter_mut()
        .filter(|member| !member.is_static)
    {
        rewrite_method_body(ctx, &mut member.function.body);
    }

    // 3. Constructor.
    let mut ctor = constructor.take().unwrap_or_else(|| Function {
        id: ctx.fresh_func(),
        name: format!("{}::constructor", name),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Void,
        body: Vec::new(),
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    });
    let mut ctor_id_map: std::collections::HashMap<LocalId, LocalId> =
        std::collections::HashMap::new();
    let mut assignment_stmts: Vec<Stmt> = Vec::with_capacity(captures_vec.len());
    for &outer_id in &captures_vec {
        let fresh_param_id = ctx.fresh_local();
        ctor_id_map.insert(outer_id, fresh_param_id);
        let ty = captured_outer_types
            .get(&outer_id)
            .cloned()
            .unwrap_or(Type::Any);
        ctor.params.push(Param {
            id: fresh_param_id,
            name: format!("__perry_cap_{}", outer_id),
            ty,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
        });
        assignment_stmts.push(Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: format!("__perry_cap_{}", outer_id),
            value: Box::new(Expr::LocalGet(fresh_param_id)),
        }));
    }
    // Rewrite user-written ctor body BEFORE inserting the assignment
    // stmts (which already reference the fresh ids directly).
    crate::analysis::remap_local_ids_in_stmts(&mut ctor.body, &ctor_id_map);
    let super_pos = ctor
        .body
        .iter()
        .position(|s| matches!(s, Stmt::Expr(Expr::SuperCall(_))));
    let insert_at = super_pos.map(|p| p + 1).unwrap_or(0);
    for (i, stmt) in assignment_stmts.into_iter().enumerate() {
        ctor.body.insert(insert_at + i, stmt);
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
