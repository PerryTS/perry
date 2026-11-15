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
    let img = build(None);
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
    let img = build(None);
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
        program_tls: [0; 4],
    };
    let probe = std::ptr::addr_of_mut!(PROBE);
    // SAFETY: a test-owned static, written by this one test.
    unsafe { probe.write_volatile(crate::value::js_nanbox_string(s as i64).to_bits()) };
    let img = build(Some(&base));
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
