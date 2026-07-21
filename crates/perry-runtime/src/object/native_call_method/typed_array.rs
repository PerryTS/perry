use super::*;

/// Dispatch a `%TypedArray%` instance method on an already-resolved
/// `TypedArrayHeader` pointer. Returns `Some(result)` when handled, `None` when
/// the method isn't a typed-array method (caller falls through to the generic
/// dispatch tower / catch-all). Shared between the raw-pointer (#654) and
/// NaN-boxed POINTER_TAG receiver paths so a `Uint8Array` local reaches the
/// element-typed `js_typed_array_*` helpers regardless of how codegen boxed
/// the receiver. Issues #2797 / #2798 / #2799 added the callback-bearing arms.
pub(crate) unsafe fn dispatch_typed_array_method(
    ta: *mut crate::typedarray::TypedArrayHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let arg0 = || -> f64 {
        if args_len >= 1 && !args_ptr.is_null() {
            *args_ptr
        } else {
            f64::NAN
        }
    };
    // #4091: validate the 1st argument is callable, throwing a spec `TypeError`
    // otherwise (this dynamic dispatch tower is the inline-`new` /
    // `Uint8Array`-local path, where the boxed callback is still available).
    // `map` uses %TypedArray%.prototype.map's distinct non-callable rendering.
    let validate_cb = |map_form: bool| -> *const crate::closure::ClosureHeader {
        let boxed = if args_len >= 1 && !args_ptr.is_null() {
            *args_ptr
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let p = if map_form {
            crate::array::js_validate_array_map_callback(ta as i64, boxed)
        } else {
            crate::array::js_validate_array_callback(boxed)
        };
        p as *const crate::closure::ClosureHeader
    };
    let r = match method_name {
        "length" => crate::typedarray::js_typed_array_length(ta) as f64,
        "at" => crate::typedarray::js_typed_array_at(ta, arg0()),
        "sort" => {
            // #2796: validate the comparator (function | undefined) before sorting.
            let cmp = if args_len >= 1 && !args_ptr.is_null() {
                crate::array::js_validate_array_comparator(*args_ptr)
                    as *const crate::closure::ClosureHeader
            } else {
                std::ptr::null()
            };
            let result = if cmp.is_null() {
                crate::typedarray::js_typed_array_sort_default(ta)
            } else {
                crate::typedarray::js_typed_array_sort_with_comparator(ta, cmp)
            };
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "toSorted" => {
            let cmp = if args_len >= 1 && !args_ptr.is_null() {
                crate::array::js_validate_array_comparator(*args_ptr)
                    as *const crate::closure::ClosureHeader
            } else {
                std::ptr::null()
            };
            let result = if cmp.is_null() {
                crate::typedarray::js_typed_array_to_sorted_default(ta)
            } else {
                crate::typedarray::js_typed_array_to_sorted_with_comparator(ta, cmp)
            };
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "toReversed" => f64::from_bits(
            JSValue::pointer(crate::typedarray::js_typed_array_to_reversed(ta) as *mut u8).bits(),
        ),
        // #2879: bulk `set(source, offset?)` and `copyWithin`.
        "set" => {
            let source = arg0();
            let offset = if args_len >= 2 && !args_ptr.is_null() {
                *args_ptr.add(1)
            } else {
                0.0
            };
            crate::typedarray::js_typed_array_set_from(ta, source, offset)
        }
        "copyWithin" => {
            let target = arg0();
            let start = if args_len >= 2 && !args_ptr.is_null() {
                *args_ptr.add(1)
            } else {
                0.0
            };
            let end = if args_len >= 3 && !args_ptr.is_null() {
                *args_ptr.add(2)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            f64::from_bits(
                JSValue::pointer(crate::typedarray::js_typed_array_copy_within(
                    ta, target, start, end,
                ) as *mut u8)
                .bits(),
            )
        }
        "with" => {
            let idx = arg0();
            let val = if args_len >= 2 && !args_ptr.is_null() {
                *args_ptr.add(1)
            } else {
                f64::NAN
            };
            f64::from_bits(
                JSValue::pointer(crate::typedarray::js_typed_array_with(ta, idx, val) as *mut u8)
                    .bits(),
            )
        }
        "findLast" => crate::typedarray::js_typed_array_find_last(ta, validate_cb(false)),
        "findLastIndex" => {
            crate::typedarray::js_typed_array_find_last_index(ta, validate_cb(false))
        }
        // #2797/#2798/#2799: callback-bearing %TypedArray% methods. The codegen
        // lowerers only fire for receivers it can statically prove are plain
        // Arrays; a `Uint8Array` local reaches this dynamic dispatch tower,
        // where these arms previously fell through to the undefined catch-all
        // (so `ta.map`/`ta.reduce`/`ta.find` silently no-op'd).
        "map" => {
            let result = crate::typedarray::js_typed_array_map(ta, validate_cb(true));
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "filter" => {
            let result = crate::typedarray::js_typed_array_filter(ta, validate_cb(false));
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "forEach" => crate::typedarray::js_typed_array_for_each(ta, validate_cb(false)),
        "some" => crate::typedarray::js_typed_array_some(ta, validate_cb(false)),
        "every" => crate::typedarray::js_typed_array_every(ta, validate_cb(false)),
        "find" => crate::typedarray::js_typed_array_find(ta, validate_cb(false)),
        "findIndex" => crate::typedarray::js_typed_array_find_index(ta, validate_cb(false)),
        "values" | "Symbol.iterator" | "@@iterator" => {
            let iter =
                crate::array::js_array_values_iter_obj(ta as *const crate::array::ArrayHeader);
            if iter == 0 {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                f64::from_bits(JSValue::pointer(iter as *mut u8).bits())
            }
        }
        "keys" => {
            let iter = crate::array::js_array_keys_iter_obj(ta as *const crate::array::ArrayHeader);
            if iter == 0 {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                f64::from_bits(JSValue::pointer(iter as *mut u8).bits())
            }
        }
        "entries" => {
            let iter =
                crate::array::js_array_entries_iter_obj(ta as *const crate::array::ArrayHeader);
            if iter == 0 {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                f64::from_bits(JSValue::pointer(iter as *mut u8).bits())
            }
        }
        "reduce" | "reduceRight" => {
            let cb = validate_cb(false);
            // initial value present only when a 2nd arg was passed.
            let (has_init, init) = if args_len >= 2 && !args_ptr.is_null() {
                (1, *args_ptr.add(1))
            } else {
                (0, f64::NAN)
            };
            if method_name == "reduce" {
                crate::typedarray::js_typed_array_reduce(ta, cb, has_init, init)
            } else {
                crate::typedarray::js_typed_array_reduce_right(ta, cb, has_init, init)
            }
        }
        // Non-callback search / view / join methods. These reach this tower
        // through the brand-checking `%TypedArray%.prototype` value-path thunks
        // (`typed_array_proto_thunks`); the receiver-typed fast path lowers them
        // via dedicated codegen. The array search helpers (`js_array_*_jsvalue`)
        // detect a registered TypedArray receiver and read its typed store, so a
        // `TypedArrayHeader*` cast to `ArrayHeader*` is sound here.
        "indexOf" | "lastIndexOf" | "includes" => {
            // Absent searchElement is `undefined`, NOT the NaN sentinel —
            // `new Float64Array([NaN]).includes()` must be false (SameValueZero
            // against undefined), and NaN never `===`-matches for indexOf.
            let value = if args_len >= 1 && !args_ptr.is_null() {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            let (has_from, from) = if args_len >= 2 && !args_ptr.is_null() {
                (1, *args_ptr.add(1))
            } else {
                (0, f64::NAN)
            };
            let arr = ta as *const crate::array::ArrayHeader;
            match method_name {
                "indexOf" => {
                    crate::array::js_array_indexOf_jsvalue(arr, value, from, has_from) as f64
                }
                "lastIndexOf" => {
                    crate::array::js_array_last_index_of_jsvalue(arr, value, from, has_from) as f64
                }
                _ => f64::from_bits(
                    JSValue::bool(
                        crate::array::js_array_includes_jsvalue(arr, value, from, has_from) != 0,
                    )
                    .bits(),
                ),
            }
        }
        "join" => {
            let sep = arg0();
            let s = crate::typedarray::js_typed_array_join_value(ta, sep);
            f64::from_bits(JSValue::string_ptr(s).bits())
        }
        // `%TypedArray%.prototype.toLocaleString` (§23.2.3.32): for each
        // element, `? ToString(? Invoke(element, "toLocaleString"))`, joined by
        // ",". When the user has NOT replaced `Number.prototype.toLocaleString`
        // (or `BigInt.prototype...` for the bigint kinds) the result is the
        // default comma-separated join, which Perry's plain `join` matches —
        // keep that fast path. With a patch installed, run the spec loop so
        // the user function is invoked per element (its result then goes
        // through ordinary ToString, running `toString`/`valueOf` and
        // propagating abrupt completions).
        "toLocaleString" => {
            let kind = crate::typedarray::lookup_typed_array_kind(ta as usize);
            let is_bigint = matches!(
                kind,
                Some(crate::typedarray::KIND_BIGINT64) | Some(crate::typedarray::KIND_BIGUINT64)
            );
            let builtin: &[u8] = if is_bigint { b"BigInt" } else { b"Number" };
            match builtin_proto_user_method(builtin, "toLocaleString") {
                None => {
                    let s = crate::typedarray::js_typed_array_join_value(
                        ta,
                        f64::from_bits(crate::value::TAG_UNDEFINED),
                    );
                    f64::from_bits(JSValue::string_ptr(s).bits())
                }
                Some(patched) => {
                    let len = crate::typedarray::js_typed_array_length(ta);
                    let mut out = String::new();
                    for k in 0..len {
                        if k > 0 {
                            out.push(',');
                        }
                        let elem = crate::typedarray::js_typed_array_get(ta, k);
                        let r = call_primitive_closure_value(elem, patched, std::ptr::null(), 0)
                            .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                        let s_hdr = crate::builtins::js_string_coerce(r);
                        out.push_str(
                            super::has_own_helpers::str_from_string_header(s_hdr).unwrap_or(""),
                        );
                    }
                    let s = crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32);
                    f64::from_bits(JSValue::string_ptr(s).bits())
                }
            }
        }
        "slice" => {
            // `ToIntegerOrInfinity` each index (runs `valueOf`/`Symbol.toPrimitive`,
            // which may throw) — `js_typed_array_slice` then does the relative-index
            // clamp. `end` absent / `undefined` → slice to the end (`i32::MAX`).
            let to_idx = |v: f64| -> i32 {
                let n = crate::builtins::js_number_coerce(v);
                if n.is_nan() {
                    0
                } else if n >= i32::MAX as f64 {
                    i32::MAX
                } else if n <= i32::MIN as f64 {
                    i32::MIN
                } else {
                    n.trunc() as i32
                }
            };
            let start = if args_len >= 1 && !args_ptr.is_null() {
                to_idx(*args_ptr)
            } else {
                0
            };
            let end = if args_len >= 2
                && !args_ptr.is_null()
                && !JSValue::from_bits((*args_ptr.add(1)).to_bits()).is_undefined()
            {
                to_idx(*args_ptr.add(1))
            } else {
                i32::MAX
            };
            let result = crate::typedarray::js_typed_array_slice(ta, start, end);
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "subarray" => {
            let (has_begin, begin) = if args_len >= 1 && !args_ptr.is_null() {
                (1, *args_ptr)
            } else {
                (0, f64::NAN)
            };
            let (has_end, end) = if args_len >= 2 && !args_ptr.is_null() {
                (1, *args_ptr.add(1))
            } else {
                (0, f64::NAN)
            };
            let result =
                crate::typedarray::js_typed_array_subarray(ta, has_begin, begin, has_end, end);
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "reverse" => {
            let result = crate::typedarray::js_typed_array_reverse(ta);
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "fill" => {
            let value = arg0();
            let (has_start, start) = if args_len >= 2 && !args_ptr.is_null() {
                (1, *args_ptr.add(1))
            } else {
                (0, f64::NAN)
            };
            let (has_end, end) = if args_len >= 3 && !args_ptr.is_null() {
                (1, *args_ptr.add(2))
            } else {
                (0, f64::NAN)
            };
            let result =
                crate::typedarray::js_typed_array_fill(ta, value, has_start, start, has_end, end);
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        _ => return None,
    };
    Some(r)
}
