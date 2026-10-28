//! Accessor-literal descriptor admission for property modeling.
use super::*;

impl<'a> Gen<'a> {
    /// FVA 4.3a: `d` is an accessor descriptor literal: a closed literal
    /// layout whose keys are within get/set/enumerable/configurable and
    /// name a getter or a setter (no value, no writable).
    pub(super) fn accessor_descriptor(&self, mi: u32, d: &Expr) -> bool {
        let Expr::New { class_name, .. } = d else {
            return false;
        };
        let Some(&tm) = self.mods[mi as usize].classes.get(class_name) else {
            return false;
        };
        let tpl = &self.m.tmpls[tm as usize];
        if !matches!(
            tpl.kind,
            TmplKind::Class {
                literal_shape: true,
                ..
            }
        ) {
            return false;
        }
        let keys: Vec<&str> = tpl
            .birth_keys
            .iter()
            .map(|a| self.m.atoms[*a as usize].as_str())
            .collect();
        keys.iter()
            .all(|k| matches!(*k, "get" | "set" | "enumerable" | "configurable"))
            && keys.iter().any(|k| matches!(*k, "get" | "set"))
    }

}
