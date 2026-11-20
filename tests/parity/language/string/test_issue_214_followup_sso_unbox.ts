// Issue #214 follow-up — SSO unbox sites that #214 didn't catch.
//
// Pre-fix, eight call shapes routed string-typed values through
// `unbox_to_i64` (bitcast+mask) instead of `unbox_str_handle`
// (js_get_string_pointer_unified). For SSO-tagged inputs (≤5-byte
// ASCII strings, common from JSON.parse / .slice / String.fromCharCode)
// the lower-48-bit mask returned the inline-payload bits, which the
// runtime then dereferenced as `*StringHeader` — silent garbage
// (arr.join → empty), or SIGSEGV (string.match / crypto / property
// access). Each section below reproduces one of the patterns and is
// compared byte-for-byte against `node --experimental-strip-types`.

// Helper: produce a guaranteed-SSO short ASCII string.
function sso(s: string): string {
    return JSON.parse(JSON.stringify(s));
}

// 1) arr.join(SSO separator) — js_array_join derefs sep StringHeader.
console.log("[1]", [1, 2, 3].join(sso("-")));

// 2) Expr::ArrayJoin variant via .join("," literal) is the safe path,
//    so test the explicit .join(sep) shape that lowers to the variant
//    when the receiver is an array literal.
const items = [10, 20, 30];
console.log("[2]", items.join(sso(":")));

// 3) obj[SSO key] read — js_object_get_field_by_name_f64 derefs key.
const lookup: Record<string, number> = { abc: 1, xyz: 2 };
const k1 = sso("abc");
console.log("[3]", lookup[k1]);

// 4) obj[SSO key] write — js_object_set_field_by_name derefs key.
const sink: Record<string, string> = {};
const k2 = sso("foo");
sink[k2] = "wrote";
console.log("[4]", sink["foo"]);

// 5) delete obj[SSO key] — js_object_delete_field derefs key.
const target: Record<string, boolean> = { gone: true, kept: true };
const k3 = sso("gone");
delete target[k3];
console.log("[5]", "gone" in target, "kept" in target);

// 6) SSO_string.match(/regex/) — js_string_match derefs receiver.
//    Pre-fix this segfaulted with exit code 139.
const hay = sso("abc");
const m = hay.match(/b/);
console.log("[6]", m && m[0]);

// 7) process.env[SSO_dynamic_name] — js_getenv derefs the name.
//    PATH is set on every host; sso("PATH") would crash pre-fix on the
//    SSO unbox of "PATH" (4 bytes, ASCII → SSO-eligible).
const haveValue = !!process.env[sso("PATH")];
console.log("[7]", haveValue);

// Note: crypto.createHash chain SSO sites at expr.rs:7100/7120/7132 are
// also fixed but the chain itself doesn't fully resolve through Perry's
// codegen yet (separate pre-existing limitation — both literal and SSO
// inputs return "0" instead of the digest). The fix is defensive for
// when that chain becomes wired.
