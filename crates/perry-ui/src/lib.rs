pub mod key_dispatch;
pub mod keys;
pub mod state;
pub mod styling_matrix;
pub mod widget;

use std::sync::Once;

use perry_ffi::{gc_register_mutable_root_scanner_named, GcRootVisitor};

pub use keys::{KeyCode, KEY_TABLE};
pub use state::StateId;
pub use widget::{WidgetHandle, WidgetKind};

static GC_REGISTERED: Once = Once::new();

/// Register roots owned by the platform-independent UI dispatcher.
///
/// Platform backends call this from their own GC registration path before
/// they retain any user callback.
pub fn ensure_gc_scanner_registered() {
    GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ui", scan_gc_roots);
    });
}

fn scan_gc_roots(visitor: &mut GcRootVisitor<'_>) {
    key_dispatch::scan_key_dispatch_gc_roots(visitor);
}
