use super::*;
use perry_hir::Expr;

#[derive(Default)]
pub(crate) struct SharedClosures<'a> {
    bodies: Vec<FxHashMap<u32, &'a Expr>>,
}

impl<'a> Gen<'a> {
    pub(crate) fn index_emitted_closures(&mut self) {
        self.shared_closures.bodies = self
            .modules
            .iter()
            .enumerate()
            .map(|(mi, (_, module))| match self.unit_view(mi) {
                Some(v) => v.closure_bodies.clone(),
                None => perry_hir::emitted_closures(module).into_iter().collect(),
            })
            .collect();
    }

    pub(crate) fn emitted_closure(&self, mi: u32, id: u32) -> Option<&'a Expr> {
        self.shared_closures
            .bodies
            .get(mi as usize)?
            .get(&id)
            .copied()
    }

    /// Closure copies create values for one emitted body. Capture indices
    /// relate the creation frame's bindings to that body's lexical bindings.
    pub(crate) fn feed_closure_captures(&mut self, mi: u32, creation: &Expr, canonical: &Expr) {
        let (Expr::Closure { captures: from, .. }, Expr::Closure { captures: into, .. }) =
            (creation, canonical)
        else {
            unreachable!()
        };
        assert_eq!(
            from.len(),
            into.len(),
            "shared emitted closure capture layout changed"
        );
        for (&src, &dst) in from.iter().zip(into) {
            let src = self.local(mi, src);
            let dst = self.local(mi, dst);
            self.m.edge(src, dst);
        }
    }
}
