//! Complete candidates for lazy declaration holders. No object is built here.
use super::{BirthProto, BirthShape, ConstFnBirth, ModuleBirth};
use std::collections::{HashMap, HashSet};

pub(crate) fn collect(
    module: &perry_hir::Module,
    prefix: &str,
    ids: &HashMap<String, u32>,
) -> Vec<ModuleBirth> {
    let fresh = super::fresh_class_templates::fresh_class_templates(module);
    module
        .classes
        .iter()
        .filter_map(|c| {
            if c.is_imported_stub()
                || c.is_literal_shape()
                || c.is_nested
                || fresh.contains(&c.name)
                || c.specialized_from.is_some()
                || c.extends_expr.is_some()
                || c.native_extends.is_some()
                || !c.computed_members.is_empty()
                || !c.decorators.is_empty()
                || c.methods
                    .iter()
                    .any(|m| !m.decorators.is_empty() || m.name.starts_with('#'))
            {
                return None;
            }
            let cid = *ids.get(&c.name)?;
            let parent = match &c.extends_name {
                Some(n) => *ids.get(n)?,
                None => 0,
            };
            let mut members = std::collections::BTreeMap::new();
            for m in &c.methods {
                if members
                    .insert(m.name.clone(), (m.id, Some(m), 2u8))
                    .is_some()
                {
                    return None;
                }
            }
            for (setter, accessors) in [(false, &c.getters), (true, &c.setters)] {
                for (name, function) in accessors {
                    if c.static_accessor_fn_ids.contains(&function.id) || name.starts_with('#') {
                        continue;
                    }
                    if !function.decorators.is_empty() {
                        return None;
                    }
                    let entry = members
                        .entry(name.clone())
                        .or_insert((function.id, None, 10));
                    if entry.1.is_some() {
                        return None;
                    }
                    entry.0 = entry.0.min(function.id);
                    let half = if setter { 32 } else { 16 };
                    if entry.2 & half != 0 {
                        return None;
                    }
                    entry.2 |= half;
                }
            }
            let mut members: Vec<_> = members.into_iter().collect();
            members.sort_by_key(|(_, (order, _, _))| *order);
            let mut keys = b"constructor\0".to_vec();
            let mut attrs = vec![2];
            let mut constfn = Vec::new();
            let mut rep = 0;
            for (i, (name, (_, method, attr))) in members.iter().enumerate() {
                if name.is_empty() || name.contains('\0') || name == "constructor" {
                    return None;
                }
                keys.extend_from_slice(name.as_bytes());
                keys.push(0);
                attrs.push(*attr);
                let slot = i + 1;
                if let Some(m) = method.filter(|_| slot < 32) {
                    rep |= 3u64 << (2 * slot);
                    let body = format!(
                        "perry_method_{}__{}__{}__eclo",
                        prefix,
                        super::helpers::sanitize_member(&c.name),
                        super::helpers::sanitize_member(&m.name)
                    );
                    constfn.push(ConstFnBirth {
                        slot: slot as u8,
                        symbol: crate::fn_info::info_symbol(&body),
                    });
                }
            }
            Some(ModuleBirth {
                keys_global: format!("perry_decl_proto_{prefix}__{cid}"),
                class_id: 0,
                defined: false,
                shape: BirthShape {
                    keys,
                    key_count: members.len() as u32 + 1,
                    live: members.len() as u32 + 1,
                    proto: BirthProto::Prototype(cid, parent),
                    typed: None,
                    rep,
                    constfn,
                    private: Vec::new(),
                    brands: Vec::new(),
                    attrs,
                },
            })
        })
        .collect()
}

/// Every intermediate shape proves absence and the next prototype link.
/// The final holder shape proves the compiler's body candidate.
pub(crate) fn chain(
    ctx: &crate::expr::FnCtx<'_>,
    class: &str,
    method: &str,
) -> Option<Vec<(u32, u32)>> {
    let mut name = class.to_string();
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for _ in 0..64 {
        if !seen.insert(name.clone()) {
            return None;
        }
        let c = ctx.classes.get(&name)?;
        if let Some(origin) = &c.specialized_from {
            name = origin.clone();
            continue;
        }
        let cid = *ctx.class_ids.get(&name)?;
        let (id, shape) = super::static_shape_ids::static_prototype_shape(cid)?;
        result.push((cid, id));
        if let Some(slot) = shape
            .keys
            .split(|&b| b == 0)
            .take(shape.key_count as usize)
            .position(|k| k == method.as_bytes())
        {
            // Only ConstFn lanes prove a body; wide Any slots can change in place.
            return (shape
                .constfn
                .iter()
                .any(|entry| entry.slot as usize == slot)
                && c.methods
                    .iter()
                    .any(|m| m.name == method && !m.is_async && !m.is_generator))
            .then_some(result)
            .filter(|_| slot != 0);
        }
        name = c.extends_name.clone()?;
        if !matches!(shape.proto, BirthProto::Prototype(_, parent) if ctx.class_ids.get(&name) == Some(&parent))
        {
            return None;
        }
    }
    None
}

/// A learned inherited 5C entry can name its declaration holder P statically.
pub(crate) fn inherited_lane(
    ctx: &crate::expr::FnCtx<'_>,
    class: &str,
    property: &str,
) -> Option<super::static_constfn::StaticMethodLane> {
    let chain = chain(ctx, class, property)?;
    let &(cid, p) = chain.last()?;
    let (_, shape) = super::static_shape_ids::static_prototype_shape(cid)?;
    let name = ctx.class_ids.iter().find(|(_, id)| **id == cid)?.0;
    let declaration = ctx.classes.get(name)?;
    let method = declaration.methods.iter().find(|m| m.name == property)?;
    let key = (name.clone(), property.to_string());
    if method.is_async
        || method.is_generator
        || method
            .params
            .iter()
            .any(|p| p.is_rest || p.arguments_object.is_some())
        || ctx.method_has_rest.get(&key).copied().unwrap_or(false)
        || ctx
            .method_has_synthetic_arguments
            .get(&key)
            .copied()
            .unwrap_or(false)
    {
        return None;
    }
    let slot = shape
        .keys
        .split(|&b| b == 0)
        .position(|k| k == property.as_bytes())?;
    let lane = shape.constfn.iter().find(|e| e.slot as usize == slot)?;
    Some(super::static_constfn::StaticMethodLane {
        word: (u64::from(p) << 32) | u64::from(cid),
        slot: slot as u32,
        body: lane.symbol.strip_suffix("$info")?.to_string(),
        arity: *ctx.method_param_counts.get(&key)?,
        own: false,
    })
}
