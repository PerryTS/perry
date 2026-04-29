use gtk4::prelude::*;
use gtk4::StringList;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static PICKER_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static NEXT_PICKER_ID: RefCell<usize> = RefCell::new(1);
    /// Store DropDown references for add_item/set_selected/get_selected
    static PICKER_DROPDOWNS: RefCell<HashMap<i64, gtk4::DropDown>> = RefCell::new(HashMap::new());
    /// Store string lists per picker handle for adding items
    static PICKER_MODELS: RefCell<HashMap<i64, StringList>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Create a Picker (dropdown). label_ptr is unused in GTK4 (DropDown has no label).
/// style is ignored (GTK4 always uses dropdown style).
pub fn create(_label_ptr: *const u8, on_change: f64, _style: i64) -> i64 {
    crate::app::ensure_gtk_init();
    let model = StringList::new(&[]);
    let dropdown = gtk4::DropDown::new(Some(model.clone()), None::<gtk4::Expression>);

    let callback_id = NEXT_PICKER_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    PICKER_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(callback_id, on_change);
    });

    dropdown.connect_selected_notify(move |dd| {
        let closure_f64 = PICKER_CALLBACKS.with(|cbs| cbs.borrow().get(&callback_id).copied());
        if let Some(closure_f64) = closure_f64 {
            let selected = dd.selected() as f64;
            let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
            unsafe {
                js_closure_call1(closure_ptr as *const u8, selected);
            }
        }
    });

    let handle = super::register_widget(dropdown.clone().upcast());
    PICKER_DROPDOWNS.with(|p| p.borrow_mut().insert(handle, dropdown));
    PICKER_MODELS.with(|m| m.borrow_mut().insert(handle, model));
    handle
}

/// Add an item to a picker.
pub fn add_item(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    PICKER_MODELS.with(|m| {
        if let Some(model) = m.borrow().get(&handle) {
            model.append(title);
        }
    });
}

/// Set the selected item index.
pub fn set_selected(handle: i64, index: i64) {
    PICKER_DROPDOWNS.with(|p| {
        if let Some(dd) = p.borrow().get(&handle) {
            dd.set_selected(index as u32);
        }
    });
}

/// Get the selected item index.
pub fn get_selected(handle: i64) -> i64 {
    PICKER_DROPDOWNS.with(|p| {
        if let Some(dd) = p.borrow().get(&handle) {
            dd.selected() as i64
        } else {
            0
        }
    })
}
