//! Filesystem state registers its roots when that state is first created.
//! Reading a function export does not imply any filesystem state exists.
use super::*;

pub(crate) fn install_roots() {
    crate::fs_hook_slots::install();
    dir_glob_watch::install_tls_resolvers();
    crate::feature_hooks::FS_REGISTER_ROOTS.set(register_roots);
    register_roots();
}

fn register_roots() {
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::fs::scan_fs_handle_roots_mut",
        scan_fs_handle_roots_mut,
    );
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::fs::scan_fs_stream_roots_mut",
        scan_fs_stream_roots_mut,
    );
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::fs::scan_fs_watcher_roots_mut",
        scan_fs_watcher_roots_mut,
    );
}

pub(crate) fn install_handle_roots() {
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::fs::scan_fs_handle_roots_mut",
        scan_fs_handle_roots_mut,
    );
}

pub(crate) fn install_stream_roots() {
    crate::fs_hook_slots::install_streams();
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::fs::scan_fs_stream_roots_mut",
        scan_fs_stream_roots_mut,
    );
}

pub(crate) fn install_watcher_roots() {
    dir_glob_watch::install_tls_resolvers();
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::fs::scan_fs_watcher_roots_mut",
        scan_fs_watcher_roots_mut,
    );
}
