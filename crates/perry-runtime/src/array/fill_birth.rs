//! The fill birth: `new Array(k).fill(v)` born once, in its final
//! representation.
//!
//! The two-step path allocates `new Array(k)` with every slot a hole
//! (`js_array_constructor_single`), then the native `fill` writes the value
//! into every slot and re-derives the array's layout by reading them all back
//! (`js_array_fill`, `rebuild_array_layout`), and an at-rest birth then
//! rewrites every slot as an I32 word (`layout_slot::js_array_rest_encode`).
//! Every slot is written two or three times and read once or twice, for a
//! result that is a function of `k` and one value.
//!
//! [`js_array_fill_birth`] is that result in one pass: one allocation with
//! the final length and capacity (`perry_abi::fill_birth_capacity`), the
//! value stored once per slot in its final word (the canonical double of a
//! Number, or its I32 word for an at-rest birth), the slack as holes, and
//! the header the two-step path leaves (`perry_abi::fill_birth_layout`, the
//! element-kind prefix `k`). Nothing is visible before the last store: the
//! allocation is the only call, and it comes first.
//!
//! Generated code calls it for a fused fill birth the facts name (wpfacts
//! `fill_births`), or emits the same birth inline for a small one
//! (`perry-codegen` `expr/fill_birth.rs`); both take the capacity and the
//! layout bits from `perry-abi`, and the tests below hold them to the
//! two-step path.
//!
//! Every birth the one pass does not cover takes the two-step path itself,
//! unchanged: a value that is no Number (a string is demoted to shared, a
//! pointer value makes the layout pointer bearing), an empty birth, a length
//! past the dense bound, an at-rest value that has no I32 word (a false
//! proof, reported by `js_array_rest_encode`).

use super::ArrayHeader;

const _: () = assert!(super::MIN_ARRAY_CAPACITY == perry_abi::MIN_ARRAY_CAPACITY);
const _: () = assert!(crate::gc::GC_ARRAY_RAW_F64_HOLES == perry_abi::GC_ARRAY_RAW_F64_HOLES);
const _: () = assert!(crate::layout_slot::GC_ARRAY_I32_LAYOUT == perry_abi::GC_ARRAY_I32_LAYOUT);
const _: () = assert!(
    perry_abi::fill_birth_layout(false)
        == crate::gc::GC_LAYOUT_POINTER_FREE
            | crate::gc::GC_ARRAY_RAW_F64_LAYOUT
            | crate::gc::GC_ARRAY_RAW_F64_HOLES
);
const _: () = assert!(
    perry_abi::fill_birth_layout(true)
        == crate::gc::GC_LAYOUT_POINTER_FREE | crate::layout_slot::GC_ARRAY_I32_LAYOUT
);

/// The word every slot of the birth holds when the one pass covers it: the
/// I32 word of an int32 Number for an at-rest birth (never `-0`), else the
/// canonical double of a Number (what `rebuild_array_numeric_raw_f64` leaves
/// in a slot: an INT32-boxed integer becomes its double).
fn final_word(bits: u64, rest: bool) -> Option<u64> {
    if rest {
        crate::layout_slot::encode(bits)
    } else {
        super::value_bits_to_number(bits).map(f64::to_bits)
    }
}

/// `new Array(length).fill(value)` in one pass (module docs). `reserve` is
/// the capacity an at-rest birth reserves (0: none); `rest != 0` stores the
/// elements as I32 words. Returns the born array's head. May collect (the
/// allocation).
#[no_mangle]
pub extern "C" fn js_array_fill_birth(
    length: u32,
    reserve: u32,
    value: f64,
    rest: u32,
) -> *mut ArrayHeader {
    let rest = rest != 0;
    let word = match final_word(value.to_bits(), rest) {
        Some(w) if length != 0 && length <= perry_abi::MAX_FRESH_DENSE_ARRAY_LENGTH => w,
        _ => return two_step(length, reserve, value, rest),
    };
    let capacity =
        super::alloc::array_capacity_or_throw(perry_abi::fill_birth_capacity(length, reserve));
    // `word` is no pointer, so nothing needs a root across the allocation.
    let arr = crate::arena::arena_alloc_gc(
        super::array_byte_size(capacity as usize),
        8,
        crate::gc::GC_TYPE_ARRAY,
    ) as *mut ArrayHeader;
    unsafe {
        (*arr).length = length;
        (*arr).capacity = capacity;
        (*arr).named = std::ptr::null_mut();
        // EK §3.1: `fill` proves the prefix it writes (`ek_after_fill`).
        (*arr).ek_prefix = length;
        (*arr).ek_kind = 0;
        let slots = super::array_elements_ptr(arr as *const ArrayHeader) as *mut u64;
        for i in 0..length as usize {
            // GC_STORE_AUDIT(INIT): a Number word in a fresh, unpublished array.
            std::ptr::write(slots.add(i), word);
        }
        for i in length as usize..capacity as usize {
            // GC_STORE_AUDIT(INIT): TAG_HOLE is a non-pointer sentinel for fresh array slots.
            std::ptr::write(slots.add(i), crate::value::TAG_HOLE);
        }
        super::alloc::install_birth_layout(arr, perry_abi::fill_birth_layout(rest));
    }
    arr
}

