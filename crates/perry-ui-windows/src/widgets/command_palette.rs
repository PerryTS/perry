//! Command Palette (issue #477) — Win32 popup window backed by an EDIT
//! search field on top and a LISTBOX of matching commands underneath.
//!
//! Mirrors the macOS NSPanel + NSSearchField + NSTableView shape:
//! - `register(id, label, subtitle, on_run)` adds an entry.
//! - `unregister(id)` / `clear()` remove entries.
//! - `show()` presents a centered borderless popup with focus on the
//!   search field; typing filters the listbox (case-insensitive
//!   substring match on label OR subtitle).
//! - Enter invokes the highlighted command's `on_run`; Esc closes;
//!   arrow keys navigate; clicking a row also runs.
//!
//! Out of scope v1 (matching macOS scope): fuzzy ranking, recents
//! boost, async command sources, command groups, OS-level global
//! hotkey wiring (user binds `commandPaletteShow()` to a shortcut).

use std::cell::RefCell;

#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

#[derive(Clone)]
struct Command {
    id: String,
    label: String,
    subtitle: String,
    on_run: f64,
}

thread_local! {
    static COMMANDS: RefCell<Vec<Command>> = const { RefCell::new(Vec::new()) };
    static FILTERED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static QUERY: RefCell<String> = const { RefCell::new(String::new()) };
    /// Popup HWND, EDIT search HWND, LISTBOX HWND. None when palette is hidden.
    #[cfg(target_os = "windows")]
    static POPUP: RefCell<Option<(HWND, HWND, HWND)>> = const { RefCell::new(None) };
}

fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len)).to_string()
    }
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn register(id_ptr: *const u8, label_ptr: *const u8, subtitle_ptr: *const u8, on_run: f64) {
    let id = str_from_header(id_ptr);
    let label = str_from_header(label_ptr);
    let subtitle = str_from_header(subtitle_ptr);
    if id.is_empty() {
        return;
    }
    COMMANDS.with(|c| {
        let mut cmds = c.borrow_mut();
        if let Some(slot) = cmds.iter_mut().find(|cmd| cmd.id == id) {
            slot.label = label;
            slot.subtitle = subtitle;
            slot.on_run = on_run;
        } else {
            cmds.push(Command {
                id,
                label,
                subtitle,
                on_run,
            });
        }
    });
}

pub fn unregister(id_ptr: *const u8) {
    let id = str_from_header(id_ptr);
    COMMANDS.with(|c| {
        c.borrow_mut().retain(|cmd| cmd.id != id);
    });
}

pub fn clear() {
    COMMANDS.with(|c| c.borrow_mut().clear());
}

