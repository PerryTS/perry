//! Spacer widget — virtual entry (no HWND), just a layout weight placeholder.
//! Spacers expand to fill remaining space in VStack/HStack containers.

use super::{alloc_control_id, register_widget, WidgetKind};

/// Create a Spacer. Returns widget handle.
pub fn create() -> i64 {
    let control_id = alloc_control_id();

    // Spacers have no HWND — they are virtual entries in the widget registry
    // that the layout engine treats as flexible space.
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::HWND;
        register_widget(HWND(std::ptr::null_mut()), WidgetKind::Spacer, control_id)
    }

    #[cfg(not(target_os = "windows"))]
    {
        register_widget(0, WidgetKind::Spacer, control_id)
    }
}
