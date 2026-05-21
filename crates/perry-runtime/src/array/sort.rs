//! Mutating sort — default + comparator.
use super::*;
use crate::closure::{js_closure_call2, ClosureHeader};
use std::ptr;

/// Array.prototype.sort() default sort with no comparator. Per JS
/// semantics, elements are converted to strings and compared
/// lexicographically. Sorts in place and returns the same array pointer.
#[no_mangle]
pub extern "C" fn js_array_sort_default(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    use crate::string::StringHeader;
    use crate::value::js_jsvalue_to_string;
    unsafe {
        let arr = clean_arr_ptr(arr as *const ArrayHeader) as *mut ArrayHeader;
        if arr.is_null() {
            return arr;
        }
        // Issue #654: route typed-array receivers (compiler statically
        // typed `arr` as `Float64Array | Int32Array | …` and emitted the
        // ArraySort lowering) through the typed-array sorter so element
        // bytes are read by the right per-kind accessor instead of as
        // raw f64. Without this, `Int8Array.sort()` produced 4 i8 cells
        // re-interpreted as 8-byte f64s — garbage values + occasional
        // OOB reads.
        if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
            return crate::typedarray::js_typed_array_sort_default(
                arr as *mut crate::typedarray::TypedArrayHeader,
            ) as *mut ArrayHeader;
        }
        let length = (*arr).length as usize;
        if length <= 1 {
            return arr;
        }
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        // Materialize each element as an owned Rust `String` while keeping the
        // original f64 bits. Using strings (not pointer equality) guarantees
        // correct ordering for numbers, NaN-boxed strings, booleans, null and
        // undefined — matching JS default sort semantics.
        let mut pairs: Vec<(String, f64)> = Vec::with_capacity(length);
        for i in 0..length {
            let val = *elements_ptr.add(i);
            let str_ptr = js_jsvalue_to_string(val);
            let s = if str_ptr.is_null() {
                String::new()
            } else {
                let header = &*(str_ptr as *const StringHeader);
                let bytes_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let slice = std::slice::from_raw_parts(bytes_ptr, header.byte_len as usize);
                std::str::from_utf8(slice).unwrap_or("").to_string()
            };
            pairs.push((s, val));
        }

        // Stable lexicographic sort on the string keys.
        pairs.sort_by(|a, b| a.0.cmp(&b.0));

        for (i, (_, val)) in pairs.into_iter().enumerate() {
            *elements_ptr.add(i) = val;
        }
        rebuild_array_layout(arr);

        arr
    }
}