/// The two-step birth, as generated code made it before the fill birth:
/// `new Array(length)`, the native `fill`, and for an at-rest birth the
/// reservation and the encode.
fn two_step(length: u32, reserve: u32, value: f64, rest: bool) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let arr = super::alloc::js_array_constructor_single(f64::from(length));
    let arr = super::js_array_fill(arr, value.get_nanbox_f64());
    if !rest {
        return arr;
    }
    let boxed = crate::value::js_nanbox_pointer(arr as i64);
    let born = if reserve != 0 {
        crate::layout_slot::js_array_rest_reserve(boxed, reserve)
    } else {
        crate::layout_slot::js_array_rest_encode(boxed)
    };
    (born.to_bits() & crate::value::POINTER_MASK) as *mut ArrayHeader
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a reader can tell two arrays apart by: the layout bits, the
    /// length, the capacity, the element-kind word and every slot word.
    unsafe fn image(arr: *mut ArrayHeader) -> (u16, u32, u32, u32, u32, Vec<u64>) {
        let header = (arr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        let slots = crate::array::array_elements_ptr(arr as *const ArrayHeader);
        let words = (0..(*arr).capacity as usize)
            .map(|i| *slots.add(i))
            .collect();
        (
            (*header)._reserved,
            (*arr).length,
            (*arr).capacity,
            (*arr).ek_prefix,
            (*arr).ek_kind,
            words,
        )
    }

    fn numbers() -> Vec<f64> {
        vec![
            0.0,
            -0.0,
            1.0,
            -7.0,
            1.5,
            f64::NAN,
            f64::INFINITY,
            2147483648.0,
            f64::from_bits(crate::value::INTERNAL_WORD_TAG | 5),
        ]
    }

    /// The one pass leaves what the two-step path leaves, for every Number
    /// and every small length: same layout bits, length, capacity, element
    /// kind word and slot words.
    #[test]
    fn a_number_birth_is_the_two_step_birth() {
        for length in [1u32, 2, 3, 4, 5, 8, 16, 17, 128] {
            for value in numbers() {
                let born = js_array_fill_birth(length, 0, value, 0);
                let old = two_step(length, 0, value, false);
                unsafe {
                    assert_eq!(image(born), image(old), "length {length} value {value:?}");
                    assert_eq!(image(born).0, perry_abi::fill_birth_layout(false));
                    assert_eq!((*born).capacity, perry_abi::fill_birth_capacity(length, 0));
                }
            }
        }
    }

    /// An at-rest birth, with and without a reservation, is the two-step
    /// birth followed by the encode: I32 words below the length, holes in
    /// the slack, the I32 layout bit and no raw-f64 bit.
    #[test]
    fn an_at_rest_birth_is_the_encoded_two_step_birth() {
        for (length, reserve) in [(1u32, 0u32), (8, 0), (8, 16), (128, 256), (5, 3), (3, 0)] {
            for value in [0.0, 1.0, -7.0, 2147483647.0, -2147483648.0] {
                let born = js_array_fill_birth(length, reserve, value, 1);
                let old = two_step(length, reserve, value, true);
                unsafe {
                    assert_eq!(
                        image(born),
                        image(old),
                        "length {length} reserve {reserve} value {value}"
                    );
                    assert_eq!(image(born).0, perry_abi::fill_birth_layout(true));
                    assert_eq!(
                        (*born).capacity,
                        perry_abi::fill_birth_capacity(length, reserve)
                    );
                    assert!(crate::layout_slot::is_i32_layout(born));
                    let slots = crate::array::array_elements_ptr(born as *const ArrayHeader);
                    assert_eq!(
                        *slots,
                        crate::value::INTERNAL_WORD_TAG | u64::from(value as i32 as u32)
                    );
                }
            }
        }
    }

    /// What the one pass does not cover is born by the two-step path: an
    /// empty birth, a value that is no Number, an at-rest value with no I32
    /// word. The result is the two-step result.
    #[test]
    fn every_other_birth_takes_the_two_step_path() {
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
        let truth = f64::from_bits(crate::value::TAG_TRUE);
        for (length, value, rest) in [
            (0u32, 0.0, 0u32),
            (0, 0.0, 1),
            (8, undefined, 0),
            (8, truth, 0),
            (8, 1.5, 1),
            (8, -0.0, 1),
        ] {
            let born = js_array_fill_birth(length, 0, value, rest);
            let old = two_step(length, 0, value, rest != 0);
            unsafe {
                assert_eq!(image(born), image(old), "length {length} value {value:?}");
            }
        }
        // A string value: every slot holds it, and the layout is the
        // two-step one (pointer bearing).
        let text = crate::string::js_string_from_bytes(b"fill birth".as_ptr(), 10);
        let value = f64::from_bits(crate::value::STRING_TAG | text as u64);
        let born = js_array_fill_birth(6, 0, value, 0);
        let old = two_step(6, 0, value, false);
        unsafe {
            assert_eq!(image(born), image(old));
            assert_eq!(image(born).5[5], value.to_bits());
        }
    }

    /// The capacity rule: a dense `new Array(k)`'s slots, or the reservation
    /// when it is more.
    #[test]
    fn the_capacity_is_the_constructors_or_the_reservation() {
        assert_eq!(perry_abi::fill_birth_capacity(1, 0), 4);
        assert_eq!(perry_abi::fill_birth_capacity(4, 0), 4);
        assert_eq!(perry_abi::fill_birth_capacity(8, 0), 8);
        assert_eq!(perry_abi::fill_birth_capacity(8, 16), 16);
        assert_eq!(perry_abi::fill_birth_capacity(8, 6), 8);
        assert_eq!(perry_abi::fill_birth_capacity(2, 3), 4);
        for length in [1u32, 3, 4, 9, 100] {
            let old = super::super::alloc::js_array_constructor_single(f64::from(length));
            unsafe {
                assert_eq!((*old).capacity, perry_abi::fill_birth_capacity(length, 0));
            }
        }
    }
}
