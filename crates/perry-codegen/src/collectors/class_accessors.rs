//! Getter / setter dispatch detection used by escape analysis.
//!
//! Split out of `escape_news.rs` in v0.5.1021 to satisfy the file-size CI
//! gate. No behavior change — these functions remain `pub` and are re-
//! exported from `collectors/mod.rs`.

/// Is `property` a getter on `class_name` (walking its inheritance chain)?
/// Used by escape analysis: a `LocalGet(candidate).gettableProp` access is
/// a real getter dispatch that needs `this` as a heap pointer, so the
/// candidate must escape.
pub fn is_class_getter(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    class_name: &str,
    property: &str,
) -> bool {
    let mut cur = Some(class_name.to_string());
    let mut seen = std::collections::HashSet::new();
    let mut depth = 0usize;
    while let Some(name) = cur {
        if !seen.insert(name.clone()) || depth > 64 {
            break;
        }
        depth += 1;
        if let Some(class) = classes.get(&name) {
            if class.getters.iter().any(|(n, _)| n == property) {
                return true;
            }
            cur = class.extends_name.clone();
        } else {
            return false;
        }
    }
    false
}

/// Mirror of `is_class_getter` for setters — used on the PropertySet/
/// PropertyUpdate paths where a setter dispatch (vs. a plain field write)
/// likewise needs a real `this` pointer.
pub fn is_class_setter(
    classes: &std::collections::HashMap<String, &perry_hir::Class>,
    class_name: &str,
    property: &str,
) -> bool {
    let mut cur = Some(class_name.to_string());
    let mut seen = std::collections::HashSet::new();
    let mut depth = 0usize;
    while let Some(name) = cur {
        if !seen.insert(name.clone()) || depth > 64 {
            break;
        }
        depth += 1;
        if let Some(class) = classes.get(&name) {
            if class.setters.iter().any(|(n, _)| n == property) {
                return true;
            }
            cur = class.extends_name.clone();
        } else {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::{Class, Function};
    use std::collections::HashMap;

    fn function(name: &str) -> Function {
        Function {
            id: 0,
            name: name.to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: Vec::new(),
            is_async: false,
            is_generator: false,
            is_strict: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        }
    }

    fn class(name: &str, extends_name: Option<&str>) -> Class {
        Class {
            id: 0,
            name: name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: extends_name.map(str::to_string),
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: Vec::new(),
            constructor: None,
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            computed_members: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
        }
    }

    #[test]
    fn accessor_lookup_stops_on_cyclic_parent_chain() {
        let mut child = class("A", Some("B"));
        let mut parent = class("B", Some("A"));
        parent
            .getters
            .push(("value".to_string(), function("__get_value")));
        child
            .setters
            .push(("own".to_string(), function("__set_own")));

        let mut classes = HashMap::new();
        classes.insert(child.name.clone(), &child);
        classes.insert(parent.name.clone(), &parent);

        assert!(is_class_getter(&classes, "A", "value"));
        assert!(is_class_setter(&classes, "A", "own"));
        assert!(!is_class_getter(&classes, "A", "missing"));
        assert!(!is_class_setter(&classes, "A", "missing"));
    }
}
