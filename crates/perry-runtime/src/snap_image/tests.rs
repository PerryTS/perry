use super::format::*;
use super::format_read::*;
use super::*;

fn sample() -> Image {
    Image {
        cells: vec![Cell {
            obj_type: 1,
            flags: 5,
            reserved: 7,
            size: 16,
            bytes: (0..16).collect(),
            slots: vec![(
                8,
                Val::Cell {
                    hi: 0x7FFD,
                    bare: false,
                    index: 3,
                },
            )],
        }],
        words: vec![
            Word {
                region: WORD_DATA,
                off: 24,
                val: Val::Ext {
                    hi: 0,
                    bare: true,
                    region: REGION_EXE,
                    off: 99,
                },
            },
            Word {
                region: WORD_TLS,
                off: 8,
                val: Val::Opaque(0x55),
            },
            Word {
                region: WORD_TLS,
                off: 16,
                val: Val::Scalar(42),
            },
        ],
        stops: vec![Stop {
            cell: 0,
            obj_type: 1,
            kind: STOP_NO_FORM,
            off: 0,
            value: 0,
        }],
        pending: PENDING,
        claims_from: 5,
        claims: vec![b"a::B@1:2".to_vec(), b"anonymous:4096".to_vec()],
        registry: Vec::new(),
        mint: Vec::new(),
    }
}

#[test]
fn file_round_trips_every_value_kind() {
    let img = sample();
    assert_eq!(decode(&encode(&img)).unwrap(), img);
    let d = dump(&img);
    assert!(d.contains("cells=1 bytes=16 slots=1"), "{d}");
    assert!(
        d.contains("stop cell=0 type=1 kind=0") && d.contains("pending atom table"),
        "{d}"
    );
}

#[test]
fn decode_rejects_a_truncated_or_foreign_file() {
    let b = encode(&sample());
    assert!(decode(&b[..b.len() - 3]).is_err());
    assert!(decode(b"not an image").is_err());
}

fn string_cell(img: &Image, needle: &[u8]) -> Option<usize> {
    img.cells.iter().position(|c| {
        c.obj_type == crate::gc::GC_TYPE_STRING
            && c.bytes.windows(needle.len()).any(|w| w == needle)
    })
}

#[test]
fn array_cell_names_its_string_and_zeroes_the_slot() {
    let text = b"image-run probe string, long enough to stay a heap cell";
    let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let mut arr = crate::array::js_array_alloc(2);
    arr = crate::array::js_array_push_f64(arr, crate::value::js_nanbox_string(s as i64));
    let img = build(None, Vec::new(), false);
    let si = string_cell(&img, text).expect("string cell captured");
    let ai = img
        .cells
        .iter()
        .position(|c| {
            c.slots
                .iter()
                .any(|(_, v)| matches!(v, Val::Cell { index, .. } if *index as usize == si))
        })
        .expect("a cell names the string");
    let a = &img.cells[ai];
    assert_eq!(a.obj_type, crate::gc::GC_TYPE_ARRAY);
    let (off, v) = &a.slots[0];
    assert!(matches!(v, Val::Cell { bare: false, .. }));
    assert_eq!(
        &a.bytes[*off as usize..*off as usize + 8],
        &[0u8; 8],
        "slot word is zeroed"
    );
    assert_eq!(a.bytes.len(), a.size as usize);
    assert!(
        img.stops.iter().all(|s| s.cell as usize != ai),
        "{:?}",
        img.stops
    );
    assert_eq!(img.pending, PENDING);
    std::hint::black_box(arr);
}

#[test]
fn native_payload_types_have_no_image_form() {
    assert!(no_form(crate::gc::GC_TYPE_INTERP_FUNCTION));
    assert!(no_form(crate::gc::GC_TYPE_NATIVE_HANDLE));
    assert!(!no_form(crate::gc::GC_TYPE_ARRAY));
    assert!(!no_form(crate::gc::GC_TYPE_STRING));
}

