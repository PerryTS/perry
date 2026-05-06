//! GTK4 Command palette — floating `GtkWindow` with `GtkSearchEntry`
//! on top and a `GtkListBox` of filtered commands below (issue #477 /
//! Linux parity work).
//!
//! GTK4 has no menu-bar-equivalent for ad-hoc ⌘K palettes, so we
//! build the same shape as the macOS impl: borderless decoration-less
//! Window, `set_modal(true)` so it captures focus, GtkSearchEntry +
//! GtkListBox composing the search-and-pick UX. Substring filter
//! (case-insensitive) over `label` and `subtitle`. Activate-row →
//! `js_closure_call0` the registered `on_run` closure and dismiss.

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

struct Command {
    id: String,
    label: String,
    subtitle: String,
    on_run: f64,
}

struct PaletteState {
    commands: Vec<Command>,
    filtered: Vec<usize>,
    query: String,
    window: Option<gtk4::Window>,
    listbox: Option<gtk4::ListBox>,
}

thread_local! {
    static STATE: RefCell<PaletteState> = RefCell::new(PaletteState {
        commands: Vec::new(),
        filtered: Vec::new(),
        query: String::new(),
        window: None,
        listbox: None,
    });
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

fn refresh_filter() {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        let q = state.query.to_lowercase();
        let filtered: Vec<usize> = state
            .commands
            .iter()
            .enumerate()
            .filter_map(|(i, cmd)| {
                if q.is_empty() {
                    Some(i)
                } else {
                    let l = cmd.label.to_lowercase();
                    let st = cmd.subtitle.to_lowercase();
                    if l.contains(&q) || st.contains(&q) {
                        Some(i)
                    } else {
                        None
                    }
                }
            })
            .collect();
        state.filtered = filtered;

        if let Some(listbox) = state.listbox.clone() {
            // Drop existing rows + re-populate. ListBox rebuilds are
            // cheap relative to user keystroke cadence.
            while let Some(child) = listbox.first_child() {
                listbox.remove(&child);
            }
            for &cmd_idx in &state.filtered {
                let Some(cmd) = state.commands.get(cmd_idx) else { continue };
                let row = gtk4::ListBoxRow::new();
                let label_text = if cmd.subtitle.is_empty() {
                    cmd.label.clone()
                } else {
                    format!("{}    {}", cmd.label, cmd.subtitle)
                };
                let label = gtk4::Label::new(Some(&label_text));
                label.set_halign(gtk4::Align::Start);
                label.set_margin_top(4);
                label.set_margin_bottom(4);
                label.set_margin_start(10);
                label.set_margin_end(10);
                row.set_child(Some(&label));
                // Stash the command index on the row so the activate
                // handler can look up the right `on_run`.
                unsafe {
                    row.set_data::<usize>("cmd_idx", cmd_idx);
                }
                listbox.append(&row);
            }
        }
    });
}

pub fn register(id_ptr: *const u8, label_ptr: *const u8, subtitle_ptr: *const u8, on_run: f64) {
    let id = str_from_header(id_ptr);
    let label = str_from_header(label_ptr);
    let subtitle = str_from_header(subtitle_ptr);
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        if let Some(existing) = state.commands.iter_mut().find(|x| x.id == id) {
            existing.label = label;
            existing.subtitle = subtitle;
            existing.on_run = on_run;
        } else {
            state.commands.push(Command {
                id,
                label,
                subtitle,
                on_run,
            });
        }
    });
    refresh_filter();
}

pub fn unregister(id_ptr: *const u8) {
    let id = str_from_header(id_ptr);
    STATE.with(|s| s.borrow_mut().commands.retain(|c| c.id != id));
    refresh_filter();
}

pub fn clear() {
    STATE.with(|s| s.borrow_mut().commands.clear());
    refresh_filter();
}

pub fn show() {
    crate::app::ensure_gtk_init();
    if STATE.with(|s| s.borrow().window.is_some()) {
        return;
    }
    STATE.with(|s| s.borrow_mut().query.clear());

    let win = gtk4::Window::new();
    win.set_title(Some("Command Palette"));
    win.set_decorated(false);
    win.set_modal(true);
    win.set_default_width(480);
    win.set_default_height(380);

    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    outer.set_margin_top(12);
    outer.set_margin_bottom(12);
    outer.set_margin_start(12);
    outer.set_margin_end(12);

    let search = gtk4::SearchEntry::new();
    search.set_placeholder_text(Some("Type a command…"));
    outer.append(&search);

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_vexpand(true);
    let listbox = gtk4::ListBox::new();
    listbox.set_selection_mode(gtk4::SelectionMode::Single);
    scroll.set_child(Some(&listbox));
    outer.append(&scroll);

    win.set_child(Some(&outer));

    // Search-changed → update query + refilter.
    search.connect_search_changed(|entry| {
        let q = entry.text().to_string();
        STATE.with(|s| s.borrow_mut().query = q);
        refresh_filter();
    });

    // Enter in search entry → activate the first filtered row.
    let listbox_for_activate = listbox.clone();
    search.connect_activate(move |_| {
        if let Some(row) = listbox_for_activate.row_at_index(0) {
            listbox_for_activate.emit_by_name::<()>("row-activated", &[&row]);
        }
    });

    // Click / Enter on a row → invoke the command.
    listbox.connect_row_activated(|_, row| {
        let cmd_idx = unsafe { row.data::<usize>("cmd_idx") }.map(|nn| *unsafe { nn.as_ref() });
        let on_run = STATE.with(|s| {
            cmd_idx
                .and_then(|i| s.borrow().commands.get(i).map(|c| c.on_run))
                .unwrap_or(0.0)
        });
        if on_run != 0.0 {
            unsafe {
                let closure_ptr = js_nanbox_get_pointer(on_run) as *const u8;
                js_closure_call0(closure_ptr);
            }
        }
        hide();
    });

    // Esc → dismiss.
    let key = gtk4::EventControllerKey::new();
    key.connect_key_pressed(|_, keyval, _, _| {
        if keyval == gtk4::gdk::Key::Escape {
            hide();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    win.add_controller(key);

    let win_clone = win.clone();
    let listbox_clone = listbox.clone();
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.window = Some(win_clone);
        state.listbox = Some(listbox_clone);
    });
    win.present();
    refresh_filter();
    let _ = Rc::new(()); // silence unused-import if it ever fires
}

pub fn hide() {
    let (win, _) = STATE.with(|s| {
        let mut state = s.borrow_mut();
        let w = state.window.take();
        let lb = state.listbox.take();
        (w, lb)
    });
    if let Some(w) = win {
        w.close();
    }
}
