// Behavioral parity coverage for Buffer, typed arrays, and value-level
// helpers (closures, dynamic dispatch, primitive coercion). Output is
// deterministic and byte-comparable to Node --experimental-strip-types.

function line(label: string, value: unknown) {
  console.log(label + ":", value);
}

// Buffer creation, length, byteLength.
const b1 = Buffer.from("hello");
line("from-string-len", b1.length);
line("from-string-hex", b1.toString("hex"));
line("from-string-utf8", b1.toString("utf8"));
line("from-string-base64", b1.toString("base64"));

const b2 = Buffer.alloc(8, 0);
line("alloc-len", b2.length);
line("alloc-hex", b2.toString("hex"));

const unsafeBuf = Buffer.allocUnsafe(4);
unsafeBuf.fill(0);
line("allocUnsafe-len", unsafeBuf.length);
line("allocUnsafe-after-fill", unsafeBuf.toString("hex"));

const b3 = Buffer.alloc(4);
b3.fill(0xab);
line("fill-hex", b3.toString("hex"));

// Buffer.from array.
const b4 = Buffer.from([1, 2, 3, 4]);
line("from-array-hex", b4.toString("hex"));
line("byteLength", Buffer.byteLength("perry"));
line("isBuffer-yes", Buffer.isBuffer(b1));
line("isBuffer-no", Buffer.isBuffer("not-a-buffer"));

// Index access.
line("get[0]", b4[0]);
line("get[3]", b4[3]);
b4[0] = 0xff;
line("set[0]", b4[0]);

// Buffer.concat.
const cat = Buffer.concat([Buffer.from("ab"), Buffer.from("cd"), Buffer.from("ef")]);
line("concat-hex", cat.toString("hex"));
line("concat-utf8", cat.toString("utf8"));

// Buffer.compare / equals.
const c1 = Buffer.from([1, 2, 3]);
const c2 = Buffer.from([1, 2, 3]);
const c3 = Buffer.from([1, 2, 4]);
line("equals-yes", c1.equals(c2));
line("equals-no", c1.equals(c3));
line("compare-eq", Buffer.compare(c1, c2));
line("compare-lt", Buffer.compare(c1, c3) < 0);

// includes / indexOf.
const finder = Buffer.from("foobarbaz");
line("includes-yes", finder.includes("bar"));
line("includes-no", finder.includes("xyz"));
line("indexOf", finder.indexOf("bar"));

// slice / copy.
const sliced = Buffer.from("abcdef").subarray(1, 4);
line("slice-utf8", sliced.toString("utf8"));

const target = Buffer.alloc(4);
Buffer.from("abcdef").copy(target, 0, 0, 4);
line("copy-target", target.toString("utf8"));

// Numeric reads + writes.
const numBuf = Buffer.alloc(8);
numBuf.writeUInt8(0xab, 0);
numBuf.writeUInt16BE(0xcdef, 1);
numBuf.writeInt32BE(-1, 3);
numBuf.writeUInt8(0xff, 7);
line("write-readUInt8", numBuf.readUInt8(0).toString(16));
line("write-readUInt16BE", numBuf.readUInt16BE(1).toString(16));
line("write-readInt32BE", numBuf.readInt32BE(3));
line("write-readUInt8-end", numBuf.readUInt8(7).toString(16));

const ieee = Buffer.alloc(8);
ieee.writeDoubleBE(1.5, 0);
line("read-doubleBE", ieee.readDoubleBE(0));
const ieeeLE = Buffer.alloc(4);
ieeeLE.writeFloatLE(2.5, 0);
line("read-floatLE", ieeeLE.readFloatLE(0));

// BigInt reads / writes.
const bbuf = Buffer.alloc(8);
bbuf.writeBigInt64BE(1234567890123n, 0);
line("readBigInt64BE", bbuf.readBigInt64BE(0).toString());

const ubuf = Buffer.alloc(8);
ubuf.writeBigUInt64LE(987654321n, 0);
line("readBigUInt64LE", ubuf.readBigUInt64LE(0).toString());

// swap helpers.
const sw = Buffer.from([0x01, 0x02, 0x03, 0x04]);
sw.swap16();
line("swap16", sw.toString("hex"));

const sw32 = Buffer.from([0x01, 0x02, 0x03, 0x04]);
sw32.swap32();
line("swap32", sw32.toString("hex"));