#[test]
fn shaped_object_keeps_its_slots_inside_the_cell() {
    let o = crate::object::js_object_alloc(0, 0);
    let text = b"shaped object probe value, long enough to stay a heap cell";
    let v = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let key = crate::string::js_string_from_bytes(b"probeKey".as_ptr(), 8);
    crate::object::js_object_set_field_by_name(o, key, crate::value::js_nanbox_string(v as i64));
    let img = build(None, Vec::new(), false);
    let si = string_cell(&img, text).expect("value cell captured");
    let oi = img
        .cells
        .iter()
        .position(|c| {
            c.obj_type == crate::gc::GC_TYPE_OBJECT
                && c.slots
                    .iter()
                    .any(|(_, v)| matches!(v, Val::Cell { index, .. } if *index as usize == si))
        })
        .expect("the object names its value");
    let c = &img.cells[oi];
    assert!(
        c.slots
            .iter()
            .all(|(off, _)| (*off as usize) + 8 <= c.bytes.len()),
        "{:?}",
        c.slots
    );
    assert!(
        img.stops.iter().all(|s| s.cell as usize != oi),
        "{:?}",
        img.stops
    );
    std::hint::black_box(o);
}

#[used]
static mut PROBE: u64 = 0x1234_5678;

#[test]
fn a_changed_data_word_naming_a_cell_is_a_root_word() {
    let text = b"root word probe string, long enough to stay a heap cell!!";
    let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let lo = std::ptr::addr_of!(__data_start) as usize;
    let hi = std::ptr::addr_of!(_end) as usize;
    let base = Baseline {
        data: copy(lo, hi),
        tls: (0, Vec::new()),
        pre_data: vec![0u8; hi - lo],
        pre_tls: Vec::new(),
        classes: crate::object::class_image::snap_image_table_sizes(),
        claims_from: 0,
        claims: Vec::new(),
        shapes: crate::object::shapes::snap_image_shape_state(),
        token: crate::object::field_get_set::snap_image_next_token(),
    };
    let probe = std::ptr::addr_of_mut!(PROBE);
    // SAFETY: a test-owned static, written by this one test.
    unsafe { probe.write_volatile(crate::value::js_nanbox_string(s as i64).to_bits()) };
    let mut base = base;
    take_pre(&mut base);
    let img = build(Some(&base), Vec::new(), false);
    let want = (probe as usize - lo) as u64;
    let w = img
        .words
        .iter()
        .find(|w| w.region == WORD_DATA && w.off == want)
        .expect("probe word captured");
    let si = string_cell(&img, text).unwrap();
    assert!(
        matches!(w.val, Val::Cell { index, .. } if index as usize == si),
        "{:?}",
        w.val
    );
    // SAFETY: as above.
    unsafe { probe.write_volatile(0x1234_5678) };
}

/// The dump tool: `PERRY_SNAP_IMAGE_DUMP=<file> cargo test -- --ignored
/// print_image_file --nocapture` prints the summary of an image file.
#[test]
#[ignore]
fn print_image_file() {
    let Some(p) = std::env::var_os("PERRY_SNAP_IMAGE_DUMP") else {
        return;
    };
    let img = decode(&std::fs::read(p).unwrap()).unwrap();
    println!("\n{}", dump(&img));
}

fn probe_where() -> Where {
    Where {
        cells: Vec::new(),
        sections: [(0, 0); 3],
        exe: (0x40_0000, 0x40_1000),
        tls: (0, 0),
        maps: vec![(0x40_0000, 0x50_0000), (0x7000_0000_0000, 0x7000_0001_0000)],
        sym: (0x7000_0000_8000, 0x7000_0001_0000),
    }
}

#[test]
fn maps_text_is_parsed_and_merged() {
    let t = b"00400000-00452000 r-xp 00000000 00:00 0 /x\n00452000-00460000 rw-p 0 0:0 0\n7ffc00000000-7ffc00001000 rw-p 0 0:0 0 [stack]\n";
    assert_eq!(
        merged(parse_maps(t)),
        vec![(0x40_0000, 0x46_0000), (0x7ffc_0000_0000, 0x7ffc_0000_1000)]
    );
    assert!(read_maps().len() > 3, "this process has mappings");
}

