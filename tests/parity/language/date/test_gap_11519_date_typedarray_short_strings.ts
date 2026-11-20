// #11519: Date and typed-array element stores must accept a SHORT (SSO) string.
// A string of up to 5 bytes built at runtime -- `String(n)`, a template, a
// `+` concatenation, `JSON.parse` -- is stored inline in the NaN-box with no
// StringHeader behind it. `new Date(s)`, `new Date(y, s)` and the typed-array
// ToNumber checked for the heap tag only, so they read the inline string as
// NaN; `Date.parse(s)` masked it into an address and segfaulted.
const S = (s: string): string => (s.length ? s.charAt(0) + s.slice(1) : JSON.parse('""'));

// Edge lengths: empty, 4 and 5 bytes (5 is the SSO maximum), and a 7-byte heap
// control. (Loose one-digit / five-digit year strings are left out: Perry's date
// parser disagrees with V8's legacy heuristics there for heap strings too.)
for (const v of ["", "1999", " 2020", "2020-01"]) {
  const s = S(v);
  console.log(`len ${v.length}:`, new Date(s).getTime(), Date.parse(s), `${new Date(0).setUTCFullYear(s as any)}`);
}
console.log("new Date(tpl):", new Date(`${2020}`).getUTCFullYear(), new Date(String(1999)).toISOString());
console.log("new Date(json):", new Date(JSON.parse('"2001"')).getUTCFullYear());
console.log("new Date(y, m, d):", new Date(2020, S("1") as any, S("15") as any).getDate(), new Date(2020, String(11) as any).getMonth());
console.log("Date.UTC:", Date.UTC(S("2020") as any, S("1") as any, `${3}` as any));
console.log("Date.parse(tpl):", Date.parse(`${1970}`), Date.parse(String(2000)));

// Through an untyped setter: the generic element store. (A store the compiler
// can see is a `Uint8Array` writes 0 for ANY string, heap or inline -- a
// separate gap, not an SSO one.)
const setA = (a: any, i: number, v: any) => {
  a[i] = v;
};
const u8 = new Uint8Array(4);
setA(u8, 0, S("7"));
setA(u8, 1, S("255"));
setA(u8, 2, S("x"));
setA(u8, 3, S(""));
console.log("u8:", u8.join());
const f64 = new Float64Array(3);
f64[0] = S("2.5") as any;
f64[1] = `${-4}` as any;
f64[2] = S("1e3") as any;
console.log("f64:", f64.join());
const i32 = new Int32Array(2);
i32[0] = String(12345) as any;
i32[1] = S("-9") as any;
console.log("i32:", i32.join());
const any: any = new Float32Array(2);
any[0] = S("0.5");
any[1] = String(3);
console.log("f32 any:", any.join());
