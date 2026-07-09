//! Class-field layout / declared-type resolution via the inheritance chain.
//!
//! Split out of `type_analysis.rs` to keep that file under the file-size CI
//! gate. These helpers walk a class's `extends_name` chain to resolve a
//! field's declared type or its packed-slot index. Every walk here is
//! cycle-guarded: heavily-modular packages (Effect, OpenCode) declare
//! same-named classes across modules, and when those are pulled into one
//! importing module's class table by name the parent chains can form a
//! cycle — an unguarded walk would then CPU-hang or OOM. The walks bail on
//! a repeated class name (and a depth cap) instead.

use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::expr::FnCtx;
use crate::type_analysis::receiver_class_name;

pub(crate) fn declared_field_type(ctx: &FnCtx<'_>, object: &Expr, field: &str) -> Option<HirType> {
    let receiver_class = receiver_class_name(ctx, object)?;
    if let Some(class) = ctx.classes.get(&receiver_class) {
        if let Some(f) = class.fields.iter().find(|f| f.name == field) {
            return Some(f.ty.clone());
        }
        // Walk the inheritance chain. Guard against cyclic parent links so
        // the walk terminates.
        let mut parent = class.extends_name.as_deref();
        let mut seen_parent_names: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut parent_depth = 0usize;
        while let Some(p) = parent {
            parent_depth += 1;
            if parent_depth > 64 || !seen_parent_names.insert(p.to_string()) {
                break;
            }
            let Some(pc) = ctx.classes.get(p) else { break };
            if let Some(f) = pc.fields.iter().find(|f| f.name == field) {
                return Some(f.ty.clone());
            }
            parent = pc.extends_name.as_deref();
        }
        return None;
    }
    if let Some(iface) = ctx.interfaces.get(&receiver_class) {
        if let Some(p) = iface.properties.iter().find(|p| p.name == field) {
            return Some(p.ty.clone());
        }
        for ext in &iface.extends {
            if let HirType::Named(parent_name) = ext {
                if let Some(parent_iface) = ctx.interfaces.get(parent_name) {
                    if let Some(p) = parent_iface.properties.iter().find(|p| p.name == field) {
                        return Some(p.ty.clone());
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn class_field_global_index(
    ctx: &FnCtx<'_>,
    class_name: &str,
    property: &str,
) -> Option<u32> {
    // Walk parent chain to find the field. Parent fields come first in
    // the slot layout, so we sum parent counts as we descend.
    //
    // Refs #420: must skip computed-key fields (`[Symbol.X] = init`) when
    // counting positions — the inline-slot layout in `packed_keys` only
    // includes string-keyed fields. If we count computed-key fields here,
    // the index used for `this.config = {...}` writes shifts past where
    // readers look for "config", and every cross-module access reads from
    // an uninitialised slot (raw f64 zero, which presents as `number 0`
    // when treated as a NaN-boxed value). drizzle's `class ColumnBuilder
    // { config; $default = this.$defaultFn; $onUpdate = this.$onUpdateFn; }`
    // shape — where the `config;` declaration sits among method-ref class
    // fields — surfaces this as `column.config = 0` for every column
    // builder when read from the importing module.
    fn count_keyable(fields: &[perry_hir::ClassField]) -> u32 {
        fields.iter().filter(|f| f.key_expr.is_none()).count() as u32
    }
    fn accessor_in_chain(ctx: &FnCtx<'_>, class_name: &str, property: &str) -> bool {
        let mut current = Some(class_name.to_string());
        let mut seen_class_names: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut depth = 0usize;
        while let Some(name) = current {
            depth += 1;
            if depth > 64 || !seen_class_names.insert(name.clone()) {
                return true;
            }
            let Some(class) = ctx.classes.get(&name) else {
                return true;
            };
            if class.getters.iter().any(|(n, _)| n == property)
                || class.setters.iter().any(|(n, _)| n == property)
            {
                return true;
            }
            current = class.extends_name.clone();
        }
        false
    }
    // A getter/setter anywhere on the prototype chain owns this property name
    // for normal JS semantics. Do not emit direct packed-field access even if
    // HIR inferred a same-named field on a subclass assignment.
    if accessor_in_chain(ctx, class_name, property) {
        return None;
    }
    // Issue #26 / #321 (via #5654): prefer the authoritative, source-prefix-
    // disambiguated ancestor chain — the SAME data the packed-keys global and
    // constructor field-init are built from — so the returned index matches
    // the runtime keys array even when same-named cross-module classes collide
    // in the name-keyed `ctx.classes` (effect's `Type` in SchemaAST.ts vs
    // ParseResult.ts made `PropertySignature.isOptional` resolve to slot 4
    // instead of 2). The always-on guard call used to mask the wrong index at
    // runtime (`object_key_matches_field` missed → by-name fallback); with the
    // #5654 inline fast path live, the index must actually be correct.
    if let Some(chain) = ctx.class_init_chains.get(class_name) {
        // Slot layout is the chain's keyable fields, root → leaf. The
        // most-derived declaration wins (TS shadowing), so search leaf → root.
        let mut prefix_counts: Vec<u32> = Vec::with_capacity(chain.len());
        let mut acc = 0u32;
        for (_, fields) in chain {
            prefix_counts.push(acc);
            acc += count_keyable(fields);
        }
        for (i, (_, fields)) in chain.iter().enumerate().rev() {
            let mut own_idx = 0u32;
            for f in fields {
                if f.key_expr.is_some() {
                    continue;
                }
                if f.name == property {
                    return Some(prefix_counts[i] + own_idx);
                }
                own_idx += 1;
            }
        }
        // The chain is authoritative for this class's layout: absent means the
        // property is not an inline slot. Do NOT fall through to the name-keyed
        // walk — it could "find" the field on a wrong same-named stub.
        return None;
    }
    fn walk(
        ctx: &FnCtx<'_>,
        class_name: &str,
        property: &str,
        offset: u32,
        seen_class_names: &mut std::collections::HashSet<String>,
        depth: usize,
    ) -> Option<u32> {
        // Guard against cyclic parent links: same-named classes pulled
        // across modules can form an inheritance cycle, which would spin
        // this recursive walk (and the inner parent-count loop below)
        // indefinitely. Bail once a class repeats or the chain is absurdly
        // deep.
        if depth > 64 || !seen_class_names.insert(class_name.to_string()) {
            return None;
        }
        let class = ctx.classes.get(class_name)?;
        // Compute the byte-offset contribution from this class's parent.
        let parent_count = if let Some(parent_name) = class.extends_name.as_deref() {
            let mut p_count = 0u32;
            let mut p = Some(parent_name.to_string());
            let mut seen_parent_names: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut parent_depth = 0usize;
            while let Some(name) = p {
                parent_depth += 1;
                if parent_depth > 64 || !seen_parent_names.insert(name.clone()) {
                    return None; // cyclic parent chain — no inline path
                }
                if let Some(parent) = ctx.classes.get(&name) {
                    p_count += count_keyable(&parent.fields);
                    p = parent.extends_name.clone();
                } else {
                    return None; // unresolvable parent — no inline path
                }
            }
            p_count
        } else {
            0
        };
        // Look for the field on this class first (the most-derived
        // declaration shadows parents in TypeScript). Position within the
        // own-fields list must skip computed-key entries to match the
        // packed_keys layout the runtime sees.
        let mut own_idx: u32 = 0;
        for f in &class.fields {
            if f.key_expr.is_some() {
                continue;
            }
            if f.name == property {
                return Some(offset + parent_count + own_idx);
            }
            own_idx += 1;
        }
        // Otherwise walk into the parent chain looking for the field.
        if let Some(parent_name) = class.extends_name.as_deref() {
            return walk(
                ctx,
                parent_name,
                property,
                offset,
                seen_class_names,
                depth + 1,
            );
        }
        None
    }
    let mut seen_class_names = std::collections::HashSet::new();
    walk(ctx, class_name, property, 0, &mut seen_class_names, 0)
}

pub(crate) fn class_field_declared_type(
    ctx: &FnCtx<'_>,
    class_name: &str,
    property: &str,
) -> Option<HirType> {
    // Issue #26 / #321 (via #5654): same authoritative-chain preference as
    // `class_field_global_index` — the declared type gates the raw-f64 slot
    // contract, so it must come from the class that actually owns the slot,
    // not a same-named cross-module impostor from the name-keyed table.
    if let Some(chain) = ctx.class_init_chains.get(class_name) {
        for (_, fields) in chain.iter().rev() {
            if let Some(field) = fields
                .iter()
                .find(|field| field.key_expr.is_none() && field.name == property)
            {
                return Some(field.ty.clone());
            }
        }
        return None;
    }
    let mut current = ctx.classes.get(class_name).copied();
    // Guard against cyclic parent links so the walk terminates.
    let mut seen_class_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    while let Some(cls) = current {
        if !seen_class_names.insert(cls.name.clone()) {
            break;
        }
        if let Some(field) = cls
            .fields
            .iter()
            .find(|field| field.key_expr.is_none() && field.name == property)
        {
            return Some(field.ty.clone());
        }
        current = cls
            .extends_name
            .as_deref()
            .and_then(|parent| ctx.classes.get(parent).copied());
    }
    None
}
