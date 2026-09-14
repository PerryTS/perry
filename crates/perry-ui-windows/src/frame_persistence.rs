//! Native placement persistence shared by Win32 and WinUI windows.

use std::cell::RefCell;
use std::collections::HashMap;

use perry_ui::frame::WindowFrame;
pub use perry_ui::frame::{FrameStore, WindowState};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

const SUBCLASS_ID: usize = 10170;

struct SavedWindow {
    store: FrameStore,
    frame: WindowFrame,
    fullscreen: bool,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<isize, SavedWindow>> = RefCell::new(HashMap::new());
}

fn placement(hwnd: HWND) -> Option<WINDOWPLACEMENT> {
    let mut placement = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    unsafe {
        GetWindowPlacement(hwnd, &mut placement).ok()?;
    }
    Some(placement)
}

fn frame_from_placement(value: &WINDOWPLACEMENT, previous: WindowState) -> Option<WindowFrame> {
    let rect = value.rcNormalPosition;
    Some(WindowFrame {
        x: rect.left,
        y: rect.top,
        width: rect.right.checked_sub(rect.left)?,
        height: rect.bottom.checked_sub(rect.top)?,
        // Never reopen minimized. Preserve the last non-minimized state,
        // including a maximized window that was minimized before quitting.
        state: match SHOW_WINDOW_CMD(value.showCmd as i32) {
            SW_SHOWMAXIMIZED => WindowState::Maximized,
            SW_SHOWMINIMIZED | SW_MINIMIZE | SW_SHOWMINNOACTIVE => previous,
            _ => WindowState::Normal,
        },
    })
}

/// Restore while hidden, then observe placement changes. Returns the saved
/// state (if any); the caller applies it when showing the window.
pub fn install(hwnd: HWND, store: FrameStore, fallback: WindowState) -> WindowState {
    let saved = store.load();
    let mut restored = None;
    if let Some(frame) = saved {
        let placement = WINDOWPLACEMENT {
            length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
            showCmd: SW_HIDE.0 as u32,
            rcNormalPosition: RECT {
                left: frame.x,
                top: frame.y,
                right: frame.x + frame.width,
                bottom: frame.y + frame.height,
            },
            ..Default::default()
        };
        // SetWindowPlacement uses workspace coordinates and brings a frame
        // back onto an available monitor after the display setup changes.
        if unsafe { SetWindowPlacement(hwnd, &placement) }.is_ok() {
            restored = Some(frame);
        }
    }
    let state = restored.map_or(fallback, |frame| frame.state);
    let normal_frame = placement(hwnd).and_then(|value| frame_from_placement(&value, state));
    if let Some(mut frame) = normal_frame {
        frame.state = state;
        WINDOWS.with(|windows| {
            windows.borrow_mut().insert(
                hwnd.0 as isize,
                SavedWindow {
                    store,
                    frame,
                    fullscreen: state == WindowState::Fullscreen,
                },
            );
        });
        if !unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) }.as_bool() {
            WINDOWS.with(|windows| windows.borrow_mut().remove(&(hwnd.0 as isize)));
        }
    }
    state
}

