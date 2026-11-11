//! Read access to the runtime's realm table and the effect declarations it
//! carries (BFX1 S1 carrier). The table is generated once, by the build
//! script of the leaf crate perry-realm-table, from the runtime's own `#[realm(..)]` declarations: the
//! runtime's declaration is the single authority, the solver keeps no names.
//! The solver interprets them in `builtins/effects.rs` (`run_native`).
use perry_hir::realm_manifest::RealmEntry;

/// Every realm declaration, in manifest order.
pub fn realm() -> &'static [RealmEntry] {
    perry_realm_table::REALM
}

#[cfg(test)]
mod tests {
    use perry_hir::realm_manifest::{RealmEffect, RealmOperand, RealmSlotRoot};

    #[test]
    fn a_declared_native_reaches_the_solver_with_its_effects() {
        let entry = super::realm()
            .iter()
            .find(|e| e.name == "Array.prototype.filter")
            .expect("Array.prototype.filter is a realm native");
        let calls = entry
            .effects
            .iter()
            .find_map(|e| match e {
                RealmEffect::Calls { callee, this, args } => Some((callee, this, args)),
                _ => None,
            })
            .expect("filter declares the callback call");
        assert_eq!(calls.0.root, RealmSlotRoot::Arg(0));
        assert!(matches!(calls.1, RealmOperand::Slot(s) if s.root == RealmSlotRoot::Arg(1)));
        assert_eq!(calls.2.len(), 3);
        // An undeclared native carries nothing: unknown code.
        assert!(super::realm()
            .iter()
            .any(|e| e.facts.contains("\"body\"") && e.effects.is_empty()));
    }
}
