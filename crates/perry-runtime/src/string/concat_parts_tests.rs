use super::*;
use crate::value::{JSValue, STRING_TAG};

fn inline(text: &[u8]) -> f64 {
    f64::from_bits(JSValue::short_string_unchecked(text).bits())
}

fn heap(text: &str) -> f64 {
    f64::from_bits(STRING_TAG | js_string_from_str(text) as u64)
}

fn bytes(s: *const StringHeader) -> Vec<u8> {
    unsafe { std::slice::from_raw_parts(string_data(s), (*s).byte_len as usize).to_vec() }
}

fn value(v: f64) -> ConcatPartSlot {
    ConcatPartSlot {
        word: v.to_bits(),
        meta: CONCAT_PART_VALUE,
    }
}

/// A literal operand over `text`, which must outlive the call.
fn literal(text: &'static str) -> ConcatPartSlot {
    ConcatPartSlot {
        word: text.as_ptr() as u64,
        meta: text.len() as u64 | ((text.encode_utf16().count() as u64) << 32),
    }
}

fn parts_call(parts: &[ConcatPartSlot]) -> *mut StringHeader {
    js_string_concat_parts(parts.as_ptr(), parts.len() as i32)
}

fn chain_call(values: &[f64]) -> *mut StringHeader {
    super::super::concat::js_string_concat_chain(values.as_ptr(), values.len() as i32)
}

fn same_string(a: *const StringHeader, b: *const StringHeader) {
    unsafe {
        assert_eq!(bytes(a), bytes(b));
        assert_eq!((*a).utf16_len, (*b).utf16_len);
        assert_eq!(
            (*a).flags & STRING_FLAG_HAS_LONE_SURROGATES,
            (*b).flags & STRING_FLAG_HAS_LONE_SURROGATES
        );
        assert_eq!((*a).refcount, 0);
        assert_eq!((*a).capacity, (*a).byte_len);
    }
}

#[test]
fn parts_agree_with_the_chain_on_every_value_kind() {
    let numbers = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        9.0,
        10.0,
        99.0,
        100.0,
        999_999_999.0,
        1_000_000_000.0,
        2_147_483_647.0,
        -2_147_483_648.0,
        4_294_967_295.0,
        4_294_967_296.0,
        -4_294_967_295.0,
        1e15,
        1e21,
        1.5,
        -0.25,
        1e-7,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
    ];
    for number in numbers {
        let literal_chain = chain_call(&[heap("item-"), number, heap(";")]);
        let literal_parts = parts_call(&[literal("item-"), value(number), literal(";")]);
        same_string(literal_parts, literal_chain);
    }
    // Heap, inline and empty operands around a number.
    let operands: [f64; 6] = [
        heap("heap-string"),
        inline(b"ab"),
        inline(b""),
        heap(""),
        heap("é€😀"),
        inline("é".as_bytes()),
    ];
    for operand in operands {
        let a = parts_call(&[literal("x"), value(operand), value(42.0), literal("y")]);
        let b = chain_call(&[heap("x"), operand, 42.0, heap("y")]);
        same_string(a, b);
    }
}

#[test]
fn other_value_kinds_take_the_general_arm_with_chain_results() {
    let others = [
        f64::from_bits(0x7FFC_0000_0000_0001),
        f64::from_bits(0x7FFC_0000_0000_0002),
        f64::from_bits(0x7FFC_0000_0000_0004),
        f64::from_bits(0x7FFC_0000_0000_0003),
        f64::from_bits(JSValue::int32(7).bits()),
        f64::from_bits(JSValue::int32(-7).bits()),
    ];
    for other in others {
        let a = parts_call(&[literal("<"), value(other), literal(">")]);
        let b = chain_call(&[heap("<"), other, heap(">")]);
        same_string(a, b);
    }
}

#[test]
fn literal_and_value_boundaries_around_surrogates() {
    // A lone high surrogate in a heap operand followed by a lone low one joins
    // into the astral scalar, exactly as the chain's canonicalization does.
    let high = js_string_from_wtf8_bytes([0xED, 0xA0, 0xBD].as_ptr(), 3);
    let low = js_string_from_wtf8_bytes([0xED, 0xB8, 0x80].as_ptr(), 3);
    let high_box = f64::from_bits(STRING_TAG | high as u64);
    let low_box = f64::from_bits(STRING_TAG | low as u64);
    let joined = parts_call(&[value(high_box), value(low_box)]);
    assert_eq!(bytes(joined), "😀".as_bytes());
    assert_eq!(unsafe { (*joined).utf16_len }, 2);
    assert_eq!(
        unsafe { (*joined).flags } & STRING_FLAG_HAS_LONE_SURROGATES,
        0
    );
    // A lone surrogate next to literals stays lone, with its flag.
    let lone = parts_call(&[literal("a"), value(high_box), literal("b")]);
    assert_eq!(bytes(lone), vec![b'a', 0xED, 0xA0, 0xBD, b'b']);
    assert_ne!(
        unsafe { (*lone).flags } & STRING_FLAG_HAS_LONE_SURROGATES,
        0
    );
    let reference = chain_call(&[heap("a"), high_box, heap("b")]);
    same_string(lone, reference);
    // Multi-byte literals carry their own UTF-16 length.
    let wide = parts_call(&[literal("é€😀"), value(1.0), literal("😀")]);
    assert_eq!(bytes(wide), "é€😀1😀".as_bytes());
    assert_eq!(unsafe { (*wide).utf16_len }, 1 + 1 + 2 + 1 + 2);
}

