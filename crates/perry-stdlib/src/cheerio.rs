//! Cheerio module
//!
//! Native implementation of the 'cheerio' npm package using scraper.
//! Provides jQuery-like HTML parsing and manipulation.

use crate::common::{get_handle, register_handle, Handle};
use perry_runtime::{js_array_alloc, js_array_push, js_string_from_bytes, JSValue, StringHeader};
use scraper::{ElementRef, Html, Selector};

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Cheerio document handle (stores HTML string for thread safety)
pub struct CheerioHandle {
    pub html: String,
    pub is_fragment: bool,
}

/// Cheerio selection handle (array of elements)
pub struct CheerioSelectionHandle {
    pub html: String,
    pub selector: String,
}

/// cheerio.load(html) -> CheerioAPI
///
/// Load HTML content for parsing. Returns a raw CheerioHandle id; the
/// callable-`$()` form (#1193) is reachable via the codegen's static
/// dispatch table once it learns to map a value of type CheerioHandle to
/// `js_cheerio_select` on a function-call shape. Until then user code
/// can use the equivalent method form `load(html).select(...)` which
/// works through both the static dispatch table and (for any-typed
/// intermediates) the runtime fallback in
/// `common/dispatch.rs::dispatch_cheerio`.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_load(html_ptr: *const StringHeader) -> Handle {
    let html = match string_from_header(html_ptr) {
        Some(h) => h,
        None => return -1,
    };

    register_handle(CheerioHandle {
        html,
        is_fragment: false,
    })
}

/// cheerio.loadFragment(html) -> CheerioAPI
///
/// Load HTML fragment for parsing.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_load_fragment(html_ptr: *const StringHeader) -> Handle {
    let html = match string_from_header(html_ptr) {
        Some(h) => h,
        None => return -1,
    };

    register_handle(CheerioHandle {
        html,
        is_fragment: true,
    })
}

/// $(selector) -> Selection
///
/// Select elements matching the CSS selector.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_select(
    doc_handle: Handle,
    selector_ptr: *const StringHeader,
) -> Handle {
    let selector_str = match string_from_header(selector_ptr) {
        Some(s) => s,
        None => return -1,
    };

    if let Some(cheerio) = get_handle::<CheerioHandle>(doc_handle) {
        // Store the document HTML and selector for later operations
        return register_handle(CheerioSelectionHandle {
            html: cheerio.html.clone(),
            selector: selector_str,
        });
    }
    -1
}

/// selection.text() -> string
///
/// Get the combined text contents of all matched elements.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_text(selection_handle: Handle) -> *mut StringHeader {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            let text: String = document
                .select(&selector)
                .map(|el| el.text().collect::<String>())
                .collect::<Vec<_>>()
                .join("");
            return js_string_from_bytes(text.as_ptr(), text.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// selection.html() -> string | null
///
/// Get the HTML contents of the first matched element.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_html(selection_handle: Handle) -> *mut StringHeader {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            if let Some(element) = document.select(&selector).next() {
                let html = element.inner_html();
                return js_string_from_bytes(html.as_ptr(), html.len() as u32);
            }
        }
    }
    std::ptr::null_mut()
}

/// selection.attr(name) -> string | undefined
///
/// Get the value of an attribute for the first matched element.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_attr(
    selection_handle: Handle,
    attr_ptr: *const StringHeader,
) -> *mut StringHeader {
    let attr_name = match string_from_header(attr_ptr) {
        Some(a) => a,
        None => return std::ptr::null_mut(),
    };

    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            if let Some(element) = document.select(&selector).next() {
                if let Some(value) = element.value().attr(&attr_name) {
                    return js_string_from_bytes(value.as_ptr(), value.len() as u32);
                }
            }
        }
    }
    std::ptr::null_mut()
}

/// selection.length -> number
///
/// Get the number of matched elements.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_length(selection_handle: Handle) -> f64 {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            return document.select(&selector).count() as f64;
        }
    }
    0.0
}

/// selection.first() -> Selection
///
/// Get the first element of the selection.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_first(selection_handle: Handle) -> Handle {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            if let Some(element) = document.select(&selector).next() {
                let html = element.html();
                return register_handle(CheerioSelectionHandle {
                    html,
                    selector: "*".to_string(),
                });
            }
        }
    }
    -1
}

/// selection.last() -> Selection
///
/// Get the last element of the selection.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_last(selection_handle: Handle) -> Handle {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            if let Some(element) = document.select(&selector).next_back() {
                let html = element.html();
                return register_handle(CheerioSelectionHandle {
                    html,
                    selector: "*".to_string(),
                });
            }
        }
    }
    -1
}

