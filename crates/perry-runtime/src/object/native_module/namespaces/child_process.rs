//! Item-level native namespace declarations for `child_process` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_CHILD_PROCESS_DEFAULT = "child_process.default" {
        cell = "node:child_process.default";
        members = {
            ChildProcess = (ref("node:child_process.ChildProcess"));
            _forkChild = (ref("node:child_process._forkChild"));
            exec = (ref("node:child_process.exec"));
            execFile = (ref("node:child_process.execFile"));
            execFileSync = (ref("node:child_process.execFileSync"));
            execSync = (ref("node:child_process.execSync"));
            fork = (ref("node:child_process.fork"));
            spawn = (ref("node:child_process.spawn"));
            spawnSync = (ref("node:child_process.spawnSync"));
        }
    }
}
