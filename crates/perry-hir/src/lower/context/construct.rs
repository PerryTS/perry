use super::*;

impl LoweringContext {
    pub(crate) fn fresh_tagged_template_site_id(&mut self) -> u64 {
        let local_id = self.next_tagged_template_site_id;
        self.next_tagged_template_site_id = self.next_tagged_template_site_id.wrapping_add(1);
        (self.tagged_template_site_salt << 32) | u64::from(local_id)
    }

    pub(crate) fn fresh_interface(&mut self) -> InterfaceId {
        let id = self.next_interface_id;
        self.next_interface_id += 1;
        id
    }

    pub(crate) fn fresh_type_alias(&mut self) -> TypeAliasId {
        let id = self.next_type_alias_id;
        self.next_type_alias_id += 1;
        id
    }

    /// Enter a new type parameter scope (for generic function/class)
    pub(crate) fn enter_type_param_scope(&mut self, type_params: &[TypeParam]) {
        let scope: HashSet<String> = type_params.iter().map(|p| p.name.clone()).collect();
        self.type_param_scopes.push(scope);
        // Constraint table mirrors the same scope so `is_type_param(name)`
        // and `resolve_type_param_constraint(name)` agree on visibility.
        // Only params with a declared upper bound contribute an entry.
        let constraints: HashMap<String, crate::types::Type> = type_params
            .iter()
            .filter_map(|p| {
                p.constraint
                    .as_ref()
                    .map(|c| (p.name.clone(), (**c).clone()))
            })
            .collect();
        self.type_param_constraints.push(constraints);
    }

    /// Exit the current type parameter scope
    pub(crate) fn exit_type_param_scope(&mut self) {
        self.type_param_scopes.pop();
        self.type_param_constraints.pop();
    }

    /// Check if a name is a type parameter in the current scope
    pub(crate) fn is_type_param(&self, name: &str) -> bool {
        self.type_param_scopes
            .iter()
            .any(|scope| scope.contains(name))
    }

    /// Resolve a type-parameter reference to its declared upper-bound
    /// constraint, when that constraint is a runtime-meaningful type
    /// (`String`, `Number`, `Boolean`, `BigInt`, `Array<T>`). Returns
    /// `None` for parameters with no constraint or with constraints that
    /// don't usefully narrow the runtime representation (`unknown`/`any`/
    /// `object`/named-class/union — those keep `TypeVar(T)` so
    /// monomorphization or downstream native-instance tagging still has
    /// the chance to substitute a concrete type).
    ///
    /// Innermost scope wins (shadowing).
    pub(crate) fn resolve_type_param_constraint(&self, name: &str) -> Option<crate::types::Type> {
        for scope in self.type_param_constraints.iter().rev() {
            if let Some(ty) = scope.get(name) {
                // Only return constraints whose runtime shape is
                // narrower than `Any`. `String`/`Number`/`Boolean`/
                // `BigInt` are primitives; `Array<elem>` lets
                // `<T extends string[]>` propagate to the array fast
                // path. Anything else (e.g., `T extends SomeClass`,
                // `T extends "literal"`, intersections) falls back to
                // the `TypeVar`/`Named` path so existing native-
                // instance tagging / class-id propagation still work.
                let useful = matches!(
                    ty,
                    crate::types::Type::String
                        | crate::types::Type::Number
                        | crate::types::Type::Boolean
                        | crate::types::Type::BigInt
                        | crate::types::Type::Array(_)
                );
                if useful {
                    return Some(ty.clone());
                } else {
                    return None;
                }
            }
        }
        None
    }

    /// Look up a type alias by name and return its resolved type (if found).
    /// This is used during type extraction to resolve type aliases like
    /// `type BlockTag = 'latest' | number | string` so the compiler sees
    /// the underlying Union type instead of Named("BlockTag").
    pub(crate) fn resolve_type_alias(&self, name: &str) -> Option<crate::types::Type> {
        self.type_aliases
            .iter()
            .find(|(alias_name, _, type_params, _)| alias_name == name && type_params.is_empty())
            .map(|(_, _, _, ty)| ty.clone())
    }
}
