use std::sync::Once;

use perry_ffi::{gc_register_mutable_root_scanner_named, GcRootVisitor};

static GC_REGISTERED: Once = Once::new();

pub(crate) fn ensure_registered() {
    perry_ui::ensure_gc_scanner_registered();
    GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ui-android", scan_roots);
    });
}

fn scan_roots(visitor: &mut GcRootVisitor<'_>) {
    crate::app::scan_android_app_gc_roots(visitor);
    crate::callback::scan_android_callback_gc_roots(visitor);
    crate::media_playback::scan_android_media_playback_gc_roots(visitor);
    crate::state::scan_android_state_gc_roots(visitor);
    crate::widgets::lazyvstack::scan_android_lazyvstack_gc_roots(visitor);
    crate::widgets::picker::scan_android_picker_gc_roots(visitor);
    crate::widgets::webview::scan_android_webview_gc_roots(visitor);
    crate::ws::scan_android_ws_gc_roots(visitor);
}
