//! Region size in node units, including code hidden in a namespace value.
use super::facts::{for_each_child_list, for_each_own_expr};
use perry_hir::{Expr, Import, ImportSpecifier, Stmt};

pub(super) struct Cost<'a> {
    pub imports: &'a [Import],
    /// Borrow the already collected HIR graph; never retain a size table.
    pub namespace_members: &'a dyn Fn(&str) -> usize,
}

impl Cost<'_> {
    fn namespace_source(&self, name: &str) -> Option<&str> {
        self.imports.iter().find_map(|i| {
            (!i.type_only
                && !i.runtime_erased
                && !i.is_native
                && i.specifiers.iter().any(|s| {
                    matches!(s,
                    ImportSpecifier::Namespace { local } if local == name)
                }))
            .then_some(i.resolved_path.as_deref().unwrap_or(i.source.as_str()))
        })
    }

    fn namespace_ref(&self, e: &Expr) -> bool {
        matches!(e, Expr::ExternFuncRef { name, .. }
            if self.namespace_source(name).is_some())
    }

    fn extra_expr(&self, e: &Expr) -> usize {
        let mut n = match e {
            Expr::ExternFuncRef { name, .. } => self
                .namespace_source(name)
                .map(|source| (self.namespace_members)(source) * 3)
                .unwrap_or(0),
            _ => 0,
        };
        // ns.member resolves one binding directly. Only a whole-value read
        // builds the object, allocating/wrapping and storing every member,
        // with safepoint plumbing. Charge those three operations per member.
        let qualifier = match e {
            Expr::PropertyGet { object, .. } if self.namespace_ref(object) => Some(object.as_ref()),
            Expr::NativeMethodCall {
                object: Some(object),
                ..
            } if self.namespace_ref(object) => Some(object.as_ref()),
            _ => None,
        };
        perry_hir::walker::walk_expr_children(e, &mut |child| {
            if !qualifier.is_some_and(|q| std::ptr::eq(q, child)) {
                n += self.extra_expr(child);
            }
        });
        n
    }

    fn extra_body(&self, body: &[Stmt]) -> usize {
        let mut n = 0;
        for s in body {
            for_each_own_expr(s, &mut |e| n += self.extra_expr(e));
            for_each_child_list(s, &mut |b| n += self.extra_body(b));
        }
        n
    }

    pub fn body(&self, body: &[Stmt]) -> usize {
        let nodes = crate::collectors::count_body_nodes(body);
        if !self.imports.iter().any(|i| {
            i.specifiers
                .iter()
                .any(|s| matches!(s, ImportSpecifier::Namespace { .. }))
        }) {
            return nodes;
        }
        nodes + self.extra_body(body)
    }
}
