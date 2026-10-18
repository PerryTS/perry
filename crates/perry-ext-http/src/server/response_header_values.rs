//! Outgoing header directory and values. The dense native directory addresses
//! slots in the response's existing traced private state. On payload release its names move
//! into the private JS state alongside those same values.
use super::super::response_payload::{self as heap, Key};
use super::super::types::{read_string_header, TAG_TRUE, TAG_UNDEFINED};
use super::{
    is_legacy_response_handle, response_state, response_state_mut, ResponseState, ServerResponse,
};
use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, ArrayHeader, JsValue, ObjectHeader, StringHeader,
    TransientRootScope,
};
use std::collections::HashMap;

#[derive(Clone)]
struct HeaderName {
    lower: String,
    original: String,
}
#[derive(Default)]
pub(crate) struct HeaderFields {
    entries: Vec<HeaderName>,
    positions: HashMap<String, u32>,
}
impl HeaderFields {
    pub(crate) fn retained_bytes(&self) -> usize {
        self.entries.capacity() * std::mem::size_of::<HeaderName>()
            + self
                .entries
                .iter()
                .map(|n| n.lower.capacity() + n.original.capacity())
                .sum::<usize>()
            + self.positions.capacity() * std::mem::size_of::<(String, u32)>()
            + self.positions.keys().map(String::capacity).sum::<usize>()
    }
    fn remove(&mut self, index: u32) {
        let removed = self.entries.remove(index as usize);
        self.positions.remove(&removed.lower);
        for (i, name) in self.entries.iter().enumerate().skip(index as usize) {
            *self.positions.get_mut(&name.lower).unwrap() = i as u32;
        }
    }
}
extern "C" {
    fn js_object_alloc_null_proto_entries(
        keys: *const *const u8,
        lengths: *const usize,
        values: *const f64,
        count: u32,
    ) -> *mut ObjectHeader;
    fn js_object_alloc_null_proto(class_id: u32, fields: u32) -> *mut ObjectHeader;
    fn js_object_get_field_by_name(obj: *const ObjectHeader, key: *const StringHeader) -> JsValue;
    fn js_array_is_array(value: f64) -> f64;
    fn js_object_keys_value(value: f64) -> *mut ArrayHeader;
    fn js_jsvalue_to_string(value: f64) -> *mut StringHeader;
    fn js_array_pop_f64(arr: *mut ArrayHeader) -> f64;
}
fn boxed<T>(p: *mut T) -> f64 {
    f64::from_bits(JsValue::from_object_ptr(p).bits())
}
fn undef() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}
fn ptr<T>(v: f64) -> *mut T {
    JsValue::from_bits(v.to_bits()).as_pointer::<T>()
}
pub(crate) fn is_array(v: f64) -> bool {
    unsafe { js_array_is_array(v).to_bits() == TAG_TRUE }
}
pub(crate) fn string(v: f64) -> String {
    if let Some(s) = JsValue::from_bits(v.to_bits()).to_owned_string() {
        return s;
    }
    unsafe { read_string_header(js_jsvalue_to_string(v)).unwrap_or_default() }
}
fn string_value(s: &str) -> f64 {
    f64::from_bits(JsValue::from_string_ptr(alloc_string(s).as_raw()).bits())
}
pub(crate) fn array_get(v: f64, i: u32) -> f64 {
    unsafe { f64::from_bits(perry_ffi::js_array_get(ptr(v), i).bits()) }
}
pub(crate) fn array_len(v: f64) -> u32 {
    unsafe { perry_ffi::js_array_length(ptr(v)) }
}
fn array(values: &[f64]) -> f64 {
    if values.is_empty() {
        return boxed(unsafe { perry_ffi::js_array_alloc(0) });
    }
    let scope = TransientRootScope::enter();
    let values: Vec<_> = values.iter().map(|v| scope.root_nanbox(*v)).collect();
    let mut a = scope.root_nanbox(boxed(unsafe {
        perry_ffi::js_array_alloc(values.len() as u32)
    }));
    for v in values {
        unsafe {
            a = scope.root_nanbox(boxed(perry_ffi::js_array_push(
                ptr(a.get()),
                JsValue::from_bits(v.get().to_bits()),
            )));
        }
    }
    a.get()
}
fn object() -> f64 {
    boxed(unsafe { js_object_alloc_null_proto(0, 0) })
}
pub(crate) fn names(obj: f64) -> f64 {
    boxed(unsafe { js_object_keys_value(obj) })
}

