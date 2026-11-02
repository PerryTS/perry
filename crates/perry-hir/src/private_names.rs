//! Storage keys of class private names (`#x`), and where construction
//! claims a class's private fields.
//!
//! A private field's value lives in an own entry of its holder flagged
//! private (the runtime's `ENTRY_PRIVATE`, #11791), named by the declaring
//! class and the source name. Only code inside that class body names it,
//! always through `Expr::PrivateGuard`; no public key aliases it (a public
//! property of the same spelling is a different entry) and reflection never
//! lists it.

use crate::Class;

/// The storage key of private field `name` (`#x`, with its `#`) of the
/// class whose private elements carry `class_id`: the key lowering gives a
/// guarded access (`lower::expr_member::private_storage_property`) and the
/// runtime claims for the class template's evaluation.
pub fn private_value_key(class_id: u32, name: &str) -> String {
    format!("#<perry:private-value:{class_id}:{name}>")
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

/// The chain of `class`, root first, when construction claims every private
/// field of it at a known inline slot (#11791): each field initializer
/// appends its entry after the birth keys, root to leaf in declaration
/// order, into a slot the layout reserves for it, so the `i`th private field
/// of the chain is at slot `birth keys + i`. `None` when that order is not
/// decidable here: a chain that is not wholly local and ordinary, no private
/// instance element on it, a computed-key field (claimed in source order),
/// a nested or widened class (allocated outside the birth layout), or a
/// constructor that adds keys of its own (a parent's run before a child's
/// claims). Codegen's completed private shape
/// (`codegen::static_private_class::private_final`) and the program facts'
/// private field slots both follow it.
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
