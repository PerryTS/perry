//! Path identities for the generated CommonJS registry operations.

use crate::gen::Gen;
use crate::model::*;

impl<'a> Gen<'a> {
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
        let spelled = self.m.atom(path);
        let canon = self.m.atom(&key);
        self.m.cjs_paths.insert(spelled, node);
        self.m.cjs_paths.insert(canon, node);
        self.cjs_paths.insert(key, node);
        node
    }
}