#[test]
fn empty_literals_and_32_parts() {
    let empty = parts_call(&[literal(""), literal("")]);
    assert_eq!(bytes(empty), b"");
    let mut slots = vec![literal("ab"); 32];
    slots[5] = value(12345.0);
    slots[31] = value(heap("end"));
    let mut expected = String::new();
    for i in 0..32 {
        expected.push_str(match i {
            5 => "12345",
            31 => "end",
            _ => "ab",
        });
    }
    assert_eq!(bytes(parts_call(&slots)), expected.as_bytes());
    // Counts that select each scratch size.
    for count in [2, 4, 5, 8, 9, 31] {
        let slots = vec![literal("z"); count];
        assert_eq!(bytes(parts_call(&slots)), vec![b'z'; count]);
    }
}

#[test]
fn very_long_results_cross_the_copy_thresholds() {
    for len in [15usize, 16, 17, 31, 32, 33, 4096, 100_000] {
        let text = "q".repeat(len);
        let long = heap(&text);
        let a = parts_call(&[literal("item-"), value(long), literal(";"), value(7.0)]);
        assert_eq!(unsafe { (*a).byte_len } as usize, len + 5 + 1 + 1);
        let out = bytes(a);
        assert!(out.starts_with(b"item-q") && out.ends_with(b"q;7"));
    }
}

#[test]
fn a_refused_no_collect_allocation_takes_the_rooted_copy() {
    // Heap operands must survive a collecting allocation: drive the cold arm
    // directly with a payload pointer taken before the collection.
    let scope = crate::gc::RuntimeHandleScope::new();
    let kept = scope.root_string_ptr(js_string_from_str("kept-across-collection"));
    let boxed = || f64::from_bits(STRING_TAG | kept.get_raw_const_ptr::<StringHeader>() as u64);
    let slots = [literal("<"), value(boxed()), value(5.0), literal(">")];
    let mut piece_ptrs: [*const u8; 4] = [std::ptr::null(); 4];
    let mut piece_lens: [u32; 4] = [0; 4];
    let text = "kept-across-collection";
    piece_ptrs[0] = b"<".as_ptr();
    piece_lens[0] = 1;
    piece_ptrs[1] = string_data(kept.get_raw_const_ptr::<StringHeader>());
    piece_lens[1] = text.len() as u32;
    let five = b"5";
    piece_ptrs[2] = five.as_ptr();
    piece_lens[2] = 1;
    piece_ptrs[3] = b">".as_ptr();
    piece_lens[3] = 1;
    let total = 1 + text.len() as u32 + 1 + 1;
    let result = concat_parts_rooted_copy::<4>(
        slots.as_ptr(),
        4,
        &mut piece_ptrs,
        &piece_lens,
        &[0u32; 4],
        total,
        total,
        STRING_FLAG_WTF8_VALIDATED,
    );
    assert_eq!(bytes(result), format!("<{text}5>").as_bytes());
}

#[test]
fn literal_value_box_matches_the_fused_value_box() {
    use super::super::concat::js_string_concat_value_box;
    let prefixes: [(&'static str, f64); 4] = [
        ("k", heap("k")),
        ("item-", heap("item-")),
        ("user_id_", heap("user_id_")),
        ("abcd", heap("abcd")),
    ];
    let values = [
        0.0,
        7.0,
        42.0,
        99_999.0,
        100_000.0,
        750_000.0,
        4_294_967_295.0,
        4_294_967_296.0,
        -1.0,
        -0.0,
        1.5,
        f64::NAN,
        f64::INFINITY,
        1e21,
    ];
    for (text, prefix_box) in prefixes {
        let prefix = (prefix_box.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        for (v, eligible) in values
            .iter()
            .flat_map(|v| [(*v, 0), (*v, CONCAT_LIT_MEMO_ELIGIBLE)])
        {
            let meta = text.len() as u64 | ((text.len() as u64) << 32) | eligible;
            let new = js_string_concat_lit_value_box(text.as_ptr(), meta, v);
            let old = js_string_concat_value_box(prefix, v);
            assert_eq!(
                JSValue::from_bits(new.to_bits()).is_short_string(),
                JSValue::from_bits(old.to_bits()).is_short_string(),
                "{text}+{v}: inline/heap representation"
            );
            let text_of = |boxed: f64| -> Vec<u8> {
                let bits = boxed.to_bits();
                if JSValue::from_bits(bits).is_short_string() {
                    let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                    let len = JSValue::from_bits(bits).short_string_to_buf(&mut buf);
                    buf[..len].to_vec()
                } else {
                    bytes((bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader)
                }
            };
            assert_eq!(text_of(new), text_of(old), "{text}+{v}");
        }
    }
    // A non-number takes the ToString arm.
    let meta = 1u64 | (1u64 << 32);
    let joined = js_string_concat_lit_value_box(b"k".as_ptr(), meta, heap("tail"));
    let ptr = (joined.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
    assert_eq!(bytes(ptr), b"ktail");
}

#[test]
fn digit_pair_formatter_is_exact() {
    let mut buf = [0u8; 32];
    for n in 0..=10_000_000u32 {
        let len = fast_itoa_u32(n, &mut buf);
        let text = n.to_string();
        assert_eq!(&buf[..len], text.as_bytes(), "{n}");
    }
    // Digit-count boundaries and extremes, plus a deterministic spread.
    let mut samples = vec![
        u32::MAX,
        u32::MAX - 1,
        99,
        100,
        101,
        999,
        1000,
        9_999_999,
        10_000_000,
    ];
    let mut x = 0x9E37_79B9u32;
    for _ in 0..200_000 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        samples.push(x);
    }
    for p in 1..10u32 {
        samples.push(10u32.pow(p) - 1);
        samples.push(10u32.pow(p));
    }
    for n in samples {
        let len = fast_itoa_u32(n, &mut buf);
        assert_eq!(&buf[..len], n.to_string().as_bytes(), "{n}");
    }
}