/// This copies only slots/names from state before any allocating operation.
/// Private state and value-array reads do not invoke JS or allocate.
fn data(handle: i64) -> (Option<&'static ResponseState>, f64) {
    if is_legacy_response_handle(handle) {
        get_handle::<ServerResponse>(handle)
            .map_or((None, undef()), |r| (Some(&r.state), r.header_values))
    } else {
        let state = unsafe { heap::state(handle) }.map(|s| &*s);
        (state, heap::existing_state(handle))
    }
}
pub(crate) fn records(handle: i64) -> f64 {
    if is_legacy_response_handle(handle) {
        get_handle::<ServerResponse>(handle).map_or(undef(), |r| r.header_values)
    } else {
        heap::existing_state(handle)
    }
}
fn closed_names(handle: i64) -> f64 {
    if is_legacy_response_handle(handle) {
        undef()
    } else {
        heap::get_existing(handle, Key::HeaderNames)
    }
}
fn offset(handle: i64) -> u32 {
    if is_legacy_response_handle(handle) {
        0
    } else {
        heap::HEADER_VALUES_START
    }
}
fn store_records(handle: i64, values: f64) {
    if is_legacy_response_handle(handle) {
        super::super::ensure_gc_scanner_registered();
        if let Some(r) = get_handle_mut::<ServerResponse>(handle) {
            r.header_values = values;
        }
    } else {
        heap::store_state(handle, values);
    }
}
/// Growth may replace an ArrayHeader; keep the owner's direct edge current.
fn push_record(
    owner: perry_ffi::TransientRootedAddr,
    values: perry_ffi::TransientRootedNanbox,
    value: perry_ffi::TransientRootedNanbox,
) {
    let updated = boxed(unsafe {
        perry_ffi::js_array_push(ptr(values.get()), JsValue::from_bits(value.get().to_bits()))
    });
    if updated.to_bits() != values.get().to_bits() {
        store_records(owner.get(), updated);
    }
}
fn closed_position(names: f64, lower: &str) -> Option<u32> {
    if names.to_bits() == TAG_UNDEFINED {
        return None;
    }
    (0..array_len(names)).find(|i| {
        JsValue::from_bits(array_get(names, *i).to_bits())
            .to_owned_string()
            .as_deref()
            == Some(lower)
    })
}
pub(crate) fn set(handle: i64, name: &str, value: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let value = scope.root_nanbox(value);
    let lower = name.to_ascii_lowercase();
    let (state, old) = data(owner.get());
    let open_slot = state.map(|s| {
        s.header_fields
            .as_ref()
            .and_then(|f| f.positions.get(&lower).copied())
    });
    let start = offset(owner.get());
    let values = scope.root_nanbox(if old.to_bits() == TAG_UNDEFINED {
        if is_legacy_response_handle(owner.get()) {
            array(&[])
        } else {
            heap::js_state(owner.get())
        }
    } else {
        old
    });
    if old.to_bits() == TAG_UNDEFINED && is_legacy_response_handle(owner.get()) {
        store_records(owner.get(), values.get());
    }
    if let Some(slot) = open_slot {
        if let Some(slot) = slot {
            unsafe {
                perry_ffi::js_array_set(
                    ptr(values.get()),
                    start + slot,
                    JsValue::from_bits(value.get().to_bits()),
                );
            }
            if let Some(state) = response_state_mut(owner.get()) {
                state.header_fields.as_mut().unwrap().entries[slot as usize].original =
                    name.to_string();
            }
        } else {
            let slot = array_len(values.get()) - start;
            push_record(owner, values, value);
            if let Some(state) = response_state_mut(owner.get()) {
                let fields = state
                    .header_fields
                    .get_or_insert_with(HeaderFields::default);
                fields.positions.insert(lower.clone(), slot);
                fields.entries.push(HeaderName {
                    lower,
                    original: name.to_string(),
                });
            }
        }
        heap::restate(owner.get());
    } else {
        let old_names = closed_names(owner.get());
        let names = scope.root_nanbox(if old_names.to_bits() == TAG_UNDEFINED {
            array(&[])
        } else {
            old_names
        });
        if old_names.to_bits() == TAG_UNDEFINED {
            heap::set(owner.get(), Key::HeaderNames, names.get());
        }
        if let Some(slot) = closed_position(names.get(), &lower) {
            unsafe {
                perry_ffi::js_array_set(
                    ptr(values.get()),
                    start + slot,
                    JsValue::from_bits(value.get().to_bits()),
                );
            }
        } else {
            let key = scope.root_nanbox(string_value(&lower));
            let updated = boxed(unsafe {
                perry_ffi::js_array_push(ptr(names.get()), JsValue::from_bits(key.get().to_bits()))
            });
            if updated.to_bits() != names.get().to_bits() {
                heap::set(owner.get(), Key::HeaderNames, updated);
            }
            push_record(owner, values, value);
        }
    }
}
pub(crate) fn lookup(handle: i64, name: &str) -> f64 {
    let (state, values) = data(handle);
    let slot = match state {
        Some(s) => s
            .header_fields
            .as_ref()
            .and_then(|f| f.positions.get(name).copied()),
        None => closed_position(closed_names(handle), name),
    };
    slot.map_or(undef(), |i| array_get(values, offset(handle) + i))
}
pub(crate) fn has(handle: i64, name: &str) -> bool {
    if let Some(s) = response_state(handle) {
        s.header_fields
            .as_ref()
            .is_some_and(|f| f.positions.contains_key(name))
    } else {
        closed_position(closed_names(handle), name).is_some()
    }
}
fn erase(array: f64, index: u32) {
    let scope = TransientRootScope::enter();
    let a = scope.root_nanbox(array);
    for i in index..array_len(a.get()).saturating_sub(1) {
        let value = scope.root_nanbox(array_get(a.get(), i + 1));
        unsafe {
            perry_ffi::js_array_set(ptr(a.get()), i, JsValue::from_bits(value.get().to_bits()));
        }
    }
    unsafe {
        js_array_pop_f64(ptr(a.get()));
    }
}
pub(crate) fn remove(handle: i64, name: &str) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let (state, values) = data(owner.get());
    let open = state.map(|s| {
        s.header_fields
            .as_ref()
            .and_then(|f| f.positions.get(name).copied())
    });
    let values = scope.root_nanbox(values);
    if let Some(slot) = open {
        if let Some(i) = slot {
            erase(values.get(), offset(owner.get()) + i);
            if let Some(s) = response_state_mut(owner.get()) {
                s.header_fields.as_mut().unwrap().remove(i);
            }
            heap::restate(owner.get());
        }
    } else {
        let names = scope.root_nanbox(closed_names(owner.get()));
        if let Some(i) = closed_position(names.get(), name) {
            erase(values.get(), offset(owner.get()) + i);
            erase(names.get(), i);
        }
    }
}
pub(crate) fn append(handle: i64, name: &str, value: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let value = scope.root_nanbox(value);
    let lower = name.to_ascii_lowercase();
    let old = scope.root_nanbox(lookup(owner.get(), &lower));
    if old.get().to_bits() == TAG_UNDEFINED {
        set(owner.get(), name, value.get());
        return;
    }
    let target = if is_array(old.get()) {
        old.get()
    } else {
        let spelling = response_state(owner.get())
            .and_then(|s| {
                s.header_fields.as_ref().and_then(|f| {
                    f.positions
                        .get(&lower)
                        .map(|i| f.entries[*i as usize].original.clone())
                })
            })
            .unwrap_or_else(|| name.to_string());
        let a = scope.root_nanbox(array(&[old.get()]));
        set(owner.get(), &spelling, a.get());
        a.get()
    };
    let initial_target = scope.root_nanbox(target);
    let mut target = initial_target;
    if is_array(value.get()) {
        let len = array_len(value.get());
        for i in 0..len {
            let item = scope.root_nanbox(array_get(value.get(), i));
            target = scope.root_nanbox(boxed(unsafe {
                perry_ffi::js_array_push(
                    ptr(target.get()),
                    JsValue::from_bits(item.get().to_bits()),
                )
            }));
        }
    } else {
        target = scope.root_nanbox(boxed(unsafe {
            perry_ffi::js_array_push(ptr(target.get()), JsValue::from_bits(value.get().to_bits()))
        }));
    }
    if target.get().to_bits() != initial_target.get().to_bits() {
        let spelling = response_state(owner.get())
            .and_then(|s| {
                s.header_fields.as_ref().and_then(|f| {
                    f.positions
                        .get(&lower)
                        .map(|i| f.entries[*i as usize].original.clone())
                })
            })
            .unwrap_or_else(|| name.to_string());
        set(owner.get(), &spelling, target.get());
    }
}
fn key_array(names: &[String]) -> f64 {
    let scope = TransientRootScope::enter();
    let mut keys = scope.root_nanbox(array(&[]));
    for name in names {
        let key = scope.root_nanbox(string_value(name));
        unsafe {
            keys = scope.root_nanbox(boxed(perry_ffi::js_array_push(
                ptr(keys.get()),
                JsValue::from_bits(key.get().to_bits()),
            )));
        }
    }
    keys.get()
}
fn lower_names(handle: i64) -> Vec<String> {
    if let Some(s) = response_state(handle) {
        return s
            .header_fields
            .as_ref()
            .map(|f| f.entries.iter().map(|e| e.lower.clone()).collect())
            .unwrap_or_default();
    }
    let keys = closed_names(handle);
    if keys.to_bits() == TAG_UNDEFINED {
        Vec::new()
    } else {
        (0..array_len(keys))
            .map(|i| {
                JsValue::from_bits(array_get(keys, i).to_bits())
                    .to_owned_string()
                    .unwrap()
            })
            .collect()
    }
}
pub(crate) fn get_headers(handle: i64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let names = lower_names(owner.get());
    if names.is_empty() {
        return object();
    }
    let values = records(owner.get());
    let start = offset(owner.get());
    let keys: Vec<_> = names.iter().map(|n| n.as_ptr()).collect();
    let lengths: Vec<_> = names.iter().map(String::len).collect();
    let fields: Vec<_> = (0..names.len() as u32)
        .map(|i| array_get(values, start + i))
        .collect();
    // No GC allocation occurs while gathering these native spans and bits.
    // The runtime's callback-free birth retains them under its construction
    // guard, then publishes the whole final layout once.
    boxed(unsafe {
        js_object_alloc_null_proto_entries(
            keys.as_ptr(),
            lengths.as_ptr(),
            fields.as_ptr(),
            fields.len() as u32,
        )
    })
}
fn array_index(name: &str) -> Option<u32> {
    if name.is_empty()
        || !name.bytes().all(|b| b.is_ascii_digit())
        || (name.len() > 1 && name.starts_with('0'))
    {
        return None;
    }
    name.parse::<u32>().ok().filter(|i| *i != u32::MAX)
}
pub(crate) fn get_names(handle: i64) -> f64 {
    let mut names = lower_names(handle);
    names.sort_by(|a, b| match (array_index(a), array_index(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    key_array(&names)
}
/// Native metadata is released; names now become part of the owned JS state.
/// The value array itself, and every original value in it, stay in place.
pub(crate) fn close_headers(handle: i64) {
    if !response_state(handle).is_some_and(|s| s.header_fields.is_some()) {
        return;
    }
    let scope = TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let names = lower_names(owner.get());
    let keys = key_array(&names);
    heap::set(owner.get(), Key::HeaderNames, keys);
}

/// End consumes the committed wire artifact. Original JS values remain on
/// the response, independently of this transport-owned representation.
pub(crate) fn take_wire_headers(handle: i64) -> Vec<(String, String)> {
    if let Some(headers) = response_state_mut(handle).and_then(|s| s.committed_headers.take()) {
        headers
    } else {
        snapshot(handle)
    }
}

/// Render originals only when committing a head. No stringify/parse roundtrip:
/// array identity and changes made before writeHead are therefore observable.
pub(crate) fn snapshot(handle: i64) -> Vec<(String, String)> {
    if let Some(headers) = response_state(handle).and_then(|s| s.committed_headers.as_ref()) {
        return headers.clone();
    }
    let scope = TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let mut native = response_state(owner.get())
        .map(|s| s.snapshot_headers())
        .unwrap_or_default();
    let entries = response_state(owner.get())
        .and_then(|s| s.header_fields.as_ref().map(|f| f.entries.clone()))
        .unwrap_or_default();
    if entries.is_empty() {
        return native;
    }
    let values = scope.root_nanbox(records(owner.get()));
    let mut order: Vec<_> = entries.into_iter().enumerate().collect();
    order.sort_by(
        |(_, a), (_, b)| match (array_index(&a.lower), array_index(&b.lower)) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        },
    );
    let mut result = Vec::new();
    for (index, name) in order {
        native.retain(|(n, _)| !n.eq_ignore_ascii_case(&name.lower));
        let value = scope.root_nanbox(array_get(values.get(), offset(owner.get()) + index as u32));
        render_value(&name.original, value.get(), &mut result);
    }
    result.extend(native);
    result
}
/// writeHead merges into a previously created outgoing map. With no map, node
/// renders its argument directly and getters continue to report no headers.
pub(crate) fn apply_write_head(handle: i64, headers: f64) -> Option<Vec<(String, String)>> {
    let scope = TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let headers = scope.root_nanbox(headers);
    let existing = response_state(owner.get()).map_or_else(
        || closed_names(owner.get()).to_bits() != TAG_UNDEFINED,
        |s| s.header_fields.is_some(),
    );
    let mut direct = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let pairs = if is_array(headers.get()) {
        None
    } else {
        Some(scope.root_nanbox(names(headers.get())))
    };
    let len = pairs.as_ref().map_or_else(
        || array_len(headers.get()) / 2,
        |keys| array_len(keys.get()),
    );
    for i in 0..len {
        let key = scope.root_nanbox(match &pairs {
            Some(keys) => array_get(keys.get(), i),
            None => array_get(headers.get(), 2 * i),
        });
        let key = scope.root_addr(unsafe { js_jsvalue_to_string(key.get()) } as i64);
        let name =
            unsafe { read_string_header(key.get() as *mut StringHeader) }.unwrap_or_default();
        let v = match &pairs {
            Some(_) => unsafe {
                f64::from_bits(
                    js_object_get_field_by_name(ptr(headers.get()), key.get() as *mut StringHeader)
                        .bits(),
                )
            },
            None => array_get(headers.get(), 2 * i + 1),
        };
        let v = scope.root_nanbox(v);
        validate(&name, v.get());
        if !existing {
            render_value(&name, v.get(), &mut direct);
        } else if pairs.is_some() || seen.insert(name.to_ascii_lowercase()) {
            set(owner.get(), &name, v.get());
        } else {
            append(owner.get(), &name, v.get());
        }
    }
    if existing {
        None
    } else {
        let native = response_state(owner.get())
            .map(|s| s.snapshot_headers())
            .unwrap_or_default();
        for (name, value) in native {
            if !direct.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                direct.push((name, value));
            }
        }
        Some(direct)
    }
}
fn render_value(name: &str, value: f64, out: &mut Vec<(String, String)>) {
    let scope = TransientRootScope::enter();
    let value = scope.root_nanbox(value);
    if is_array(value.get()) {
        let len = array_len(value.get());
        if name.eq_ignore_ascii_case("cookie") && len > 1 {
            let values = (0..len)
                .map(|i| {
                    let v = array_get(value.get(), i);
                    let jsv = JsValue::from_bits(v.to_bits());
                    if jsv.is_null() || jsv.is_undefined() {
                        String::new()
                    } else {
                        string(v)
                    }
                })
                .collect::<Vec<_>>();
            out.push((name.to_string(), values.join("; ")));
        } else {
            for i in 0..len {
                out.push((name.to_string(), string(array_get(value.get(), i))));
            }
        }
    } else {
        out.push((name.to_string(), string(value.get())));
    }
}

pub(crate) fn validate(name: &str, value: f64) {
    if !super::http_is_valid_token(name) {
        perry_ffi::throw_with_code(
            &format!("Header name must be a valid HTTP token [\"{name}\"]"),
            "ERR_INVALID_HTTP_TOKEN",
            perry_ffi::ErrorKind::TypeError,
        );
    }
    if value.to_bits() == TAG_UNDEFINED {
        perry_ffi::throw_with_code(
            &format!("Invalid value \"undefined\" for header \"{name}\""),
            "ERR_HTTP_INVALID_HEADER_VALUE",
            perry_ffi::ErrorKind::TypeError,
        );
    }
    let v = JsValue::from_bits(value.to_bits());
    if v.is_number() || v.is_bool() || v.is_null() {
        return;
    }
    let rendered = v.to_owned_string().unwrap_or_else(|| string(value));
    if rendered
        .chars()
        .any(|c| c != '\t' && (c < ' ' || c == '\u{7f}' || c > '\u{ff}'))
    {
        perry_ffi::throw_with_code(
            &format!("Invalid character in header content [\"{name}\"]"),
            "ERR_INVALID_CHAR",
            perry_ffi::ErrorKind::TypeError,
        );
    }
}
