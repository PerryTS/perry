//! The declaration surface needed to resolve namespace exports.

use crate::ir::{Export, Import, Module};

/// Export resolution needs graph edges and binding existence, not function
/// bodies. Drivers can normalize the edges in a borrowed view of a module.
pub trait ExportBindings {
    fn exports(&self) -> &[Export];
    fn imports(&self) -> &[Import];
    fn defines_local_binding(&self, name: &str) -> bool;
}

impl ExportBindings for Module {
    fn exports(&self) -> &[Export] {
        &self.exports
    }

    fn imports(&self) -> &[Import] {
        &self.imports
    }

    fn defines_local_binding(&self, name: &str) -> bool {
        super::binding_origin::defines_local_binding(self, name)
    }
}
