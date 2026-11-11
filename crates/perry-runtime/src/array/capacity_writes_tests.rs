//! Every write of a `capacity` field in perry-runtime, classified against
//! wpfacts' length floor (`perry-wpfacts/src/solve/arr_len.rs`, Invariant
//! C): while a program Array's length interval is not ⊤, its capacity is at
//! least the interval's `lo`. A birth allocates at least its length, growth
//! only raises capacity, and every write that lowers it runs on an array
//! whose length is ⊤ (a shift) or that is no program Array token. Generated
//! code writes one header itself, through `perry_abi::array_birth_header`,
//! which asserts `capacity >= length` (`perry-codegen`
//! `expr/array_literal.rs`).
//!
//! A new write fails this test until it is added below with its class.

use std::collections::BTreeMap;
use std::path::Path;

/// (file under `src/`, the trimmed source line, how many such lines, why it
/// keeps Invariant C).
const CLASSIFIED: &[(&str, &str, usize, &str)] = &[
    // ---- ArrayHeader: allocation (capacity >= the length it is born with).
    (
        "array/alloc.rs",
        "(*ptr).capacity = actual_capacity;",
        4,
        "allocation",
    ),
    (
        "array/alloc.rs",
        "(*ptr).capacity = capacity;",
        4,
        "allocation",
    ),
    // The fill birth: `capacity >= length` (`perry_abi::fill_birth_capacity`).
    (
        "array/fill_birth.rs",
        "(*arr).capacity = capacity;",
        1,
        "allocation",
    ),
    // ---- ArrayHeader: growth raises capacity.
    (
        "array/push_pop/growth.rs",
        "(*arr).capacity = new_capacity;",
        1,
        "in-place growth",
    ),
    (
        "array/push_pop.rs",
        "(*new_ptr).capacity = new_capacity;",
        1,
        "growth into a new allocation",
    ),
    // ---- ArrayHeader: shifts. A shift makes the length ⊤ in wpfacts.
    (
        "array/storage.rs",
        "(*arr).capacity += (front - reserve) as u32;",
        1,
        "shift_dense: empty-queue reset",
    ),
    (
        "array/storage.rs",
        "(*arr).capacity -= 1;",
        1,
        "shift_dense",
    ),
    // ---- ArrayHeader: never a program Array token.
    (
        "array/named_props.rs",
        "(*arr).capacity -= 1;",
        1,
        "ensure_key_attrs_slot: owned key lists only",
    ),
    (
        "object/builtin_proto.rs",
        "(*arr).capacity = 0;",
        1,
        "%Array.prototype% at seal",
    ),
    (
        "json/construction_array.rs",
        "(*ptr).capacity = capacity;",
        1,
        "JSON batch array (not a plain birth)",
    ),
    (
        "json_tape.rs",
        "(*hdr).capacity = cached_length;",
        1,
        "lazy JSON array header",
    ),
    (
        "object/class_registry/construct/site.rs",
        "(*array).capacity = site[0].load(Ordering::Relaxed) as u32;",
        1,
        "test-only",
    ),
    (
        "object/class_registry/construct/site.rs",
        "(*array).capacity = capacity;",
        1,
        "test-only (restore)",
    ),
    // ---- Other headers (Map, Set, String, Buffer, the TLS pool).
    ("map.rs", "(*ptr).capacity = cap;", 1, "MapHeader"),
    ("map.rs", "(*map).capacity = new_capacity;", 1, "MapHeader"),
    ("map.rs", "(*previous).capacity = 0;", 1, "MapHeader"),
    ("map/store.rs", "store.capacity = 0;", 1, "Map store"),
    ("map/store.rs", "(*map).capacity = 0;", 1, "MapHeader"),
    ("set.rs", "(*set).capacity = 0;", 1, "SetHeader"),
    ("set.rs", "(*set).capacity = new_capacity;", 1, "SetHeader"),
    ("set.rs", "(*previous).capacity = 0;", 1, "SetHeader"),
    ("set.rs", "(*ptr).capacity = cap;", 1, "SetHeader"),
    ("set/store.rs", "store.capacity = 0;", 1, "Set store"),
    (
        "string/mod.rs",
        "(*ptr).capacity = capacity;",
        1,
        "StringHeader",
    ),
    (
        "tls_os_pool.rs",
        "(*pool).capacity = capacity;",
        1,
        "TLS pool",
    ),
    (
        "buffer/store.rs",
        "(*cell).capacity = 0;",
        1,
        "BufferHeader",
    ),
    (
        "buffer/store.rs",
        "(*cell).capacity = offset;",
        1,
        "BufferHeader",
    ),
    (
        "buffer/store.rs",
        "(*(addr as *mut BufferHeader)).capacity = cap;",
        1,
        "BufferHeader",
    ),
    (
        "buffer/store/layout.rs",
        "(*ptr).capacity = capacity;",
        2,
        "BufferHeader",
    ),
    (
        "buffer/store/layout.rs",
        "(*ptr).header.capacity = length;",
        1,
        "BufferHeader",
    ),
    (
        "buffer/store/layout.rs",
        "(*buffer).header.capacity = length;",
        1,
        "BufferHeader",
    ),
];