#[test]
fn a_word_outside_every_mapping_is_a_scalar_and_a_tagged_symbol_word_is_sym() {
    let w = probe_where();
    // A flag-byte word (0x01010100) is a number, not an address.
    assert_eq!(w.val(0x0101_0100), Ok(Val::Scalar(0x0101_0100)));
    assert_eq!(
        w.val(crate::value::POINTER_TAG | 0x1234),
        Ok(Val::Scalar(crate::value::POINTER_TAG | 0x1234))
    );
    // An untagged and a tagged address inside the symbol space.
    let a = 0x7000_0000_9000u64;
    assert_eq!(w.val(a), Ok(Val::Sym(a)));
    assert_eq!(
        w.val(crate::value::POINTER_TAG | a),
        Ok(Val::Sym(crate::value::POINTER_TAG | a))
    );
    // Inside a mapping but neither a cell nor the symbol space: opaque.
    let m = 0x7000_0000_2000u64;
    assert_eq!(
        w.val(crate::value::STRING_TAG | m),
        Ok(Val::Opaque(crate::value::STRING_TAG | m))
    );
}

// The probes read the Linux x86-64 thread pointer and static TLS block.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[thread_local]
static PROBE_TLS: std::cell::Cell<u64> = std::cell::Cell::new(0x77);

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn probe_baseline() -> (Baseline, usize) {
    let lo = std::ptr::addr_of!(__data_start) as usize;
    let hi = std::ptr::addr_of!(_end) as usize;
    let mut fs = 0usize;
    // SAFETY: ARCH_GET_FS stores the thread pointer.
    unsafe { libc::syscall(libc::SYS_arch_prctl, 0x1003, &mut fs as *mut usize) };
    let (memsz, align) = executable_tls_size();
    let (tls_lo, tls_len) = tls_window(fs, memsz, align);
    let data = copy(lo, hi);
    let tls = copy(tls_lo, tls_lo + tls_len);
    let (pre_data, pre_tls) = (vec![0u8; data.1.len()], vec![0u8; tls.1.len()]);
    let b = Baseline {
        data,
        tls,
        pre_data,
        pre_tls,
        classes: crate::object::class_image::snap_image_table_sizes(),
        claims_from: 0,
        claims: Vec::new(),
        shapes: crate::object::shapes::snap_image_shape_state(),
        token: crate::object::field_get_set::snap_image_next_token(),
    };
    (b, tls_lo)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn a_runtime_thread_local_word_written_after_the_baseline_is_a_root_word() {
    let (base, tls_lo) = probe_baseline();
    PROBE_TLS.set(0x78);
    let mut base = base;
    take_pre(&mut base);
    let img = build(Some(&base), Vec::new(), false);
    let want = (std::ptr::addr_of!(PROBE_TLS) as usize - tls_lo) as u64;
    PROBE_TLS.set(0x77);
    let w = img
        .words
        .iter()
        .find(|w| w.region == WORD_TLS && w.off == want)
        .expect("the thread-local word is captured");
    assert_eq!(w.val, Val::Scalar(0x78));
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn a_word_the_capture_writes_after_the_first_copy_is_not_a_root_word() {
    let probe = std::ptr::addr_of_mut!(PROBE);
    let lo = std::ptr::addr_of!(__data_start) as usize;
    let (mut base, _) = probe_baseline();
    let want = (probe as usize - lo) as u64;
    // SAFETY: a test-owned static, written by the tests that use it.
    unsafe { probe.write_volatile(0x9999) };
    take_pre(&mut base);
    // Written by the capture's own code after the first copy.
    unsafe { probe.write_volatile(0x6666) };
    let img = build(Some(&base), Vec::new(), false);
    let w = img
        .words
        .iter()
        .find(|w| w.off == want && w.region & WORD_TLS == 0)
        .expect("the program's word is captured");
    assert_eq!(w.val, Val::Scalar(0x9999), "the value of the first copy");
    // A word the program left unchanged is not a root word at all.
    let (mut base2, _) = probe_baseline();
    take_pre(&mut base2);
    unsafe { probe.write_volatile(0x7777) };
    let img2 = build(Some(&base2), Vec::new(), false);
    assert!(
        !img2
            .words
            .iter()
            .any(|w| w.off == want && w.region & WORD_TLS == 0),
        "a write after the first copy is invisible"
    );
    unsafe { probe.write_volatile(0x1234_5678) };
}

static mut LIVE_ROOT: u64 = 0;

#[test]
fn the_mark_keeps_what_the_roots_reach_and_nothing_else() {
    let text = |t: &[u8]| crate::string::js_string_from_bytes(t.as_ptr(), t.len() as u32);
    let held = text(b"mark probe: held by an array that a root names, long enough to be a cell");
    let dead = text(b"mark probe: reachable from nothing at all, long enough to be a cell too");
    let mut arr = crate::array::js_array_alloc(2);
    arr = crate::array::js_array_push_f64(arr, crate::value::js_nanbox_string(held as i64));
    let header = |p: usize| p - crate::gc::GC_HEADER_SIZE;
    let all = cell_headers();
    let live = mark_live(all.clone(), |v| {
        v(crate::value::js_nanbox_pointer(arr as i64).to_bits())
    });
    assert!(live.contains(&header(arr as usize)), "the root's own cell");
    assert!(
        live.contains(&header(held as usize)),
        "reached through a traced slot"
    );
    assert!(!live.contains(&header(dead as usize)), "garbage stays out");
    assert!(live.len() < all.len());
}

#[test]
fn every_root_source_of_the_full_trace_reaches_the_mark() {
    let s = crate::string::js_string_from_bytes(
        b"global root probe, a heap string cell for the mark".as_ptr(),
        50,
    );
    // SAFETY: a test-owned static word registered as a global root.
    unsafe {
        let slot = std::ptr::addr_of_mut!(LIVE_ROOT);
        // GC_STORE_AUDIT(ROOT): a static word registered as a global root next.
        slot.write(crate::value::js_nanbox_string(s as i64).to_bits());
        crate::gc::js_gc_register_global_root(slot as i64);
    }
    let live = mark_live(cell_headers(), crate::gc::image_visit_roots);
    assert!(
        live.contains(&(s as usize - crate::gc::GC_HEADER_SIZE)),
        "a cell held by a registered global root is live"
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn growth_of_the_class_tables_stops_the_image() {
    let (mut base, _) = probe_baseline();
    let none = build(Some(&base), Vec::new(), false);
    assert!(none.stops.iter().all(|s| s.kind != STOP_CLASS_TABLES));
    base.classes[4] = base.classes[4].wrapping_sub(3);
    let grew = build(Some(&base), Vec::new(), false);
    let s: Vec<_> = grew
        .stops
        .iter()
        .filter(|s| s.kind == STOP_CLASS_TABLES)
        .collect();
    assert_eq!(s.len(), 1, "one stop per growing table");
    assert_eq!(
        (s[0].cell, s[0].value),
        (4, 3),
        "the stop names the table and its rows"
    );
}

#[test]
fn the_claim_record_lists_late_claims_in_order_and_replays_to_the_same_indices() {
    let from = crate::tls_hot::js_tls_hot_claimed_slots();
    let (n1, n2) = (b"snap_test::LATE_A@1:1", b"snap_test::LATE_B@2:2");
    // SAFETY: names of readable bytes; an anonymous claim has `len` 0.
    let (i1, i2, i3) = unsafe {
        (
            crate::tls_hot::js_tls_hot_claim_slot(n1.as_ptr(), n1.len(), 0),
            crate::tls_hot::js_tls_hot_claim_slot(n2.as_ptr(), n2.len(), 0),
            crate::tls_hot::js_tls_hot_claim_slot(std::ptr::null(), 0, 0x1000),
        )
    };
    assert_eq!((i1, i2, i3), (from, from + 1, from + 2));
    let now = crate::tls_hot::claims_in_order(from);
    let rec: Vec<Vec<u8>> = now.iter().map(|e| e.0.clone()).collect();
    assert_eq!(rec[0], n1);
    assert_eq!(rec[1], n2);
    assert_eq!(rec[2], b"anonymous:4096");
    assert_eq!(parse_claims_blob(&claims_blob(&rec)), Some(rec.clone()));
    assert!(claims_replay_matches(from, &rec, &now));
    // A fresh process that claimed one name first hands out shifted indices.
    let shifted: Vec<(Vec<u8>, u32)> = now.iter().map(|e| (e.0.clone(), e.1 + 1)).collect();
    assert!(!claims_replay_matches(from, &rec, &shifted));
    let mut swapped = now.clone();
    swapped.swap(0, 1);
    assert!(!claims_replay_matches(from, &rec, &swapped));
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn the_capture_takes_the_claims_before_its_own_code_runs() {
    let (mut base, _) = probe_baseline();
    let n = b"snap_test::CAPTURE_LATE@9:9";
    // SAFETY: a readable name.
    let i = unsafe { crate::tls_hot::js_tls_hot_claim_slot(n.as_ptr(), n.len(), 0) };
    base.claims_from = i;
    take_pre(&mut base);
    let img = build(Some(&base), Vec::new(), false);
    assert_eq!(img.claims_from, i);
    assert_eq!(img.claims.first().map(|k| k.as_slice()), Some(&n[..]));
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn a_claim_key_over_four_kib_stops_the_image() {
    let (mut base, _) = probe_baseline();
    let long = vec![b'k'; 5000];
    // SAFETY: a readable name of 5000 bytes.
    let i = unsafe { crate::tls_hot::js_tls_hot_claim_slot(long.as_ptr(), long.len(), 0) };
    base.claims_from = i;
    take_pre(&mut base);
    let img = build(Some(&base), Vec::new(), false);
    let s = img.stops.iter().find(|s| s.kind == STOP_CLAIM_KEY);
    assert_eq!(s.map(|s| s.cell), Some(i));
}

fn forms_of(img: &Image, t: u8) -> Vec<&Cell> {
    img.cells.iter().filter(|c| c.obj_type == t).collect()
}

#[test]
fn a_map_with_string_and_number_keys_has_a_self_contained_form() {
    let key = crate::string::js_string_from_bytes(b"map-form-key".as_ptr(), 12);
    let mut m = crate::map::js_map_alloc(4);
    m = crate::map::js_map_set(m, crate::value::js_nanbox_string(key as i64), 1.5);
    m = crate::map::js_map_set(m, 7.0, 2.5);
    let img = build(None, Vec::new(), false);
    let maps = forms_of(&img, crate::gc::GC_TYPE_MAP);
    let stores = forms_of(&img, crate::gc::GC_TYPE_MAP_STORE);
    assert!(!maps.is_empty() && !stores.is_empty());
    let has = |c: &Cell, k: u8| c.slots.iter().any(|s| s.1 == Val::Derived(k));
    assert!(maps.iter().all(|c| has(c, DERIVED_MAP_ENTRIES)));
    assert!(stores
        .iter()
        .all(|c| has(c, DERIVED_STORE_ENTRIES) && has(c, DERIVED_STORE_INDEX)));
    // The derived words are zero in the bytes: nothing of this process's heap remains.
    assert!(maps.iter().all(|c| c.bytes[16..24] == [0; 8]));
    let bad = |i: &Image| {
        i.stops
            .iter()
            .filter(|s| {
                s.obj_type == crate::gc::GC_TYPE_MAP || s.obj_type == crate::gc::GC_TYPE_MAP_STORE
            })
            .map(|s| s.kind)
            .collect::<Vec<_>>()
    };
    assert!(bad(&img).is_empty(), "{:?}", bad(&img));
    // An object key has no form: the image stops with a named reason.
    let arr = crate::array::js_array_alloc(1);
    m = crate::map::js_map_set(m, crate::value::js_nanbox_pointer(arr as i64), 3.5);
    let img2 = build(None, Vec::new(), false);
    assert!(bad(&img2).contains(&STOP_MAP_KEYS));
    let _ = m;
}

#[test]
fn an_untraced_raw_address_stops_the_image_and_a_zero_word_does_not() {
    let mut m = crate::map::js_map_alloc(2);
    m = crate::map::js_map_set(m, 1.0, 2.0);
    let count = |img: &Image| {
        img.stops
            .iter()
            .filter(|s| s.kind == STOP_RAW_HEAP_WORD && s.obj_type == crate::gc::GC_TYPE_MAP_STORE)
            .count()
    };
    let before = count(&build(None, Vec::new(), false));
    // The store's receiver word is untraced and zero: a heap address there has no form.
    let word = unsafe { (*m).store.cast::<u64>() };
    let leaked = Box::into_raw(Box::new(5u64)) as u64;
    unsafe { word.write(leaked) };
    let after = count(&build(None, Vec::new(), false));
    unsafe { word.write(0) };
    assert_eq!((before, after), (0, 1));
}

#[test]
fn completeness_names_each_missing_form() {
    use crate::object::shapes::SnapShapeDelta;
    let none = SnapShapeDelta::default();
    let (p, s) = completeness(none, 1, 2);
    assert_eq!((p, s.len()), (PENDING_ATOMS, 0));
    let minted = SnapShapeDelta {
        minted: [1, 2, 0, 3],
        changed: 4,
        retired: 5,
        ..none
    };
    let (p, s) = completeness(minted, 1, 2);
    assert_eq!(p, PENDING_ATOMS | PENDING_SHAPES);
    assert_eq!(
        s.iter()
            .map(|s| (s.kind, s.cell, s.off, s.value))
            .collect::<Vec<_>>(),
        vec![(STOP_SHAPES_MINTED, 5, 4, 1 | 2 << 16 | 3 << 48)]
    );
    // A changed pre-existing record alone, a retired one alone and a counter alone each stop.
    for d in [
        SnapShapeDelta { changed: 1, ..none },
        SnapShapeDelta { retired: 1, ..none },
    ] {
        let (p, s) = completeness(d, 1, 2);
        assert_eq!((p & PENDING_SHAPES, s.len()), (PENDING_SHAPES, 1), "{d:?}");
    }
    // A minted record alone sets the shape form pending; its stop is the mint record's.
    for d in [
        SnapShapeDelta {
            minted: [1, 0, 0, 0],
            ..none
        },
        SnapShapeDelta {
            minted: [0, 0, 0, 1],
            ..none
        },
    ] {
        let (p, s) = completeness(d, 1, 2);
        assert_eq!((p & PENDING_SHAPES, s.len()), (PENDING_SHAPES, 0), "{d:?}");
    }
    let (p, s) = completeness(
        SnapShapeDelta {
            drawn: 7,
            generations: 2,
            ..none
        },
        1,
        2,
    );
    assert_eq!(p, PENDING_ATOMS | PENDING_SHAPES);
    assert_eq!(
        s.iter().map(|s| (s.kind, s.value)).collect::<Vec<_>>(),
        vec![(STOP_SHAPE_COUNTERS, 7 | 2 << 32)]
    );
    let (p, s) = completeness(none, 1, 3);
    assert_eq!(p, PENDING_ATOMS | PENDING_TOKENS);
    assert_eq!(
        s.iter().map(|s| s.kind).collect::<Vec<_>>(),
        vec![STOP_TOKENS]
    );
    assert_eq!(
        (
            completeness(none, 3, 3).1.len(),
            completeness(none, 3, 4).1.len()
        ),
        (0, 1)
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
extern "C" fn registry_probe_init() {}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn module_inits_registered_in_the_path_registry_become_a_replay_record() {
    let key = b"\0module-init:img-probe";
    unsafe {
        crate::module_require::js_register_path_init(
            key.as_ptr(),
            key.len() as i64,
            registry_probe_init as usize as i64,
        );
    }
    let (b, _) = probe_baseline();
    let img = build(Some(&b), Vec::new(), false);
    let r = img
        .registry
        .iter()
        .find(|r| r.key == key)
        .expect("the entry is in the record");
    let exe = std::ptr::addr_of!(__executable_start) as usize;
    assert_eq!(r.init_off, (registry_probe_init as usize - exe) as u64);
    assert_eq!((r.status, r.exports.clone()), (0, None));
    let blob = registry_blob(&img.registry);
    let (back, used) = parse_registry_blob(&blob).expect("parses");
    assert_eq!((back, used), (img.registry.clone(), blob.len()));
}

#[test]
fn the_file_carries_derived_words_and_the_registry() {
    let mut img = sample();
    if let Some(c) = img.cells.get_mut(0) {
        c.slots.push((12, Val::Derived(DERIVED_STORE_INDEX)));
    }
    img.registry = vec![RegRec {
        key: b"k".to_vec(),
        init_off: 9,
        status: 2,
        exports: Some(Val::Cell {
            hi: 0x7FFD,
            bare: false,
            index: 0,
        }),
        factory: None,
    }];
    assert_eq!(decode(&encode(&img)).as_ref(), Ok(&img));
}

#[test]
fn the_tls_window_is_the_executables_block_without_the_gap_above_it() {
    let (lo, len) = super::tls_window(0x10_0000, 0x2ed70, 0x40);
    assert_eq!(lo, 0x10_0000 - 0x2ed80);
    assert_eq!(len, 0x2ed70, "the padding up to fs holds libc blocks");
    assert_eq!(
        super::tls_window(0x10_0000, 0x100, 8),
        (0x10_0000 - 0x100, 0x100)
    );
}

fn full_mint_rec(n: u32) -> MintRec {
    MintRec {
        id: 0x0030_0000 + n,
        word: 0xA000_0000 + n,
        keys: 0x1234_5678_9abc + u64::from(n),
        logical: 3,
        live: 4,
        holes: 5,
        generation: 0x1_0000_0002,
        kind: 6,
        summary: 7,
        integrity: 8,
        proto_id: 0x55,
        proto_cell: 0x66,
        rep: 0x77,
        brands: vec![11, 12],
        constfn: vec![(1, 0x21), (2, 0x22)],
        handles: [1, 2, 3, 4, 5, 6],
        family_desc: 13,
        edges: vec![9, 10, 11],
    }
}

#[test]
fn mint_record_round_trips_every_field() {
    let recs = vec![full_mint_rec(1), MintRec::default(), full_mint_rec(2)];
    let blob = mint_blob(&recs);
    assert_eq!(parse_mint_blob(&blob), Some((recs.clone(), blob.len())));
    assert_eq!(parse_mint_blob(&blob[..blob.len() - 1]), None);
    let mut img = sample();
    img.mint = recs;
    assert_eq!(decode(&encode(&img)).expect("parses"), img);
}

#[test]
fn mint_stop_reports_the_size_of_a_verified_record_and_names_a_failing_one() {
    let before = crate::object::shapes::snap_image_shape_state();
    let o = crate::object::js_object_alloc(0, 0);
    crate::object::js_object_set_field_by_name(
        o,
        crate::string::js_string_from_bytes(b"snapimgmintstop".as_ptr(), 15u32),
        1.0,
    );
    let delta = crate::object::shapes::snap_image_shape_delta(
        &before,
        &crate::object::shapes::snap_image_shape_state(),
    );
    let recs = crate::object::shapes::snap_image_mint_collect(&before);
    assert!(!recs.is_empty());
    let s = mint_stop(&delta, &recs).expect("a stop");
    assert_eq!(
        (s.kind, s.value & 0xffff_ffff, s.value >> 32),
        (
            STOP_MINT_RECORD,
            recs.len() as u64,
            mint_blob(&recs).len() as u64
        )
    );
    assert_eq!(s.cell & 0xffff, delta.minted[0]);
    assert_eq!(s.off & 0xffff, delta.minted[1]);
    assert_eq!(mint_stop(&Default::default(), &[]).map(|s| s.kind), None);
    // A record whose rebuilt form differs stops with its index.
    let mut bad = recs.clone();
    bad[0].generation += 1;
    let s = mint_stop(&delta, &bad).expect("a stop");
    assert_eq!(
        (s.kind, s.cell, u64::from(bad[0].id)),
        (STOP_MINT_MISMATCH, 0, s.value)
    );
}
