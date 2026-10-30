//! Turning HIR shapes into candidate sites: the `Object.create(P)` creation
//! points (`create_site`), the `Object.assign` adds (`add_site`), the holder
//! reads (`holder_read`), the literal-keyed adds (`literal_add_site`), and the
//! `site` entry point that decides which of them an expression is. Collecting
//! facts and following bindings is `collect`'s job, not this module's.
use super::*;
use super::emission::use_layout;

/// The family whose birth the `Object.create` expression `e` is: the
/// pending creation point's, when `e` is its initializer. Its layouts are
/// used from here on, and the binding's stores find it.
pub(crate) fn create_site(ctx: &mut FnCtx<'_>, e: &Expr) -> Option<Create> {
    let Expr::ObjectCreate(proto, None) = e else {
        return None;
    };
    let p = proto_binding(proto)?;
    if ctx
        .create_families
        .pending
        .as_ref()
        .is_none_or(|(_, f)| f.proto != p)
    {
        return None;
    }
    let (a, family) = ctx.create_families.pending.take()?;
    for (i, desc) in family.states.iter().enumerate() {
        use_layout(Site {
            desc_global: desc.clone(),
            keys: family.keys[..i].to_vec(),
            slots: family.keys.len() as u32,
            proto_global: family.proto_global.clone(),
        });
    }
    ctx.create_families.born.insert(a, family.clone());
    Some(family)
}

/// The family add `object.property = …` names, when `object` is a binding
/// born of a family that holds `property`.
pub(crate) fn add_site(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> Option<Add> {
    let Expr::LocalGet(a) = object else {
        return None;
    };
    let family = ctx.create_families.born.get(a)?;
    let index = family.keys.iter().position(|k| k == property)?;
    Some(Add {
        family: family.clone(),
        index,
    })
}

/// The [`HolderRead`] of `object.property`, when `object` is a binding
/// born of a family whose chain's carried layouts hold `property` as data.
pub(crate) fn holder_read(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> Option<HolderRead> {
    let Expr::LocalGet(a) = object else {
        return None;
    };
    let family = ctx.create_families.born.get(a)?;
    if property == "__proto__" || family.keys.iter().any(|k| k == property) {
        return None;
    }
    let key = ImageKey::Str(property.to_string());
    MODULE_CREATE_FAMILIES.with(|m| {
        let m = m.borrow();
        if !m.checked {
            return None;
        }
        let mut chain = Vec::new();
        for h in chain_of(family.proto, &m.borns)? {
            let born = &m.borns[&h];
            // Emitted with its descriptor (`emission`), and born there.
            let linked = match born.proto {
                Link::Binding(q) => m.bound.contains(&q),
                Link::Object | Link::Null => true,
            };
            if !linked || !m.bound.contains(&h) || !ctx.module_globals.contains_key(&h) {
                return None;
            }
            let image = born.image.as_ref()?;
            chain.push((h, image_global(&m.prefix, h)));
            if let Some(pos) = image.iter().position(|e| e.key == key) {
                if image[pos].attrs & ACCESSOR != 0 {
                    return None;
                }
                return Some(HolderRead {
                    states: family.states.clone(),
                    chain,
                    slot: pos as u32,
                });
            }
        }
        None
    })
}

/// The family of `Object.assign(Object.create(P), { k1: e1, … })`, when `P`
/// is bound to a closed literal and every key's add is free.
/// The layout of `Object.assign(Object.create(P), new Literal(..))` when `P`
/// is a closed literal and the literal's keys are free adds.
pub(super) fn literal_add_site(
    m: &ModuleCreateFamilies,
    p: u32,
    class: &perry_hir::Class,
    arg_count: usize,
) -> Option<Site> {
    let keys: Vec<String> = class.fields.iter().map(|f| f.name.clone()).collect();
    if keys.is_empty()
        || keys.len() != arg_count
        || class.fields.iter().any(|f| f.key_expr.is_some())
        || !keys.iter().all(|k| layout_key(k))
        || (1..keys.len()).any(|i| keys[..i].contains(&keys[i]))
        || !keys.iter().all(|k| add_free(k))
    {
        return None;
    }
    m.literals.contains(&p).then(|| Site {
        desc_global: format!(
            "perry_layout_create_{}_{p}_{:08x}",
            m.prefix,
            super::super::fnv1a(keys.join("\u{1}").as_bytes()) as u32
        ),
        slots: keys.len() as u32,
        keys,
        proto_global: format!("perry_protofam_{}_{p}", m.prefix),
    })
}

pub(crate) fn site(ctx: &FnCtx<'_>, e: &Expr) -> Option<Site> {
    let Expr::ObjectAssign { target, sources } = e else {
        return None;
    };
    let Expr::ObjectCreate(proto, None) = target.as_ref() else {
        return None;
    };
    let p = proto_binding(proto)?;
    let [source] = sources.as_slice() else {
        return None;
    };
    let Expr::New {
        class_name, args, ..
    } = source
    else {
        return None;
    };
    let class = ctx
        .classes
        .get(class_name)
        .filter(|c| c.is_literal_shape())?;
    let site =
        MODULE_CREATE_FAMILIES.with(|m| literal_add_site(&m.borrow(), p, class, args.len()))?;
    use_layout(site.clone());
    Some(site)
}