/// selection.eq(index) -> Selection
///
/// Get the element at the specified index.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_eq(selection_handle: Handle, index: f64) -> Handle {
    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            if let Some(element) = document.select(&selector).nth(index as usize) {
                let html = element.html();
                return register_handle(CheerioSelectionHandle {
                    html,
                    selector: "*".to_string(),
                });
            }
        }
    }
    -1
}

/// selection.find(selector) -> Selection
///
/// Find descendants matching the selector.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_find(
    selection_handle: Handle,
    selector_ptr: *const StringHeader,
) -> Handle {
    let new_selector = match string_from_header(selector_ptr) {
        Some(s) => s,
        None => return -1,
    };

    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        // Combine selectors for descendant search
        let combined = format!("{} {}", selection.selector, new_selector);
        return register_handle(CheerioSelectionHandle {
            html: selection.html.clone(),
            selector: combined,
        });
    }
    -1
}

/// selection.children(selector?) -> Selection
///
/// Get the children of each element.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_children(
    selection_handle: Handle,
    selector_ptr: *const StringHeader,
) -> Handle {
    let filter_selector = string_from_header(selector_ptr);

    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            let mut children_html = String::new();

            for element in document.select(&selector) {
                for child in element.children() {
                    if let Some(el) = ElementRef::wrap(child) {
                        // If filter selector provided, check if child matches
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

/// selection.parent() -> Selection
///
/// Get the parent of each element.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_parent(selection_handle: Handle) -> Handle {
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

/// selection.hasClass(className) -> boolean
///
/// Check if any of the matched elements have the given class.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_has_class(
    selection_handle: Handle,
    class_ptr: *const StringHeader,
) -> bool {
    let class_name = match string_from_header(class_ptr) {
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

/// selection.is(selector) -> boolean
///
/// Check if any of the matched elements match the selector.
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_is(
    selection_handle: Handle,
    selector_ptr: *const StringHeader,
) -> bool {
    let test_selector = match string_from_header(selector_ptr) {
        Some(s) => s,
        None => return false,
    };

    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            if let Ok(test_sel) = Selector::parse(&test_selector) {
                for element in document.select(&selector) {
                    // Check if element matches the test selector
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

/// selection.each(fn) - iterate over elements
/// Returns an array of HTML strings for each element
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_to_array(
    selection_handle: Handle,
) -> *mut perry_runtime::ArrayHeader {
    let result = js_array_alloc(0);

    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            for element in document.select(&selector) {
                let html = element.html();
                let ptr = js_string_from_bytes(html.as_ptr(), html.len() as u32);
                js_array_push(result, JSValue::string_ptr(ptr));
            }
        }
    }

    result
}

/// selection.map(fn) - get array of texts
/// Returns an array of text content for each element
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_texts(
    selection_handle: Handle,
) -> *mut perry_runtime::ArrayHeader {
    let result = js_array_alloc(0);

    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            for element in document.select(&selector) {
                let text: String = element.text().collect();
                let ptr = js_string_from_bytes(text.as_ptr(), text.len() as u32);
                js_array_push(result, JSValue::string_ptr(ptr));
            }
        }
    }

    result
}

/// Get all attribute values for an attribute across all matched elements
#[no_mangle]
pub unsafe extern "C" fn js_cheerio_selection_attrs(
    selection_handle: Handle,
    attr_ptr: *const StringHeader,
) -> *mut perry_runtime::ArrayHeader {
    let result = js_array_alloc(0);

    let attr_name = match string_from_header(attr_ptr) {
        Some(a) => a,
        None => return result,
    };

    if let Some(selection) = get_handle::<CheerioSelectionHandle>(selection_handle) {
        let document = Html::parse_document(&selection.html);
        if let Ok(selector) = Selector::parse(&selection.selector) {
            for element in document.select(&selector) {
                if let Some(value) = element.value().attr(&attr_name) {
                    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
                    js_array_push(result, JSValue::string_ptr(ptr));
                }
            }
        }
    }

    result
}

// ============================================================================
// #1193 — runtime fall-through dispatch for cheerio handles.
//
// The static NATIVE_MODULE_TABLE path resolves `cheerio.load(html).select(sel)`
// when the receiver type survives lowering. As soon as user code lands an
// intermediate in a `let` binding (`const sel = $.select(".x")`), the
// codegen sees a generic `(number).method` call and routes through
// `js_handle_method_dispatch` — which until this commit had no cheerio
// arm, so `sel.text()` failed with `(number).text is not a function`.
//
// This helper accepts both `CheerioHandle` (the document) and
// `CheerioSelectionHandle` (selection) shapes and dispatches the methods
// the static table already exposes. Returns `None` when the handle id
// doesn't belong to either registry so the caller falls through to the
// next dispatcher.
// ============================================================================
// #854: runtime fallback dispatcher for any-typed cheerio intermediates; the
// codegen path that routes here (see module doc above) is not yet wired, so
// this is currently unreferenced but intentionally retained.
#[allow(dead_code)]
pub(crate) unsafe fn dispatch_cheerio(handle: Handle, method: &str, args: &[f64]) -> Option<f64> {
    use crate::common::with_handle;
    const TAG_UNDEFINED_F64: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL_F64: u64 = 0x7FFC_0000_0000_0002;

    let nanbox_pointer = |raw: i64| -> f64 {
        f64::from_bits(0x7FFD_0000_0000_0000 | (raw as u64 & 0x0000_FFFF_FFFF_FFFF))
    };
    let nanbox_string = |ptr: *mut StringHeader| -> f64 {
        if ptr.is_null() {
            return f64::from_bits(TAG_NULL_F64);
        }
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    };
    let arg_str_ptr = |idx: usize| -> *const StringHeader {
        if idx >= args.len() {
            return std::ptr::null();
        }
        perry_runtime::js_get_string_pointer_unified(args[idx]) as *const StringHeader
    };

    let is_selection =
        with_handle::<CheerioSelectionHandle, bool, _>(handle, |_| true).unwrap_or(false);
    let is_document = with_handle::<CheerioHandle, bool, _>(handle, |_| true).unwrap_or(false);
    if !is_selection && !is_document {
        return None;
    }

    // Document-level methods. `select` is the only thing user code reaches
    // for the document; the rest of the table is selection-only.
    if is_document && method == "select" {
        let raw = js_cheerio_select(handle, arg_str_ptr(0));
        return Some(nanbox_pointer(raw));
    }
    if !is_selection {
        return None;
    }

    match method {
        "text" => Some(nanbox_string(js_cheerio_selection_text(handle))),
        "html" => Some(nanbox_string(js_cheerio_selection_html(handle))),
        "attr" => Some(nanbox_string(js_cheerio_selection_attr(
            handle,
            arg_str_ptr(0),
        ))),
        "length" => Some(js_cheerio_selection_length(handle)),
        "first" => Some(nanbox_pointer(js_cheerio_selection_first(handle))),
        "last" => Some(nanbox_pointer(js_cheerio_selection_last(handle))),
        "eq" => {
            let idx = if args.is_empty() { 0.0 } else { args[0] };
            Some(nanbox_pointer(js_cheerio_selection_eq(handle, idx)))
        }
        "find" => Some(nanbox_pointer(js_cheerio_selection_find(
            handle,
            arg_str_ptr(0),
        ))),
        "children" => Some(nanbox_pointer(js_cheerio_selection_children(
            handle,
            arg_str_ptr(0),
        ))),
        "parent" => Some(nanbox_pointer(js_cheerio_selection_parent(handle))),
        "hasClass" => {
            let r = js_cheerio_selection_has_class(handle, arg_str_ptr(0));
            Some(f64::from_bits(JSValue::bool(r).bits()))
        }
        "is" => {
            let r = js_cheerio_selection_is(handle, arg_str_ptr(0));
            Some(f64::from_bits(JSValue::bool(r).bits()))
        }
        "toArray" => {
            let arr = js_cheerio_selection_to_array(handle);
            if arr.is_null() {
                Some(f64::from_bits(TAG_UNDEFINED_F64))
            } else {
                Some(nanbox_pointer(arr as i64))
            }
        }
        "texts" => {
            let arr = js_cheerio_selection_texts(handle);
            if arr.is_null() {
                Some(f64::from_bits(TAG_UNDEFINED_F64))
            } else {
                Some(nanbox_pointer(arr as i64))
            }
        }
        "attrs" => {
            let arr = js_cheerio_selection_attrs(handle, arg_str_ptr(0));
            if arr.is_null() {
                Some(f64::from_bits(TAG_UNDEFINED_F64))
            } else {
                Some(nanbox_pointer(arr as i64))
            }
        }
        _ => None,
    }
}