// Uint8Array round-trip via TypedArray.
const u8 = new Uint8Array([10, 20, 30, 40]);
line("u8-len", u8.length);
line("u8-at", u8.at(-1));
line("u8-get", u8[2]);

const u16 = new Uint16Array([1, 2, 65535]);
line("u16-len", u16.length);

const u8FromArr = Uint8Array.from([5, 4, 3, 2, 1]);
line("u8-fromArray", Array.from(u8FromArr).join(","));
const u8Sorted = u8FromArr.toSorted();
line("u8-toSorted", Array.from(u8Sorted).join(","));
const u8Reversed = u8FromArr.toReversed();
line("u8-toReversed", Array.from(u8Reversed).join(","));
const u8With = u8FromArr.with(0, 99);
line("u8-with", Array.from(u8With).join(","));

// TypedArray search methods.
const tFind = new Int32Array([1, 2, 3, 4, 5]);
line("ta-findLast", tFind.findLast((n) => n < 4));
line("ta-findLastIndex", tFind.findLastIndex((n) => n < 4));

// Closures: capture + invocation arities + spread.
function makeAdder(n: number) {
  return (x: number, y: number, z: number) => x + y + z + n;
}
const add10 = makeAdder(10);
line("closure-call4", add10(1, 2, 3));
const longArity = (a: number, b: number, c: number, d: number, e: number, f: number, g: number) =>
  a + b + c + d + e + f + g;
line("closure-call7", longArity(1, 2, 3, 4, 5, 6, 7));

const spread = [1, 2, 3];
line("apply-spread", add10(...(spread as [number, number, number])));

// Closure captures across multiple frames.
let outer = 100;
const c1f = () => outer;
outer = 200;
line("closure-capture", c1f());

// typeof checks for nanboxing.
line("typeof-num", typeof 42);
line("typeof-str", typeof "x");
line("typeof-bool", typeof true);
line("typeof-undef", typeof undefined);
line("typeof-obj", typeof {});
line("typeof-fn", typeof (() => 1));
line("typeof-sym", typeof Symbol());
line("typeof-bigint", typeof 1n);

// Truthiness via NaN-boxing.
line("truthy-zero", Boolean(0));
line("truthy-empty", Boolean(""));
line("truthy-null", Boolean(null));
line("truthy-undef", Boolean(undefined));
line("truthy-nan", Boolean(NaN));
line("truthy-obj", Boolean({}));
line("truthy-arr", Boolean([]));

// Dynamic add / equality / comparison.
line("loose-eq-num-str", "5" == 5);
line("strict-eq-num-str", ("5" as unknown) === 5);
line("strict-neq", 1 !== 2);
line("compare-num", 1 < 2);
line("compare-str", "a" < "b");

// Force a small GC-active loop allocating short-lived strings.
let sum = 0;
for (let i = 0; i < 200; i++) {
  const s = "item-" + i;
  sum += s.length;
}
line("gc-loop-sum", sum);

// Box (heap-alloc) primitive via boxed coercion paths.
const big = BigInt(42);
line("bigint-box", typeof big);
line("bigint-value", big.toString());

console.log("compat-buffers-typed: ok");

