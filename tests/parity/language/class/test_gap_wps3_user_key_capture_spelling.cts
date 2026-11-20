// WPS3 class E: a user key spelled like a compiler capture field (`__perry_cap_user`) is an
// ordinary property; it must not change how the constructor or enumeration treats the class.
const M = require("../../../fixtures/programs/_helpers/wps3_user_key_capture_spelling.cjs");
function show(v: any) {
  console.log(JSON.stringify(v), Object.keys(v).join(","), v.a, v.__perry_cap_user);
}
show(new M.Plain(1));
