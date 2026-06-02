//! Native bindings for the npm `cheerio` package — jQuery-like HTML
//! parsing and traversal via the `scraper` crate. Sync, handle-based,
//! uses only perry-ffi v0.5 strings + handles + arrays.

use perry_ffi::{
    alloc_string, get_handle, js_array_alloc, js_array_push, read_string, register_handle,
    ArrayHeader, Handle, JsString, JsValue, StringHeader,
};
use scraper::{ElementRef, Html, Selector};

pub struct CheerioHandle {
    pub html: String,
    pub is_fragment: bool,
}

pub struct CheerioSelectionHandle {
    pub html: String,
    pub selector: String,
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

/// # Safety
/// `html_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_load(html_ptr: *const StringHeader) -> Handle {
    let html = match read_str(html_ptr) {
        Some(h) => h,
        None => return -1,
    };
    register_handle(CheerioHandle {
        html,
        is_fragment: false,
    })
}

/// # Safety
/// `html_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_load_fragment(html_ptr: *const StringHeader) -> Handle {
    let html = match read_str(html_ptr) {
        Some(h) => h,
        None => return -1,
    };
    register_handle(CheerioHandle {
        html,
        is_fragment: true,
    })
}

/// # Safety
/// `selector_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_select(
    doc_handle: Handle,
    selector_ptr: *const StringHeader,
) -> Handle {
    let selector_str = match read_str(selector_ptr) {
        Some(s) => s,
        None => return -1,
    };
    if let Some(cheerio) = get_handle::<CheerioHandle>(doc_handle) {
        return register_handle(CheerioSelectionHandle {
            html: cheerio.html.clone(),
            selector: selector_str,
        });
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_cheerio_selection_text(selection_handle: Handle) -> *mut StringHeader {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            let text: String = document
                .select(&selector)
                .map(|el| el.text().collect::<String>())
                .collect::<Vec<_>>()
                .join("");
            return alloc_string(&text).as_raw();
        }
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn js_cheerio_selection_html(selection_handle: Handle) -> *mut StringHeader {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            if let Some(element) = document.select(&selector).next() {
                return alloc_string(&element.inner_html()).as_raw();
            }
        }
    }
    std::ptr::null_mut()
}

/// # Safety
/// `attr_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_attr(
    selection_handle: Handle,
    attr_ptr: *const StringHeader,
) -> *mut StringHeader {
    let attr_name = match read_str(attr_ptr) {
        Some(a) => a,
        None => return std::ptr::null_mut(),
    };
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            if let Some(element) = document.select(&selector).next() {
                if let Some(value) = element.value().attr(&attr_name) {
                    return alloc_string(value).as_raw();
                }
            }
        }
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn js_cheerio_selection_length(selection_handle: Handle) -> f64 {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            return document.select(&selector).count() as f64;
        }
    }
    0.0
}

#[no_mangle]
pub extern "C" fn js_cheerio_selection_first(selection_handle: Handle) -> Handle {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            if let Some(element) = document.select(&selector).next() {
                return register_handle(CheerioSelectionHandle {
                    html: element.html(),
                    selector: "*".to_string(),
                });
            }
        }
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_cheerio_selection_last(selection_handle: Handle) -> Handle {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            if let Some(element) = document.select(&selector).next_back() {
                return register_handle(CheerioSelectionHandle {
                    html: element.html(),
                    selector: "*".to_string(),
                });
            }
        }
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_cheerio_selection_eq(selection_handle: Handle, index: f64) -> Handle {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            if let Some(element) = document.select(&selector).nth(index as usize) {
                return register_handle(CheerioSelectionHandle {
                    html: element.html(),
                    selector: "*".to_string(),
                });
            }
        }
    }
    -1
}

/// # Safety
/// `selector_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_find(
    selection_handle: Handle,
    selector_ptr: *const StringHeader,
) -> Handle {
    let new_selector = match read_str(selector_ptr) {
        Some(s) => s,
        None => return -1,
    };
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let combined = format!("{} {}", selection.selector, new_selector);
        return register_handle(CheerioSelectionHandle {
            html: selection.html.clone(),
            selector: combined,
        });
    }
    -1
}

/// # Safety
/// `selector_ptr` may be null (no filter) or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_children(
    selection_handle: Handle,
    selector_ptr: *const StringHeader,
) -> Handle {
    let filter_selector = read_str(selector_ptr);

    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            let mut children_html = String::new();
            for element in document.select(&selector) {
                for child in element.children() {
                    if let Some(el) = ElementRef::wrap(child) {
                        if let Some(ref filter) = filter_selector {
                            if let Ok(filter_sel) = Selector::parse(filter) {
                                if el.select(&filter_sel).next().is_none() {
                                    continue;
                                }
                            }
                        }
                        children_html.push_str(&el.html());
                    }
                }
            }
            return register_handle(CheerioSelectionHandle {
                html: children_html,
                selector: "*".to_string(),
            });
        }
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_cheerio_selection_parent(selection_handle: Handle) -> Handle {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            let mut parents_html = String::new();
            for element in document.select(&selector) {
                if let Some(parent) = element.parent() {
                    if let Some(parent_el) = ElementRef::wrap(parent) {
                        parents_html.push_str(&parent_el.html());
                    }
                }
            }
            return register_handle(CheerioSelectionHandle {
                html: parents_html,
                selector: "*".to_string(),
            });
        }
    }
    -1
}

