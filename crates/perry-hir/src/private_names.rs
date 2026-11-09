//! Storage keys of class private names (`#x`), and where construction
//! claims a class's private fields.
//!
//! A private field's value lives in an own entry of its holder flagged
//! private (the runtime's `ENTRY_PRIVATE`, #11791), named by the declaring
//! class and the source name. Only code inside that class body names it,
//! always through `Expr::PrivateGuard`; no public key aliases it (a public
//! property of the same spelling is a different entry) and reflection never
//! lists it.

use crate::{Class, ClassField};

/// The storage key of private field `name` (`#x`, with its `#`) of the
/// class whose private elements carry `class_id`: the key lowering gives a
/// guarded access (`lower::expr_member::private_storage_property`) and the
/// runtime claims for the class template's evaluation.
pub fn private_value_key(class_id: u32, name: &str) -> String {
    format!("#<perry:private-value:{class_id}:{name}>")
}

/// The dispatch key of private method or accessor `name` (`#x`) of the class
/// whose private elements carry `class_id`: the key lowering gives a guarded
/// access to it (`lower::expr_member::private_storage_property`).
pub fn private_member_key(class_id: u32, name: &str) -> String {
    format!("#<perry:private-member:{class_id}:{name}>")
}

/// The chain of `class`, root first, when every link is a local ordinary
/// class.
fn local_chain<'c>(
    class: &'c Class,
    lookup: &dyn Fn(&str) -> Option<&'c Class>,
) -> Option<Vec<&'c Class>> {
    let mut chain = vec![class];
    let mut current = class;
    while let Some(parent) = current.extends_name.as_deref() {
        let parent = lookup(parent)?;
        if chain.len() > 32 || chain.iter().any(|c| std::ptr::eq(*c, parent)) {
            return None;
        }
        chain.push(parent);
        current = parent;
    }
    if chain.iter().any(|c| {
        c.is_imported_stub()
            || c.native_extends.is_some()
            || c.extends_expr.is_some()
            || c.heritage_lexically_shadowed
    }) {
        return None;
    }
    chain.reverse();
    Some(chain)
}

/// The entries construction defines on an instance of a chain, in the order
/// it appends them (#8969): root first, each class's fields with a static key
/// in declaration order. A private field is one of them: its claim
/// (`js_private_field_add`, or a family layout's inline element) takes the next
/// slot at its declaration, exactly like a public field's definition, so the
/// runtime's key order is `[parent, #ps, child]` for
/// `class P { parent; #ps } class C extends P { child }`. The one rule codegen
/// (`type_analysis::class_field_global_index`, the completed private shape)
/// and the program facts' birth keys (perry-wpfacts `compute_birth_keys`)
/// take field slots from. A computed-key field is not among them (its key is
/// not known here).
pub fn construction_entries<'f>(
    chain_fields: impl IntoIterator<Item = &'f [ClassField]>,
) -> impl Iterator<Item = &'f ClassField> {
    chain_fields
        .into_iter()
        .flatten()
        .filter(|f| f.key_expr.is_none())
}

/// Does a private field's entry come before `target`'s in the construction
/// order of `chain_fields` (`construction_entries`)? Then `target`'s slot is
/// past a claimed private entry: a layout that lists only the chain's public
/// keys (codegen's packed class keys, its birth rep and birth ShapeId) has no
/// slot for it that an instance is on.
pub fn private_entry_precedes<'f>(
    chain_fields: impl IntoIterator<Item = &'f [ClassField]>,
    target: &ClassField,
) -> bool {
    for f in construction_entries(chain_fields) {
        if std::ptr::eq(f, target) {
            return false;
        }
        if f.is_private {
            return true;
        }
    }
    false
}

