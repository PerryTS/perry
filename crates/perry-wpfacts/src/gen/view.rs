use super::*;
impl<'a> Gen<'a> {
    /// The unit view of module `mi`, when this is a per-unit solve (one
    /// module, the view's indices into it). A view borrows the module: the
    /// items below are read through these, never copied per unit.
    pub(crate) fn unit_view(&self, mi: usize) -> Option<&'a crate::decl_units::UnitView<'a>> {
        self.view.filter(|_| self.modules.len() == 1 && mi == 0)
    }

    pub(crate) fn functions_of(&self, mi: usize) -> Vec<&'a Function> {
        let m = self.modules[mi].1;
        match self.unit_view(mi) {
            Some(v) => v.funcs.iter().map(|&k| &m.functions[k]).collect(),
            None => m.functions.iter().collect(),
        }
    }

    pub(crate) fn classes_of(&self, mi: usize) -> Vec<&'a Class> {
        let m = self.modules[mi].1;
        match self.unit_view(mi) {
            Some(v) => v.classes.iter().map(|&k| &m.classes[k]).collect(),
            None => m.classes.iter().collect(),
        }
    }

    pub(crate) fn globals_of(&self, mi: usize) -> Vec<&'a Global> {
        let m = self.modules[mi].1;
        match self.unit_view(mi) {
            Some(v) => v.globals.iter().map(|&k| &m.globals[k]).collect(),
            None => m.globals.iter().collect(),
        }
    }

    pub(crate) fn init_of(&self, mi: usize) -> Vec<&'a Stmt> {
        let m = self.modules[mi].1;
        match self.unit_view(mi) {
            Some(v) => v.init.iter().map(|&k| &m.init[k]).collect(),
            None => m.init.iter().collect(),
        }
    }

    /// The module's imports, then the unit's synthetic ones.
    pub(crate) fn imports_of(&self, mi: usize) -> Vec<&'a Import> {
        let m = self.modules[mi].1;
        match self.unit_view(mi) {
            Some(v) => m.imports.iter().chain(v.imports.iter()).collect(),
            None => m.imports.iter().collect(),
        }
    }

    pub(crate) fn exports_of(&self, mi: usize) -> &'a [Export] {
        match self.unit_view(mi) {
            Some(v) => &v.exports,
            None => &self.modules[mi].1.exports,
        }
    }

    pub(crate) fn exported_functions_of(&self, mi: usize) -> &'a [(String, u32)] {
        match self.unit_view(mi) {
            Some(v) => &v.exported_functions,
            None => &self.modules[mi].1.exported_functions,
        }
    }

    pub(crate) fn exported_objects_of(&self, mi: usize) -> &'a [String] {
        match self.unit_view(mi) {
            Some(v) => &v.exported_objects,
            None => &self.modules[mi].1.exported_objects,
        }
    }
}
