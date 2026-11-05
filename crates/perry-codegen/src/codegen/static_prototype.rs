//! Complete candidates for lazy declaration holders. No object is built here.
use super::{BirthProto, BirthShape, ConstFnBirth, ModuleBirth};
use std::collections::{HashMap, HashSet};

pub(crate) fn collect(
    module: &perry_hir::Module,
    prefix: &str,
    ids: &HashMap<String, u32>,
    layouts: &crate::layout_families::ProgramLayouts,
) -> Vec<ModuleBirth> {
    let fresh = super::fresh_class_templates::fresh_class_templates(module);
    // An admitted family's nonzero parent names a declaration in this
    // program. The complete module lists all of its own definitions, so
    // absence here proves that the parent belongs to another module.
    let local_definitions: HashSet<u32> = module
        .classes
        .iter()
        .filter(|c| !c.is_imported_stub())
        .filter_map(|c| ids.get(&c.name).copied())
        .collect();
    let mut refused = std::collections::BTreeMap::<&str, u32>::new();
    let mut evaluated = 0;
    let births = module
        .classes
        .iter()
        .filter_map(|c| {
            let candidate = (|| {
                let canonical_once = ids
                    .get(&c.name)
                    .is_some_and(|cid| layouts.run_once_evaluated.contains(cid));
                let refused = if c.is_imported_stub() {
                    Some("imported-stub")
                } else if c.is_literal_shape() {
                    Some("literal")
                } else if c.is_nested && !canonical_once {
                    Some("nested")
                } else if fresh.contains(&c.name) && !canonical_once {
                    Some("fresh-evaluation")
                } else if c.specialized_from.is_some() {
                    Some("specialized")
                } else if c.native_extends.is_some() {
                    Some("native-parent")
                } else if c.has_evaluated_members() {
                    Some("computed-members")
                } else if c.computed_members.iter().any(|m| m.is_static)
                    || (fresh.contains(&c.name) && !c.computed_members.is_empty())
                {
                    // A known symbol key is a birth-shape key of a declared
                    // instance member only (`declared_symbol_member`).
                    Some("known-symbol-member")
                } else if !c.decorators.is_empty() {
                    Some("decorated-class")
                } else if c
                    .methods
                    .iter()
                    .any(|m| !m.decorators.is_empty() || m.name.starts_with('#'))
                {
                    Some("decorated-or-private-method")
                } else {
                    None
                };
                if let Some(reason) = refused {
                    if std::env::var("PERRY_LAYOUT_REPORT").as_deref() == Ok("2") {
                        eprintln!("perry: static prototype refused: {}: {reason}", c.name);
                    }
                    return Err(reason);
                }
                let cid = *ids.get(&c.name).ok_or("class-id")?;
                // Evaluated heritage keeps its runtime semantics. The admitted
                // family supplies only a candidate parent; prototype birth must
                // prove the actual parent and its prototype identity before it
                // can request this static shape.
                let parent = if c.extends_expr.is_some() {
                    let parent = layouts
                        .families
                        .get(&cid)
                        .ok_or("evaluated-no-family")?
                        .parent_class_id;
                    if parent == 0 || local_definitions.contains(&parent) {
                        return Err("evaluated-local-parent");
                    }
                    parent
                } else {
                    match &c.extends_name {
                        Some(n) => *ids.get(n).ok_or("unresolved-parent")?,
                        None => 0,
                    }
                };
                let mut members = std::collections::BTreeMap::new();
                for m in &c.methods {
                    if members
                        .insert(m.name.clone(), (m.id, Some(m), 2u8))
                        .is_some()
                    {
                        return Err("member-content");
                    }
                }
                for (setter, accessors) in [(false, &c.getters), (true, &c.setters)] {
                    for (name, function) in accessors {
                        if c.static_accessor_fn_ids.contains(&function.id) || name.starts_with('#')
                        {
                            continue;
                        }
                        if !function.decorators.is_empty() {
                            return Err("member-content");
                        }
                        let entry = members
                            .entry(name.clone())
                            .or_insert((function.id, None, 10));
                        if entry.1.is_some() {
                            return Err("member-content");
                        }
                        entry.0 = entry.0.min(function.id);
                        let half = if setter { 32 } else { 16 };
                        if entry.2 & half != 0 {
                            return Err("member-content");
                        }
                        entry.2 |= half;
                    }
                }
                let mut members: Vec<_> = members.into_iter().collect();
                members.sort_by_key(|(_, (order, _, _))| *order);
                // Known-symbol members follow the string members (the
                // runtime's born-final order: OrdinaryOwnPropertyKeys lists
                // symbols after strings), each by its first definition.
                let mut symbols: Vec<(String, (u32, Option<&perry_hir::ClassComputedMember>, u8))> =
                    Vec::new();
                for m in &c.computed_members {
                    let key = m.known_symbol().ok_or("computed-members")?;
                    let spelling = symbol_key_spelling(key).ok_or("member-content")?;
                    let (method, half) = match m.kind {
                        perry_hir::ClassComputedMemberKind::Method => (true, 0),
                        perry_hir::ClassComputedMemberKind::Getter => (false, 16),
                        perry_hir::ClassComputedMemberKind::Setter => (false, 32),
                    };
                    match symbols.iter_mut().find(|(k, _)| *k == spelling) {
                        None => symbols.push((
                            spelling,
                            (m.function.id, method.then_some(m), if method { 2 } else { 10 | half }),
                        )),
                        // A redefined symbol member takes the general path.
                        Some((_, entry)) => {
                            if method || entry.1.is_some() || entry.2 & half != 0 {
                                return Err("member-content");
                            }
                            entry.0 = entry.0.min(m.function.id);
                            entry.2 |= half;
                        }
                    }
                }
                symbols.sort_by_key(|(_, (order, _, _))| *order);
                let string_count = members.len();
                let mut keys = b"constructor\0".to_vec();
                let mut attrs = vec![2];
                let mut constfn = Vec::new();
                let mut rep = 0;
                for (i, (name, (_, method, attr))) in members.iter().enumerate() {
                    if name.is_empty()
                        || name.contains('\0')
                        || name.starts_with('\u{1}')
                        || name == "constructor"
                    {
                        return Err("member-content");
                    }
                    keys.extend_from_slice(name.as_bytes());
                    keys.push(0);
                    attrs.push(*attr);
                    let slot = i + 1;
                    if let Some(m) = method {
                        // Slots 0..32 are lanes of the shape's `rep` word; a
                        // method past it has its ConstFn lane in the body list
                        // alone (the runtime derives the same lane from it).
                        if slot < 32 {
                            rep |= 3u64 << (2 * slot);
                        }
                        let body = format!(
                            "{}__eclo",
                            crate::symbol_names::method(&prefix, &c.name, &m.name)
                        );
                        constfn.push(ConstFnBirth {
                            slot: slot as u32,
                            symbol: crate::fn_info::info_symbol(&body),
                        });
                    }
                }
                for (i, (spelling, (_, member, attr))) in symbols.iter().enumerate() {
                    keys.extend_from_slice(spelling.as_bytes());
                    keys.push(0);
                    attrs.push(*attr);
                    let slot = string_count + i + 1;
                    if let Some(m) = member {
                        // As for string keys: a lane past word 0 is the body list alone.
                        if slot < 32 {
                            rep |= 3u64 << (2 * slot);
                        }
                        let body = format!(
                            "{}__eclo",
                            crate::symbol_names::method(&prefix, &c.name, &m.function.name)
                        );
                        constfn.push(ConstFnBirth {
                            slot: slot as u32,
                            symbol: crate::fn_info::info_symbol(&body),
                        });
                    }
                }
                let members_len = string_count + symbols.len();
                Ok(ModuleBirth {
                    keys_global: format!("perry_decl_proto_{prefix}__{cid}"),
                    class_id: 0,
                    defined: false,
                    shape: BirthShape {
                        keys,
                        key_count: members_len as u32 + 1,
                        live: members_len as u32 + 1,
                        proto: BirthProto::Prototype(cid, parent),
                        typed: None,
                        rep,
                        constfn,
                        private: Vec::new(),
                        brands: Vec::new(),
                        attrs,
                    },
                })
            })();
            match candidate {
                Ok(birth) => {
                    evaluated += usize::from(c.extends_expr.is_some());
                    Some(birth)
                }
                Err(reason) => {
                    *refused.entry(reason).or_default() += 1;
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    if std::env::var_os("PERRY_LAYOUT_REPORT").is_some() {
        eprintln!(
            "perry: static prototypes {prefix}: admitted {} ({} evaluated); refused {:?}",
            births.len(),
            evaluated,
            refused
        );
    }
    births
}

/// The birth-shape key spelling of known symbol `key`
/// (`ClassComputedMember::known_symbol`): `\u{1}@@wk:<number>` for a
/// well-known symbol, `\u{1}@@for:<registry key>` for a registered one (the
/// atom spellings of `layout_desc::symbol_atom`). No string member of an
/// admitted prototype starts with `\u{1}`, so a spelling names one key.
pub(crate) fn symbol_key_spelling(key: &str) -> Option<String> {
    if key.contains('\0') {
        return None;
    }
    match key.strip_prefix("@@__perry_wk_") {
        Some(name) => crate::runtime_abi::WELL_KNOWN_SYMBOLS
            .iter()
            .position(|wk| *wk == name)
            .map(|i| format!("\u{1}@@wk:{i}")),
        None => Some(format!("\u{1}@@for:{key}")),
    }
}

#[cfg(test)]
#[path = "static_prototype_tests.rs"]
mod tests;

/// The instance method of `c` keyed by the known symbol spelled `spelling`
/// ([`symbol_key_spelling`]): the body its static prototype's slot holds.
pub(crate) fn declared_symbol_method<'c>(
    c: &'c perry_hir::Class,
    spelling: &str,
) -> Option<&'c perry_hir::ClassComputedMember> {
    if !spelling.starts_with('\u{1}') {
        return None;
    }
    c.computed_members.iter().rev().find(|m| {
        !m.is_static
            && matches!(m.kind, perry_hir::ClassComputedMemberKind::Method)
            && m.known_symbol().and_then(symbol_key_spelling).as_deref() == Some(spelling)
    })
}

/// Every intermediate shape proves absence and the next prototype link.
/// A known-symbol key (`\u{1}` spelling) is proven like a string method: its
/// holder's ConstFn lane holds the declared member's body.
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
        let (id, shape) = super::static_shape_ids::static_prototype_layout(cid)?;
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
                && (c.methods
                    .iter()
                    .any(|m| m.name == method && !m.is_async && !m.is_generator)
                    || declared_symbol_method(c, method).is_some()))
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

/// `super.method` in an instance method of `class`: the chain from `class`'s
/// prototype (the methods' [[HomeObject]], whose static shape names its
/// parent's prototype as [[Prototype]]) through [`chain`] of the parent to
/// the ConstFn holder, with the holder's class name. `None` when any class on
/// it has evaluated heritage (its prototype may be born off the static shape)
/// or the compiler names no such chain.
pub(crate) fn super_chain(
    ctx: &crate::expr::FnCtx<'_>,
    class: &str,
    method: &str,
) -> Option<(Vec<(u32, u32)>, String)> {
    let c = ctx.classes.get(class)?;
    if c.specialized_from.is_some() || c.extends_expr.is_some() {
        return None;
    }
    let cid = *ctx.class_ids.get(class)?;
    let (id, shape) = super::static_shape_ids::static_prototype_layout(cid)?;
    let parent = c.extends_name.as_ref()?;
    if !matches!(shape.proto, BirthProto::Prototype(_, p) if ctx.class_ids.get(parent) == Some(&p))
    {
        return None;
    }
    let tail = chain(ctx, parent, method)?;
    let &(holder_cid, _) = tail.last()?;
    // Walk the same links to name the holder; refuse evaluated heritage.
    let mut name = parent.clone();
    for _ in 0..64 {
        let c = ctx.classes.get(&name)?;
        if c.extends_expr.is_some() {
            return None;
        }
        if let Some(origin) = &c.specialized_from {
            name = origin.clone();
            continue;
        }
        if ctx.class_ids.get(&name) == Some(&holder_cid) {
            let mut result = vec![(cid, id)];
            result.extend(tail);
            return Some((result, name));
        }
        name = c.extends_name.clone()?;
    }
    None
}

/// The static prototype chain of `class`'s instances to `%Object.prototype%`
/// (class id, static ShapeId of each prototype, the class's first), when
/// every prototype on it is a declared class with a complete static shape
/// whose next link it proves, and none holds an accessor, a non-writable or
/// a private property named like one of `keys`: an add of any of `keys` to
/// an instance is then never intercepted while each prototype is on its
/// static shape (and `%Object.prototype%`, whose one accessor is
/// `__proto__`, is unwritten).
pub(crate) fn add_chain(
    ctx: &crate::expr::FnCtx<'_>,
    class: &str,
    keys: &[&str],
) -> Option<Vec<(u32, u32)>> {
    // `key_attrs` entry bits that intercept an add: accessor, non-writable,
    // private.
    const INTERCEPTS: u8 = 0x08 | 0x01 | 0x40;
    if keys.contains(&"__proto__") {
        return None;
    }
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
        let (id, shape) = super::static_shape_ids::static_prototype_layout(cid)?;
        if shape.attrs.len() != shape.key_count as usize {
            return None;
        }
        let intercepted = shape
            .keys
            .split(|&b| b == 0)
            .take(shape.key_count as usize)
            .zip(&shape.attrs)
            .any(|(k, &attr)| attr & INTERCEPTS != 0 && keys.iter().any(|key| key.as_bytes() == k));
        if intercepted {
            return None;
        }
        result.push((cid, id));
        let BirthProto::Prototype(_, parent) = shape.proto else {
            return None;
        };
        match &c.extends_name {
            None if parent == 0 => return Some(result),
            Some(next) if ctx.class_ids.get(next) == Some(&parent) => name = next.clone(),
            _ => return None,
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
    let (_, shape) = super::static_shape_ids::static_prototype_layout(cid)?;
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

/// Complete accessor-holder shapes: the semantic generation changes whenever
/// an accessor half changes, even when its key and attributes stay the same.
pub(crate) fn accessor_chain(
    ctx: &crate::expr::FnCtx<'_>,
    class: &str,
    property: &str,
    setter: bool,
) -> Option<(String, Vec<(u32, u32)>)> {
    let (owner, chain) = accessor_owner(ctx.classes, ctx.class_ids, class, property, setter)?;
    let body = ctx
        .methods
        .get(&(
            owner,
            format!("__{}_{property}", if setter { "set" } else { "get" }),
        ))?
        .clone();
    Some((body, chain))
}

pub(crate) fn accessor_owner(
    classes: &HashMap<String, &perry_hir::Class>,
    ids: &HashMap<String, u32>,
    class: &str,
    property: &str,
    setter: bool,
) -> Option<(String, Vec<(u32, u32)>)> {
    let mut name = class.to_string();
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    for _ in 0..64 {
        if !seen.insert(name.clone()) {
            return None;
        }
        let c = classes.get(&name)?;
        if c.static_accessor_names.iter().any(|n| n == property) {
            return None;
        }
        let cid = *ids.get(&name)?;
        if cid == 0 || cid >= 0x7FFF_FF00 {
            return None;
        }
        let (shape_id, shape) = super::static_shape_ids::static_prototype_layout(cid)?;
        chain.push((cid, shape_id));
        if let Some(slot) = shape
            .keys
            .split(|&b| b == 0)
            .take(shape.key_count as usize)
            .position(|k| k == property.as_bytes())
        {
            let half = if setter { 32 } else { 16 };
            if shape.attrs.get(slot)? & half == 0 {
                return None;
            }
            let f = (if setter { &c.setters } else { &c.getters })
                .iter()
                .find(|(n, _)| n == property)?
                .1
                .clone();
            if !f.captures.is_empty()
                || f.is_async
                || f.is_generator
                || f.params
                    .iter()
                    .any(|p| p.is_rest || p.arguments_object.is_some())
            {
                return None;
            }
            return Some((name, chain));
        }
        name = c.extends_name.clone()?;
        if !matches!(shape.proto, BirthProto::Prototype(_, parent)
            if ids.get(&name) == Some(&parent))
        {
            return None;
        }
    }
    None
}