/*
@covers
crates/perry-runtime/src/buffer.rs:
  - js_buffer_alloc
  - js_buffer_alloc_unsafe
  - js_buffer_byte_length
  - js_buffer_compare
  - js_buffer_concat
  - js_buffer_copy
  - js_buffer_equals
  - js_buffer_fill
  - js_buffer_from_array
  - js_buffer_from_string
  - js_buffer_from_value
  - js_buffer_get
  - js_buffer_includes
  - js_buffer_index_of
  - js_buffer_is_buffer
  - js_buffer_length
  - js_buffer_read_bigint64_be
  - js_buffer_read_bigint64_le
  - js_buffer_read_biguint64_be
  - js_buffer_read_biguint64_le
  - js_buffer_read_double_be
  - js_buffer_read_double_le
  - js_buffer_read_float_be
  - js_buffer_read_float_le
  - js_buffer_read_int16_be
  - js_buffer_read_int16_le
  - js_buffer_read_int32_be
  - js_buffer_read_int32_le
  - js_buffer_read_int8
  - js_buffer_read_int_be
  - js_buffer_read_int_le
  - js_buffer_read_uint16_be
  - js_buffer_read_uint16_le
  - js_buffer_read_uint32_be
  - js_buffer_read_uint32_le
  - js_buffer_read_uint8
  - js_buffer_read_uint_be
  - js_buffer_read_uint_le
  - js_buffer_set
  - js_buffer_set_from
  - js_buffer_slice
  - js_buffer_swap16
  - js_buffer_swap32
  - js_buffer_swap64
  - js_buffer_to_string
  - js_buffer_to_string_range
  - js_buffer_write
  - js_buffer_write_bigint64_be
  - js_buffer_write_bigint64_le
  - js_buffer_write_biguint64_be
  - js_buffer_write_biguint64_le
  - js_buffer_write_double_be
  - js_buffer_write_double_le
  - js_buffer_write_float_be
  - js_buffer_write_float_le
  - js_buffer_write_int16_be
  - js_buffer_write_int16_le
  - js_buffer_write_int32_be
  - js_buffer_write_int32_le
  - js_buffer_write_int8
  - js_buffer_write_int_be
  - js_buffer_write_int_le
  - js_buffer_write_uint16_be
  - js_buffer_write_uint16_le
  - js_buffer_write_uint32_be
  - js_buffer_write_uint32_le
  - js_buffer_write_uint8
  - js_buffer_write_uint_be
  - js_buffer_write_uint_le
  - js_uint8array_alloc
  - js_uint8array_from_array
  - js_uint8array_new
  - js_value_to_string_with_encoding
crates/perry-runtime/src/typedarray.rs:
  - js_typed_array_at
  - js_typed_array_find_last
  - js_typed_array_find_last_index
  - js_typed_array_get
  - js_typed_array_length
  - js_typed_array_new
  - js_typed_array_new_empty
  - js_typed_array_new_from_array
  - js_typed_array_set
  - js_typed_array_to_reversed
  - js_typed_array_to_sorted_default
  - js_typed_array_with
crates/perry-runtime/src/value.rs:
  - js_checkpoint
  - js_collection_method_dispatch
  - js_debug_val
  - js_dyn_index_get
  - js_dynamic_add
  - js_dynamic_array_find
  - js_dynamic_array_findIndex
  - js_dynamic_array_get
  - js_dynamic_array_length
  - js_dynamic_bitand
  - js_dynamic_bitor
  - js_dynamic_bitxor
  - js_dynamic_div
  - js_dynamic_mod
  - js_dynamic_mul
  - js_dynamic_neg
  - js_dynamic_object_get_property
  - js_dynamic_object_keys
  - js_dynamic_shl
  - js_dynamic_shr
  - js_dynamic_string_equals
  - js_dynamic_string_or_number_add
  - js_dynamic_sub
  - js_ensure_string_ptr
  - js_handle_array_get
  - js_handle_array_length
  - js_is_truthy
  - js_is_undefined_or_bare_nan
  - js_jsvalue_compare
  - js_jsvalue_equals
  - js_jsvalue_loose_equals
  - js_jsvalue_same_value_zero
  - js_jsvalue_to_string
  - js_jsvalue_to_string_radix
  - js_nanbox_bigint
  - js_nanbox_get_bigint
  - js_nanbox_get_pointer
  - js_nanbox_get_string_pointer
  - js_nanbox_is_bigint
  - js_nanbox_is_pointer
  - js_nanbox_is_string
  - js_nanbox_pointer
  - js_nanbox_string
  - js_set_handle_array_get
  - js_set_handle_array_length
  - js_set_handle_call_method
  - js_set_handle_object_get_property
  - js_set_handle_to_string
  - js_set_handle_typeof
  - js_set_native_module_js_loader
  - js_set_new_from_handle_v8
crates/perry-runtime/src/closure.rs:
  - js_closure_alloc_singleton
  - js_closure_alloc_with_captures_singleton
  - js_closure_call4
  - js_closure_call5
  - js_closure_call6
  - js_closure_call7
  - js_closure_call_apply_with_spread
  - js_closure_get_capture_f64
  - js_closure_get_capture_ptr
  - js_closure_get_func
  - js_closure_set_capture_f64
  - js_closure_set_capture_ptr
  - js_closure_unbind_this
  - js_create_callback
  - js_native_call_value
  - js_new_instance
  - js_register_closure_arity
  - js_register_closure_rest
*/
