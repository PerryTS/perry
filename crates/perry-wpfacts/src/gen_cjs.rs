//! Path identities for the generated CommonJS registry operations.

use crate::{gen::Gen, model::NodeId};

impl Gen<'_> {
    /// Runtime bootstrap allocates one empty main record before CJS entry
    /// initialization. ESM entries have no record, hence the undefined arm.
    pub(crate) fn cjs_main_value(&mut self, mi: u32) -> NodeId {
        if let Some(node) = self.cjs_main {
            return node;
        }
        let (_, node) = self.bobj(mi, "CjsMain");
        self.m.add_prim(node, crate::model::prim::UNDEF);
        self.cjs_main = Some(node);
        node
    }

    pub(crate) fn cjs_path_node(&mut self, path: &str) -> NodeId {
        // Generated wrappers pass canonical absolute filenames. Canonicalize
        // existing aliases exactly as the runtime registry does; absent paths
        // keep their spelling and are conservatively nullable on lookup.
        let key = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());
        if let Some(&node) = self.cjs_paths.get(&key) {
            return node;
        }
        let node = self.m.node_named("cjs-path", || format!("cjs-path({key})"));
        self.cjs_paths.insert(key, node);
        node
    }
}
