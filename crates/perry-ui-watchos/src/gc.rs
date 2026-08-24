use std::sync::Once;

use perry_ffi::{gc_register_mutable_root_scanner_named, GcRootVisitor};

static GC_REGISTERED: Once = Once::new();

pub(crate) fn ensure_registered() {
    perry_ui::ensure_gc_scanner_registered();
    GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ui-watchos", scan_roots);
    });
}

fn scan_roots(visitor: &mut GcRootVisitor<'_>) {
    crate::audio_playback::scan_watchos_audio_playback_gc_roots(visitor);
    crate::background::scan_watchos_background_gc_roots(visitor);
    crate::media_playback::scan_watchos_media_playback_gc_roots(visitor);
    crate::notifications::scan_watchos_notifications_gc_roots(visitor);
    crate::state::scan_watchos_state_gc_roots(visitor);
    crate::tree::scan_watchos_tree_gc_roots(visitor);
}
