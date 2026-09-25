//! Shared zero-argument SplitView constructor and two-handle child ABI.
use crate::{catch_panic, catch_panic_void, widgets};

#[no_mangle]
pub extern "C" fn perry_ui_splitview_create() -> i64 {
    catch_panic("perry_ui_splitview_create", widgets::splitview::create)
}

#[no_mangle]
pub extern "C" fn perry_ui_splitview_add_child(parent: i64, child: i64) {
    catch_panic_void("perry_ui_splitview_add_child", || {
        widgets::splitview::add_child(parent, child)
    });
}