/// The chain of `class`, root first, when construction claims every private
/// field of it at a known inline slot (#11791): each field initializer
/// appends its entry, root to leaf in declaration order, into a slot the
/// layout reserves for it, and every private entry comes after every public
/// one in the construction order (`construction_entries`), so the `i`th
/// private field of the chain is at slot `birth keys + i`. `None` when that
/// order is not decidable here or is another one: a chain that is not wholly
/// local and ordinary, no private instance element on it, a computed-key
/// field (claimed in source order), a nested or widened class (allocated
/// outside the birth layout), a constructor that adds keys of its own (a
/// parent's run before a child's claims), or a public field defined after a
/// private one (#8969: its entry follows the claim). Codegen's completed
/// private shape (`codegen::static_private_class::private_final`) follows it.
pub fn claimed_private_chain<'c>(
    class: &'c Class,
    lookup: &dyn Fn(&str) -> Option<&'c Class>,
) -> Option<Vec<&'c Class>> {
    let chain = local_chain(class, lookup)?;
    if !chain.iter().any(|c| c.has_private_instance_elements()) {
        return None;
    }
    if chain.iter().any(|c| {
        c.fields.iter().any(|f| f.key_expr.is_some()) || c.is_nested || c.alloc_width_hint != 0
    }) {
        return None;
    }
    if constructor_added_key_count_in(class, lookup) > 0 {
        return None;
    }
    let mut private_seen = false;
    for f in construction_entries(chain.iter().map(|c| c.fields.as_slice())) {
        if f.is_private {
            private_seen = true;
        } else if private_seen {
            return None;
        }
    }
    Some(chain)
}

/// The number of distinct static keys the constructor chain of `class`
/// stores into `this` that no class of the chain declares as a field. Walks
/// each constructor body's statements (branches, loops and `try` included;
/// nested functions excluded, since their `this` is not the instance).
/// Capped, so a generated constructor cannot make an instance arbitrarily
/// wide. Codegen's birth slack (`constructor_added_key_count`) and the
/// claimed private layout (`claimed_private_chain`) both derive from it.
pub fn constructor_added_key_count_in<'c>(
    class: &'c crate::Class,
    lookup: &dyn Fn(&str) -> Option<&'c crate::Class>,
) -> u32 {
    const SLACK_CAP: usize = 64;
    let mut chain: Vec<&crate::Class> = vec![class];
    let mut parent = class.extends_name.as_deref();
    while let Some(name) = parent {
        match lookup(name) {
            Some(p) if chain.len() < 32 => {
                chain.push(p);
                parent = p.extends_name.as_deref();
            }
            _ => break,
        }
    }
    let declared: std::collections::HashSet<&str> = chain
        .iter()
        .flat_map(|c| c.fields.iter().map(|f| f.name.as_str()))
        .collect();
    let mut added: std::collections::HashSet<String> = std::collections::HashSet::new();
    fn visit_expr(
        e: &crate::Expr,
        declared: &std::collections::HashSet<&str>,
        added: &mut std::collections::HashSet<String>,
    ) {
        let key = match e {
            crate::Expr::PropertySet {
                object, property, ..
            } if matches!(object.as_ref(), crate::Expr::This) => Some(property.as_str()),
            crate::Expr::PutValueSet { target, key, .. }
                if matches!(target.as_ref(), crate::Expr::This) =>
            {
                match key.as_ref() {
                    crate::Expr::String(k) => Some(k.as_str()),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(k) = key {
            let first = k.as_bytes().first().copied().unwrap_or(b'0');
            if !declared.contains(k) && first != b'#' && !first.is_ascii_digit() {
                added.insert(k.to_string());
            }
        }
        if matches!(e, crate::Expr::Closure { .. }) {
            return;
        }
        crate::walker::walk_expr_children(e, &mut |c| visit_expr(c, declared, added));
    }
    fn visit_stmts(
        stmts: &[crate::Stmt],
        declared: &std::collections::HashSet<&str>,
        added: &mut std::collections::HashSet<String>,
    ) {
        use crate::Stmt;
        for s in stmts {
            match s {
                Stmt::Expr(e) => visit_expr(e, declared, added),
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    visit_stmts(then_branch, declared, added);
                    if let Some(eb) = else_branch {
                        visit_stmts(eb, declared, added);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                    visit_stmts(body, declared, added)
                }
                Stmt::For { body, .. } => visit_stmts(body, declared, added),
                Stmt::Labeled { body, .. } => {
                    visit_stmts(std::slice::from_ref(body.as_ref()), declared, added)
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    visit_stmts(body, declared, added);
                    if let Some(c) = catch {
                        visit_stmts(&c.body, declared, added);
                    }
                    if let Some(f) = finally {
                        visit_stmts(f, declared, added);
                    }
                }
                _ => {}
            }
        }
    }
    for c in &chain {
        if let Some(ctor) = &c.constructor {
            visit_stmts(&ctor.body, &declared, &mut added);
        }
        if added.len() >= SLACK_CAP {
            break;
        }
    }
    added.len().min(SLACK_CAP) as u32
}

#[cfg(test)]
#[path = "private_names_tests.rs"]
mod tests;
