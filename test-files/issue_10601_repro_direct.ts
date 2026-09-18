// #10601: isolated repro. A DataView-crafted NaN payload whose bits are
// 0x7FFE_0000_0000_0005 -- the exact tag band ("0x7FFE") Perry reserves for
// INT32-tagged values (both ordinary boxed int32 numbers AND codegen-emitted
// class refs use this same top-16-bit tag; see CLAUDE.md's NaN-boxing table
// and crates/perry-runtime/src/object/native_module/class_ref_values.rs).
//
// Expected (Node 26.5.1): `number true` -- it is a completely ordinary
// (if unusual) NaN as far as pure ECMA-262 is concerned; DataView doesn't
// care that the payload happens to alias one of Perry's internal tags.
//
// Perry (pre-fix, and STILL after #10592's class_ref_id fix): `number false`
// -- Number.isNaN is wrong because Perry's `is_number()`/NaN-classification
// treats the ENTIRE top16 band [0x7FF9,0x7FFF] as "not a number at all"
// (reserved for its own tags), so a genuine IEEE-754 NaN landing in that
// band never reaches the NaN check.
const dv = new DataView(new ArrayBuffer(8));
dv.setUint32(0, 0x7ffe0000, false);
dv.setUint32(4, 5, false);
const crafted: any = dv.getFloat64(0, false);
console.log(typeof crafted, Number.isNaN(crafted));
