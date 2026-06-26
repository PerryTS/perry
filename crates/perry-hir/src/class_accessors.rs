#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassAccessorNames {
    pub getter_names: Vec<String>,
    pub setter_names: Vec<String>,
    /// #5592: the class (or an ancestor) declares at least one accessor with a
    /// COMPUTED key (`get [expr]()` / `set [expr](v)`). Its property name can't
    /// be known statically, so callers that decide whether
    /// `obj.prototype.<x> = v` should invoke a setter must treat this class as
    /// "possibly has an accessor named <x>" for any `<x>`.
    pub has_computed: bool,
}

impl ClassAccessorNames {
    pub fn is_empty(&self) -> bool {
        self.getter_names.is_empty() && self.setter_names.is_empty() && !self.has_computed
    }

    pub fn contains_any(&self, name: &str) -> bool {
        self.getter_names.iter().any(|n| n == name) || self.setter_names.iter().any(|n| n == name)
    }

    pub fn insert_getter(&mut self, name: String) -> bool {
        if self.getter_names.iter().any(|n| n == &name) {
            false
        } else {
            self.getter_names.push(name);
            true
        }
    }

    pub fn insert_setter(&mut self, name: String) -> bool {
        if self.setter_names.iter().any(|n| n == &name) {
            false
        } else {
            self.setter_names.push(name);
            true
        }
    }

    pub fn extend_from(&mut self, other: &Self) -> bool {
        let mut changed = false;
        for name in &other.getter_names {
            changed |= self.insert_getter(name.clone());
        }
        for name in &other.setter_names {
            changed |= self.insert_setter(name.clone());
        }
        if other.has_computed && !self.has_computed {
            self.has_computed = true;
            changed = true;
        }
        changed
    }
}
