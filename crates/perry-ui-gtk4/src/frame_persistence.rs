use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow};
use perry_ui::frame::{FrameStore, WindowFrame, WindowState};

fn save(window: &ApplicationWindow, store: &FrameStore) {
    // GTK4 updates the default size when users resize, and preserves the
    // normal size while maximized/fullscreen. Allocation includes overrides.
    let (width, height) = window.default_size();
    let state = if window.is_fullscreen() {
        WindowState::Fullscreen
    } else if window.is_maximized() {
        WindowState::Maximized
    } else {
        WindowState::Normal
    };
    let _ = store.save(WindowFrame {
        x: 0,
        y: 0,
        width,
        height,
        state,
    });
}

pub(crate) fn install(
    window: &ApplicationWindow,
    app: &Application,
    name: &str,
) -> Option<&'static str> {
    let store = FrameStore::new(name)?;
    let restored = store.load();
    if let Some(frame) = restored {
        window.set_default_size(frame.width, frame.height);
    }
    let close_store = store.clone();
    window.connect_close_request(move |window| {
        save(window, &close_store);
        gtk4::glib::Propagation::Proceed
    });
    // Application.quit() need not emit close-request. Use a weak reference
    // so this callback does not keep a closed window alive.
    let weak_window = window.downgrade();
    app.connect_shutdown(move |_| {
        if let Some(window) = weak_window.upgrade() {
            // A previously closed window was already saved by close-request.
            if window.is_visible() {
                save(&window, &store);
            }
        }
    });
    restored.map(|frame| match frame.state {
        WindowState::Normal => "normal",
        WindowState::Maximized => "maximized",
        WindowState::Fullscreen => "fullscreen",
    })
}
