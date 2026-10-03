//! #11642: only the document's record array is sized from the remaining input.
use super::*;

struct Suppressed;
impl Suppressed {
    fn new() -> Self {
        crate::gc::gc_suppress();
        Self
    }
}
impl Drop for Suppressed {
    fn drop(&mut self) {
        crate::gc::gc_unsuppress();
    }
}

unsafe fn field(object: JSValue, name: &str) -> JSValue {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::object::js_object_get_field_by_name_f64(
        object.as_pointer::<crate::object::ObjectHeader>(),
        key,
    );
    JSValue::from_bits(value.to_bits())
}

/// A packument-shaped document: many entries, each with a tiny `[{...}]`.
fn nested_records(entries: usize) -> String {
    let body: Vec<String> = (0..entries)
        .map(|i| {
            format!(
                "\"v{i}\":{{\"dist\":{{\"signatures\":[{{\"sig\":\"s{i}\",\"keyid\":\"k\"}}]}}}}"
            )
        })
        .collect();
    format!("{{\"versions\":{{{}}}}}", body.join(","))
}

unsafe fn parse(text: &str, batched: bool) -> JSValue {
    let mut parser = if batched {
        DirectParser::new_batched(text.as_bytes())
    } else {
        DirectParser::new(text.as_bytes())
    };
    let value = parser.parse_value();
    assert!(parser.finish());
    value
}

#[test]
fn nested_record_arrays_are_not_sized_from_the_remaining_input() {
    let _suppressed = Suppressed::new();
    let text = nested_records(2000);
    for batched in [false, true] {
        unsafe {
            let versions = field(parse(&text, batched), "versions");
            for i in [0usize, 1, 999, 1999] {
                let entry = field(versions, &format!("v{i}"));
                let signatures = field(field(entry, "dist"), "signatures");
                let array = signatures.as_pointer::<ArrayHeader>();
                assert_eq!((*array).length, 1);
                // Sized from the remaining input this was ~text.len() / 96
                // (~1,000 slots at v0) — per entry, so quadratic per document.
                assert!(
                    (*array).capacity <= 16,
                    "v{i}: capacity {} (batched={batched})",
                    (*array).capacity
                );
            }
        }
    }
}

#[test]
fn the_document_record_array_keeps_its_values_at_the_root_and_under_a_key() {
    let _suppressed = Suppressed::new();
    let rows: Vec<String> = (0..3000)
        .map(|i| format!("{{\"id\":{i},\"tags\":[{{\"t\":{i}}}]}}"))
        .collect();
    let rows = rows.join(",");
    for text in [
        format!("[{rows}]"),
        format!("{{\"total\":3000,\"items\":[{rows}]}}"),
    ] {
        unsafe {
            let value = parse(&text, true);
            let array = if text.starts_with('[') {
                value
            } else {
                field(value, "items")
            };
            let array = array.as_pointer::<ArrayHeader>();
            assert_eq!((*array).length, 3000);
            // The estimate is an upper bound on waste, not a reservation kept
            // forever: a finished array uses at least a quarter of its slots.
            assert!((*array).capacity <= 64 || (*array).capacity / 4 <= (*array).length);
            let last = *crate::array::array_elements_ptr(array)
                .cast::<JSValue>()
                .add(2999);
            let tags = field(last, "tags").as_pointer::<ArrayHeader>();
            assert_eq!((*tags).length, 1);
            assert!(
                (*tags).capacity <= 16,
                "nested tags capacity {}",
                (*tags).capacity
            );
        }
    }
}