/// sort - sort array in-place using a comparator closure
/// The comparator takes (a, b) and returns negative if a < b, positive if a > b, 0 if equal
/// Returns the same array pointer (sorts in-place)
#[no_mangle]
pub extern "C" fn js_array_sort_with_comparator(
    arr: *mut ArrayHeader,
    comparator: *const ClosureHeader,
) -> *mut ArrayHeader {
    unsafe {
        let arr = clean_arr_ptr(arr as *const ArrayHeader) as *mut ArrayHeader;
        if arr.is_null() {
            return arr;
        }
        // Issue #654: same routing as `js_array_sort_default` — when
        // codegen statically typed the receiver as a typed array but
        // chose the generic ArraySort HIR lowering, dispatch through
        // the typed-array helper instead of treating the buffer as f64s.
        if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
            return crate::typedarray::js_typed_array_sort_with_comparator(
                arr as *mut crate::typedarray::TypedArrayHeader,
                comparator,
            ) as *mut ArrayHeader;
        }
        let length = (*arr).length as usize;
        if length <= 1 {
            return arr;
        }
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        mark_array_layout_unknown(arr);

        // Hoist the closure-dispatch resolution out of the hot loops.
        // For a 1.25M-element sort we'd otherwise hit ~50M HashMap lookups
        // (per-call rest + arity registry queries inside `js_closure_call2`).
        // When the comparator is a plain (a,b) => ... arrow with no captures
        // and no rest, `direct_call` is `Some(typed_fn)` and we call it
        // unconditionally inside the loop. Falls back to `js_closure_call2`
        // for the rare bound-method / rest / over-arity comparators.
        let direct_call = crate::closure::resolve_call2_direct(comparator);

        #[inline(always)]
        unsafe fn cmp_with(
            comparator: *const ClosureHeader,
            direct: Option<extern "C" fn(*const ClosureHeader, f64, f64) -> f64>,
            a: f64,
            b: f64,
        ) -> f64 {
            match direct {
                Some(f) => f(comparator, a, b),
                None => js_closure_call2(comparator, a, b),
            }
        }

        // TimSort-style hybrid: insertion sort for small runs, merge sort for large arrays.
        // Stable, O(n log n) worst case. Insertion sort is used for runs <= 32 elements
        // because it has lower overhead for small inputs.
        const INSERTION_THRESHOLD: usize = 32;

        if length <= INSERTION_THRESHOLD {
            // Insertion sort for small arrays
            for i in 1..length {
                let key = *elements_ptr.add(i);
                let mut j = i as isize - 1;
                while j >= 0 {
                    let cmp = cmp_with(comparator, direct_call, *elements_ptr.add(j as usize), key);
                    if cmp > 0.0 {
                        ptr::write(
                            elements_ptr.add((j + 1) as usize),
                            *elements_ptr.add(j as usize),
                        );
                        j -= 1;
                    } else {
                        break;
                    }
                }
                ptr::write(elements_ptr.add((j + 1) as usize), key);
            }
        } else {
            // Bottom-up merge sort for large arrays — O(n log n) stable sort
            let mut buf: Vec<f64> = Vec::with_capacity(length);
            buf.set_len(length);

            // Phase 1: Sort small runs with insertion sort
            let mut run_start = 0;
            while run_start < length {
                let run_end = (run_start + INSERTION_THRESHOLD).min(length);
                for i in (run_start + 1)..run_end {
                    let key = *elements_ptr.add(i);
                    let mut j = i as isize - 1;
                    while j >= run_start as isize {
                        let cmp =
                            cmp_with(comparator, direct_call, *elements_ptr.add(j as usize), key);
                        if cmp > 0.0 {
                            ptr::write(
                                elements_ptr.add((j + 1) as usize),
                                *elements_ptr.add(j as usize),
                            );
                            j -= 1;
                        } else {
                            break;
                        }
                    }
                    ptr::write(elements_ptr.add((j + 1) as usize), key);
                }
                run_start = run_end;
            }

            // Phase 2: Merge runs, doubling width each pass
            let buf_ptr = buf.as_mut_ptr();
            let mut width = INSERTION_THRESHOLD;
            let mut src = elements_ptr;
            let mut dst = buf_ptr;

            while width < length {
                let mut i = 0;
                while i < length {
                    let left = i;
                    let mid = (i + width).min(length);
                    let right = (i + 2 * width).min(length);

                    // Merge [left..mid) and [mid..right) into dst
                    let mut l = left;
                    let mut r = mid;
                    let mut k = left;
                    while l < mid && r < right {
                        let cmp = cmp_with(comparator, direct_call, *src.add(l), *src.add(r));
                        if cmp <= 0.0 {
                            *dst.add(k) = *src.add(l);
                            l += 1;
                        } else {
                            *dst.add(k) = *src.add(r);
                            r += 1;
                        }
                        k += 1;
                    }
                    while l < mid {
                        *dst.add(k) = *src.add(l);
                        l += 1;
                        k += 1;
                    }
                    while r < right {
                        *dst.add(k) = *src.add(r);
                        r += 1;
                        k += 1;
                    }

                    i += 2 * width;
                }
                // Swap src and dst for next pass
                std::mem::swap(&mut src, &mut dst);
                width *= 2;
            }

            // If final result is in buf, copy back to elements
            if src != elements_ptr {
                ptr::copy_nonoverlapping(src, elements_ptr, length);
            }
        }
        rebuild_array_layout(arr);

        arr
    }
}