/// Does `code` (a line without its `//` comment) assign a `capacity` field?
fn writes_capacity(code: &str) -> bool {
    let mut rest = code;
    while let Some(at) = rest.find(".capacity") {
        let after = &rest[at + ".capacity".len()..];
        let ident = after
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let op = after.trim_start();
        let assigns = ["+=", "-=", "*=", "|=", "&="]
            .iter()
            .any(|o| op.starts_with(o))
            || (op.starts_with('=') && !op.starts_with("==") && !op.starts_with("=>"));
        if !ident && assigns {
            return true;
        }
        rest = after;
    }
    false
}

fn is_test_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.ends_with("_tests.rs")
        || name == "tests.rs"
        || rel.starts_with("tests/")
        || rel.contains("/tests/")
}

fn scan(dir: &Path, root: &Path, out: &mut BTreeMap<(String, String), usize>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scan(&path, root, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if is_test_file(&rel) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for line in text.lines() {
            let code = line.split("//").next().unwrap_or("");
            if writes_capacity(code) {
                *out.entry((rel.clone(), line.trim().to_string()))
                    .or_default() += 1;
            }
        }
    }
}

#[test]
fn every_capacity_write_is_classified_for_the_length_floor() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = BTreeMap::new();
    scan(&root, &root, &mut found);
    let mut expected = BTreeMap::new();
    for &(file, line, count, class) in CLASSIFIED {
        assert!(!class.is_empty());
        let prior = expected.insert((file.to_string(), line.to_string()), count);
        assert!(prior.is_none(), "duplicate row {file}: {line}");
    }
    let unclassified: Vec<_> = found
        .iter()
        .filter(|(k, n)| expected.get(*k) != Some(*n))
        .collect();
    let stale: Vec<_> = expected
        .iter()
        .filter(|(k, n)| found.get(*k) != Some(*n))
        .collect();
    assert!(
        unclassified.is_empty() && stale.is_empty(),
        "capacity writes changed: classify each new write against wpfacts' length floor \
         (solve/arr_len.rs) in CLASSIFIED.\nunclassified or recounted: {unclassified:#?}\n\
         stale rows: {stale:#?}"
    );
}

/// Only the two shift-side writes and the owned key-list reserve lower an
/// ArrayHeader's capacity.
#[test]
fn array_capacity_is_lowered_only_by_shifts_and_key_lists() {
    let lowering: Vec<_> = CLASSIFIED
        .iter()
        .filter(|(file, line, _, _)| file.starts_with("array/") && line.contains("-="))
        .map(|(file, _, _, _)| *file)
        .collect();
    assert_eq!(lowering, ["array/storage.rs", "array/named_props.rs"]);
}

#[test]
fn the_scan_sees_every_assignment_form() {
    assert!(writes_capacity("(*a).capacity = 1;"));
    assert!(writes_capacity("(*a).capacity -= 1;"));
    assert!(writes_capacity("x.header.capacity += n;"));
    assert!(!writes_capacity("if (*a).capacity == 0 {"));
    assert!(!writes_capacity("&& (*arr).length <= (*arr).capacity =>"));
    assert!(!writes_capacity("let c = (*a).capacity;"));
    assert!(!writes_capacity("(*a).capacity_hint = 3;"));
}

/// A negative key on a full array is a named property in the separate
/// bag: the elements keep their address, and the length and capacity do
/// not move (wpfacts: a negative index store leaves the length interval
/// alone).
#[test]
fn a_negative_key_on_a_full_array_keeps_its_element_storage() {
    let _global = crate::gc::global_side_table_test_lock();
    crate::gc::initialize_runtime_fixture();
    unsafe {
        let arr = super::js_array_alloc_literal(3);
        super::js_array_set_f64(arr, 0, 1.0);
        super::js_array_set_f64(arr, 1, 2.0);
        super::js_array_set_f64(arr, 2, 3.0);
        let elements = super::array_elements_ptr(arr);
        let (length, capacity) = ((*arr).length, (*arr).capacity);
        let head = super::js_array_set_index_or_string(arr, -1.0, 9.0);
        assert_eq!(head, arr);
        assert_eq!(super::array_elements_ptr(head), elements);
        assert_eq!(((*head).length, (*head).capacity), (length, capacity));
        assert_eq!(super::js_array_get_f64(head, 2), 3.0);
    }
}
