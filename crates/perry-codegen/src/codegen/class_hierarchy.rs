//! The module's class table read in the other direction: from a class to the
//! classes that extend it, and from a method name to the classes that declare
//! it.
//!
//! Every `extends` edge lives on the child (`Class::extends_name`), so the
//! questions codegen asks per site — "which classes are subclasses of `C`",
//! "which classes resolve `m` through their chain" — were answered by walking
//! the parent chain of EVERY class in the module, at every site. On a bundle
//! with tens of thousands of classes and object-literal shapes that made the
//! method-call and class-field lowerings quadratic and the largest part of the
//! compile's serial time. This is the same edge set, inverted once when the
//! tables are final; each answer is the set the per-class walks computed, and
//! callers keep their own deterministic orderings over it.

use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Default)]
pub(crate) struct ClassHierarchy {
    /// Parent class name → every class-table name whose class extends it.
    children: HashMap<String, Vec<String>>,
    /// Method name → every class name the method table registers it under.
    definers: HashMap<String, Vec<String>>,
}

impl ClassHierarchy {
    pub(crate) fn new(
        classes: &HashMap<String, &perry_hir::Class>,
        methods: &HashMap<(String, String), String>,
    ) -> Self {
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        for (name, class) in classes {
            if let Some(parent) = &class.extends_name {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(name.clone());
            }
        }
        let mut definers: HashMap<String, Vec<String>> = HashMap::new();
        for (class, method) in methods.keys() {
            definers
                .entry(method.clone())
                .or_default()
                .push(class.clone());
        }
        Self { children, definers }
    }

    /// Every class name whose `extends` chain reaches `ancestor` within
    /// `max_depth` parent steps (unbounded when `None`), `ancestor` itself
    /// excluded. Unordered.
    ///
    /// A chain is a sequence of distinct names up to the first repeat, so a
    /// name reaches `ancestor` exactly when the reverse search from `ancestor`
    /// finds it, at the same depth.
    pub(crate) fn descendants(&self, ancestor: &str, max_depth: Option<usize>) -> Vec<&str> {
        let mut found: Vec<&str> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        seen.insert(ancestor);
        let mut queue: VecDeque<(&str, usize)> = VecDeque::new();
        queue.push_back((ancestor, 0));
        while let Some((name, depth)) = queue.pop_front() {
            if max_depth.is_some_and(|max| depth >= max) {
                continue;
            }
            for child in self.children.get(name).into_iter().flatten() {
                if seen.insert(child.as_str()) {
                    found.push(child.as_str());
                    queue.push_back((child.as_str(), depth + 1));
                }
            }
        }
        found
    }

    /// Every class the method table registers `method` under. Unordered.
    pub(crate) fn method_definers(&self, method: &str) -> &[String] {
        self.definers.get(method).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(name: &str, parent: Option<&str>) -> perry_hir::Class {
        perry_hir::Class {
            id: 0,
            name: name.to_string(),
            type_params: Vec::new(),
            extends: None,
            extends_name: parent.map(str::to_string),
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: Vec::new(),
            constructor: None,
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            computed_members: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            is_nested: false,
            alloc_width_hint: 0,
            specialized_from: None,
            aliases: Vec::new(),
        }
    }

    /// The per-class parent walk `ClassHierarchy::descendants` replaces,
    /// including its depth and repeat guards.
    fn walk_reaches(
        classes: &HashMap<String, &perry_hir::Class>,
        name: &str,
        ancestor: &str,
        max_depth: Option<usize>,
    ) -> bool {
        let mut seen: HashSet<String> = HashSet::new();
        let mut parent = classes.get(name).and_then(|c| c.extends_name.clone());
        let mut depth = 0usize;
        while let Some(p) = parent {
            depth += 1;
            if max_depth.is_some_and(|max| depth > max) || !seen.insert(p.clone()) {
                return false;
            }
            if p == ancestor {
                return true;
            }
            parent = classes.get(&p).and_then(|c| c.extends_name.clone());
        }
        false
    }

    #[test]
    fn descendants_match_the_parent_walk_on_chains_cycles_and_depth_caps() {
        let mut owned = vec![
            class("A", None),
            class("B", Some("A")),
            class("C", Some("B")),
            class("D", Some("A")),
            class("E", Some("Missing")),
            // A two-class cycle with a tail hanging off it.
            class("X", Some("Y")),
            class("Y", Some("X")),
            class("Z", Some("Y")),
        ];
        // A 70-deep chain: the depth cap must cut it exactly where the walk does.
        for i in 0..70 {
            let parent = if i == 0 {
                "A".to_string()
            } else {
                format!("L{}", i - 1)
            };
            owned.push(class(&format!("L{i}"), Some(&parent)));
        }
        let classes: HashMap<String, &perry_hir::Class> =
            owned.iter().map(|c| (c.name.clone(), c)).collect();
        let hierarchy = ClassHierarchy::new(&classes, &HashMap::new());
        let mut ancestors: Vec<&str> = classes.keys().map(String::as_str).collect();
        ancestors.push("Missing");
        for max_depth in [None, Some(64), Some(1)] {
            for ancestor in &ancestors {
                let mut got: Vec<&str> = hierarchy.descendants(ancestor, max_depth);
                got.sort_unstable();
                let mut want: Vec<&str> = classes
                    .keys()
                    .map(String::as_str)
                    .filter(|name| name != ancestor)
                    .filter(|name| walk_reaches(&classes, name, ancestor, max_depth))
                    .collect();
                want.sort_unstable();
                assert_eq!(got, want, "ancestor {ancestor}, max_depth {max_depth:?}");
            }
        }
    }

    #[test]
    fn method_definers_list_every_registered_class() {
        let mut methods = HashMap::new();
        methods.insert(("A".to_string(), "m".to_string()), "fa".to_string());
        methods.insert(("B".to_string(), "m".to_string()), "fb".to_string());
        methods.insert(("B".to_string(), "n".to_string()), "fbn".to_string());
        let hierarchy = ClassHierarchy::new(&HashMap::new(), &methods);
        let mut m: Vec<&str> = hierarchy
            .method_definers("m")
            .iter()
            .map(String::as_str)
            .collect();
        m.sort_unstable();
        assert_eq!(m, ["A", "B"]);
        assert!(hierarchy.method_definers("absent").is_empty());
    }
}