fn refresh_filter() {
    let q = QUERY.with(|s| s.borrow().to_lowercase());
    let filtered: Vec<usize> = COMMANDS.with(|c| {
        c.borrow()
            .iter()
            .enumerate()
            .filter_map(|(i, cmd)| {
                if q.is_empty() {
                    Some(i)
                } else {
                    let lab = cmd.label.to_lowercase();
                    let sub = cmd.subtitle.to_lowercase();
                    if lab.contains(&q) || sub.contains(&q) {
                        Some(i)
                    } else {
                        None
                    }
                }
            })
            .collect()
    });
    FILTERED.with(|f| *f.borrow_mut() = filtered.clone());

    #[cfg(target_os = "windows")]
    {
        POPUP.with(|p| {
            if let Some((_, _, list_hwnd)) = *p.borrow() {
                unsafe {
                    SendMessageW(list_hwnd, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
                    COMMANDS.with(|c| {
                        let cmds = c.borrow();
                        for &i in &filtered {
                            if let Some(cmd) = cmds.get(i) {
                                let display = if cmd.subtitle.is_empty() {
                                    cmd.label.clone()
                                } else {
                                    format!("{} — {}", cmd.label, cmd.subtitle)
                                };
                                let wide = to_wide(&display);
                                SendMessageW(
                                    list_hwnd,
                                    LB_ADDSTRING,
                                    WPARAM(0),
                                    LPARAM(wide.as_ptr() as isize),
                                );
                            }
                        }
                    });
                    if !filtered.is_empty() {
                        SendMessageW(list_hwnd, LB_SETCURSEL, WPARAM(0), LPARAM(0));
                    }
                }
            }
        });
    }
}

pub fn show() {
    #[cfg(target_os = "windows")]
    {
        // Already showing? Re-focus + filter.
        let already = POPUP.with(|p| p.borrow().is_some());
        if already {
            QUERY.with(|q| q.borrow_mut().clear());
            refresh_filter();
            POPUP.with(|p| {
                if let Some((popup, edit, _)) = *p.borrow() {
                    unsafe {
                        let _ = ShowWindow(popup, SW_SHOW);
                        let _ = SetFocus(edit);
                    }
                }
            });
            return;
        }
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let class_name = to_wide("PerryCommandPalette");
            // Idempotent class registration.
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(palette_wnd_proc),
                hInstance: hinstance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut _),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                style: CS_DROPSHADOW,
                ..Default::default()
            };
            RegisterClassExW(&wc);

            // Center over the primary monitor.
            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);
            let w = 540;
            let h = 380;
            let x = (screen_w - w) / 2;
            let y = (screen_h - h) / 3;

            let title = to_wide("");
            let popup = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP | WS_BORDER,
                x,
                y,
                w,
                h,
                None,
                None,
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            let edit_class = to_wide("EDIT");
            let edit_text = to_wide("");
            let edit = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(edit_class.as_ptr()),
                PCWSTR(edit_text.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                10,
                10,
                w - 20,
                28,
                popup,
                HMENU(1 as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            let list_class = to_wide("LISTBOX");
            let list_text = to_wide("");
            let list = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(list_class.as_ptr()),
                PCWSTR(list_text.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_BORDER | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
                10,
                48,
                w - 20,
                h - 60,
                popup,
                HMENU(2 as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            POPUP.with(|p| *p.borrow_mut() = Some((popup, edit, list)));
            QUERY.with(|q| q.borrow_mut().clear());
            refresh_filter();

            let _ = ShowWindow(popup, SW_SHOW);
            let _ = SetFocus(edit);
        }
    }
}

pub fn hide() {
    #[cfg(target_os = "windows")]
    {
        POPUP.with(|p| {
            if let Some((popup, _, _)) = p.borrow_mut().take() {
                unsafe {
                    let _ = DestroyWindow(popup);
                }
            }
        });
    }
}

#[cfg(target_os = "windows")]
fn run_selected() {
    let idx_in_listbox = POPUP.with(|p| {
        p.borrow().map(|(_, _, list)| unsafe {
            SendMessageW(list, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as i64
        })
    });
    let Some(idx) = idx_in_listbox else { return };
    if idx < 0 {
        return;
    }
    let cmd_idx_in_full = FILTERED.with(|f| f.borrow().get(idx as usize).copied());
    let Some(cmd_idx) = cmd_idx_in_full else {
        return;
    };
    let on_run = COMMANDS.with(|c| c.borrow().get(cmd_idx).map(|cmd| cmd.on_run));
    let Some(on_run) = on_run else { return };
    hide();
    if on_run == 0.0 {
        return;
    }
    unsafe {
        let closure_ptr = js_nanbox_get_pointer(on_run) as *const u8;
        if !closure_ptr.is_null() {
            js_closure_call0(closure_ptr);
        }
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn palette_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let control_id = (wparam.0 & 0xFFFF) as u16;
            let notify = ((wparam.0 >> 16) & 0xFFFF) as u16;
            // EN_CHANGE = 0x0300 (search field) — re-filter listbox.
            if control_id == 1 && notify == 0x0300 {
                POPUP.with(|p| {
                    if let Some((_, edit, _)) = *p.borrow() {
                        let mut buf = [0u16; 256];
                        let len = GetWindowTextW(edit, &mut buf);
                        let s = String::from_utf16_lossy(&buf[..len as usize]);
                        QUERY.with(|q| *q.borrow_mut() = s);
                    }
                });
                refresh_filter();
            }
            // LBN_DBLCLK = 2 — double-click runs.
            if control_id == 2 && notify == 2 {
                run_selected();
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            // VK_ESCAPE = 0x1B → close.
            if wparam.0 == 0x1B {
                hide();
                return LRESULT(0);
            }
            // VK_RETURN = 0x0D → run.
            if wparam.0 == 0x0D {
                run_selected();
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DESTROY => {
            POPUP.with(|p| *p.borrow_mut() = None);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
