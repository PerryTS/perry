//! A private method or getter call selects the declaring brand's member, independently
//! of the receiver's public prototype chain or the property's storage spelling.
use crate::gen::Gen;
use crate::model::*;
use perry_hir::Expr;

impl Gen<'_> {
    pub(crate) fn private_method_callee(&mut self, mi: u32, guard: &Expr) -> Option<NodeId> {
        let Expr::PrivateGuard {
            class_id,
            field_name,
            kind: 1,
            op,
            ..
        } = guard
        else {
            return None;
        };
        let target = self.modules[mi as usize]
            .1
            .classes
            .iter()
            .find(|c| *class_id != 0 && c.id == *class_id)
            .and_then(|c| {
                let methods = if *op >= 2 {
                    &c.static_methods
                } else {
                    &c.methods
                };
                methods.iter().find(|m| m.name == *field_name)
            })
            .and_then(|m| self.mods[mi as usize].funcs.get(&m.id))
            .map(|&f| self.m.funcs[f as usize].token);
        let out = self.tmp("private-method-callee");
        match target {
            Some(t) => self.m.add_tok(out, t),
            None => self.m.add_top(out, |_| {
                "unregistered function id: private declaring member".into()
            }),
        }
        Some(out)
    }

    /// A private getter is selected by its lexical declaration even when a
    /// constructor return override installs its brand on a function or exotic.
    pub(crate) fn private_getter_callee(&mut self, mi: u32, guard: &Expr) -> Option<NodeId> {
        let Expr::PrivateGuard {
            class_id,
            field_name,
            kind: 2 | 4,
            ..
        } = guard
        else {
            return None;
        };
        let target = self.modules[mi as usize]
            .1
            .classes
            .iter()
            .find(|c| *class_id != 0 && c.id == *class_id)
            .and_then(|c| c.getters.iter().find(|(name, _)| name == field_name))
            .and_then(|(_, getter)| self.mods[mi as usize].funcs.get(&getter.id))
            .map(|&f| self.m.funcs[f as usize].token);
        let out = self.tmp("private-getter-callee");
        match target {
            Some(t) => self.m.add_tok(out, t),
            None => self
                .m
                .add_top(out, |_| "unregistered function id: private getter".into()),
        }
        Some(out)
    }

    pub(crate) fn private_getter_or_load(
        &mut self,
        mi: u32,
        guard: &Expr,
        receiver: NodeId,
        key: Atom,
    ) -> NodeId {
        if let Some(callee) = self.private_getter_callee(mi, guard) {
            self.call_value(callee, ThisArg::Node(receiver), vec![], vec![])
        } else {
            self.load(receiver, Key::Named(key))
        }
    }
}