fn update(hwnd: HWND, persist: bool) {
    let current = placement(hwnd);
    WINDOWS.with(|windows| {
        let mut windows = windows.borrow_mut();
        let Some(saved) = windows.get_mut(&(hwnd.0 as isize)) else {
            return;
        };
        // Win32 fullscreen replaces the normal rect with the monitor rect.
        // Keep the pre-fullscreen placement so restore-down geometry survives.
        if !saved.fullscreen {
            if let Some(frame) =
                current.and_then(|value| frame_from_placement(&value, saved.frame.state))
            {
                saved.frame = frame;
            }
        }
        if persist {
            let _ = saved.store.save(saved.frame);
        }
    });
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // WinUI's Closed callback exits the process, so save before forwarding
    // WM_CLOSE. WM_QUERYENDSESSION also covers an OS logout/shutdown.
    if matches!(
        message,
        WM_CLOSE | WM_QUERYENDSESSION | WM_DESTROY | WM_EXITSIZEMOVE
    ) {
        update(hwnd, true);
    }
    if message == WM_NCDESTROY {
        WINDOWS.with(|windows| windows.borrow_mut().remove(&(hwnd.0 as isize)));
        let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
    }
    let result = DefSubclassProc(hwnd, message, wparam, lparam);
    if message == WM_SIZE {
        update(hwnd, false);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_close_and_reopen_restores_placement_before_showing() {
        struct Fixture {
            directory: std::path::PathBuf,
            hwnd: HWND,
        }
        impl Fixture {
            fn window(&mut self) -> HWND {
                self.hwnd = unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        windows::core::w!("STATIC"),
                        windows::core::w!("Perry frame persistence test"),
                        WS_OVERLAPPEDWINDOW,
                        200,
                        200,
                        800,
                        600,
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap()
                };
                self.hwnd
            }
            fn close(&mut self) {
                unsafe {
                    SendMessageW(self.hwnd, WM_CLOSE, None, None);
                }
                self.hwnd = HWND::default();
            }
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                if !self.hwnd.0.is_null() {
                    unsafe {
                        let _ = DestroyWindow(self.hwnd);
                    }
                }
                let key = perry_ui::frame::autosave_key("main").unwrap();
                let _ = std::fs::remove_file(self.directory.join(key));
                let _ = std::fs::remove_dir(&self.directory);
            }
        }
        let directory = std::env::temp_dir().join(format!(
            "perry-frame-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut fixture = Fixture {
            directory,
            hwnd: HWND::default(),
        };
        let store = FrameStore::in_directory(&fixture.directory, "main").unwrap();
        let hwnd = fixture.window();
        assert_eq!(
            install(hwnd, store.clone(), WindowState::Normal),
            WindowState::Normal
        );
        assert_eq!(
            store.load(),
            None,
            "opting in must not overwrite preferences during setup"
        );
        unsafe {
            SetWindowPos(hwnd, None, 120, 90, 640, 480, SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
        }
        let expected =
            frame_from_placement(&placement(hwnd).unwrap(), WindowState::Normal).unwrap();
        fixture.close();
        assert_eq!(store.load(), Some(expected));

        let hwnd = fixture.window();
        assert_eq!(
            install(hwnd, store.clone(), WindowState::Maximized),
            WindowState::Normal,
            "saved normal state takes precedence over a maximized launch default"
        );
        assert!(
            !unsafe { IsWindowVisible(hwnd) }.as_bool(),
            "restoring must not show the window"
        );
        assert_eq!(
            frame_from_placement(&placement(hwnd).unwrap(), WindowState::Normal),
            Some(expected)
        );
        fixture.close();

        let fullscreen = WindowFrame {
            state: WindowState::Fullscreen,
            ..expected
        };
        store.save(fullscreen).unwrap();
        let hwnd = fixture.window();
        assert_eq!(
            install(hwnd, store.clone(), WindowState::Normal),
            WindowState::Fullscreen
        );
        // Mimic the fullscreen resize. It must not replace the normal frame.
        unsafe {
            SetWindowPos(hwnd, None, 0, 0, 1920, 1080, SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
        }
        fixture.close();
        assert_eq!(store.load(), Some(fullscreen));
    }

    #[test]
    fn minimized_windows_keep_their_normal_frame_and_previous_state() {
        let mut value = WINDOWPLACEMENT {
            rcNormalPosition: RECT {
                left: -900,
                top: 40,
                right: -100,
                bottom: 640,
            },
            showCmd: SW_SHOWMAXIMIZED.0 as u32,
            ..Default::default()
        };
        let maximized = frame_from_placement(&value, WindowState::Normal).unwrap();
        assert_eq!(maximized.state, WindowState::Maximized);
        assert_eq!(
            (maximized.x, maximized.width, maximized.height),
            (-900, 800, 600)
        );
        value.showCmd = SW_SHOWMINIMIZED.0 as u32;
        assert_eq!(
            frame_from_placement(&value, maximized.state),
            Some(maximized)
        );
        value.showCmd = SW_SHOWNORMAL.0 as u32;
        assert_eq!(
            frame_from_placement(&value, maximized.state).unwrap().state,
            WindowState::Normal
        );
    }
}