/// # Safety
/// `class_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_has_class(
    selection_handle: Handle,
    class_ptr: *const StringHeader,
) -> bool {
    let class_name = match read_str(class_ptr) {
        Some(c) => c,
        None => return false,
    };
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            for element in document.select(&selector) {
                if let Some(classes) = element.value().attr("class") {
                    if classes.split_whitespace().any(|c| c == class_name) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// # Safety
/// `selector_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_is(
    selection_handle: Handle,
    selector_ptr: *const StringHeader,
) -> bool {
    let test_selector = match read_str(selector_ptr) {
        Some(s) => s,
        None => return false,
    };
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            if let Ok(test_sel) = Selector::parse(&test_selector) {
                for element in document.select(&selector) {
                    let el_html = element.html();
                    let el_doc = Html::parse_fragment(&el_html);
                    if el_doc.select(&test_sel).next().is_some() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[no_mangle]
pub extern "C" fn js_cheerio_selection_to_array(selection_handle: Handle) -> *mut ArrayHeader {
    let mut result = unsafe { js_array_alloc(0) };
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            for element in document.select(&selector) {
                let html_str = element.html();
                let s = alloc_string(&html_str);
                result = unsafe { js_array_push(result, JsValue::from_string_ptr(s.as_raw())) };
            }
        }
    }
    result
}

#[no_mangle]
pub extern "C" fn js_cheerio_selection_texts(selection_handle: Handle) -> *mut ArrayHeader {
    let mut result = unsafe { js_array_alloc(0) };
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            for element in document.select(&selector) {
                let text: String = element.text().collect();
                let s = alloc_string(&text);
                result = unsafe { js_array_push(result, JsValue::from_string_ptr(s.as_raw())) };
            }
        }
    }
    result
}

/// # Safety
/// `attr_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_attrs(
    selection_handle: Handle,
    attr_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    let mut result = js_array_alloc(0);
    let attr_name = match read_str(attr_ptr) {
        Some(a) => a,
        None => return result,
    };
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            for element in document.select(&selector) {
                if let Some(value) = element.value().attr(&attr_name) {
                    let s = alloc_string(value);
                    result = js_array_push(result, JsValue::from_string_ptr(s.as_raw()));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_select_text() {
        let html = alloc_string("<html><body><h1>Hello</h1><p>World</p></body></html>");
        let doc = unsafe { js_cheerio_load(html.as_raw()) };
        assert!(doc >= 0);

        let sel = alloc_string("h1");
        let s = unsafe { js_cheerio_select(doc, sel.as_raw()) };
        assert!(s >= 0);

        let text_ptr = js_cheerio_selection_text(s);
        let text = read_string(unsafe { JsString::from_raw(text_ptr) }).expect("text");
        assert_eq!(text, "Hello");
    }

    #[test]
    fn selection_length() {
        let html = alloc_string("<ul><li>a</li><li>b</li><li>c</li></ul>");
        let doc = unsafe { js_cheerio_load(html.as_raw()) };
        let sel = alloc_string("li");
        let s = unsafe { js_cheerio_select(doc, sel.as_raw()) };
        assert_eq!(js_cheerio_selection_length(s), 3.0);
    }

    #[test]
    fn selection_attr() {
        let html = alloc_string(r#"<a href="https://example.com">link</a>"#);
        let doc = unsafe { js_cheerio_load(html.as_raw()) };
        let sel = alloc_string("a");
        let s = unsafe { js_cheerio_select(doc, sel.as_raw()) };
        let attr = alloc_string("href");
        let v_ptr = unsafe { js_cheerio_selection_attr(s, attr.as_raw()) };
        let v = read_string(unsafe { JsString::from_raw(v_ptr) }).expect("href");
        assert_eq!(v, "https://example.com");
    }

    #[test]
    fn has_class() {
        let html = alloc_string(r#"<div class="foo bar">x</div>"#);
        let doc = unsafe { js_cheerio_load(html.as_raw()) };
        let sel = alloc_string("div");
        let s = unsafe { js_cheerio_select(doc, sel.as_raw()) };
        let foo = alloc_string("foo");
        let baz = alloc_string("baz");
        assert!(unsafe { js_cheerio_selection_has_class(s, foo.as_raw()) });
        assert!(!unsafe { js_cheerio_selection_has_class(s, baz.as_raw()) });
    }

    #[test]
    fn first_returns_handle() {
        let html = alloc_string("<ul><li>a</li><li>b</li></ul>");
        let doc = unsafe { js_cheerio_load(html.as_raw()) };
        let sel = alloc_string("li");
        let s = unsafe { js_cheerio_select(doc, sel.as_raw()) };
        let first = js_cheerio_selection_first(s);
        assert!(first >= 0);
    }
}
