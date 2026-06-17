function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", String(fn()));
  } catch (e: any) {
    console.log(label + ":", e?.name + ":" + e?.message);
  }
}

// 2^32-1 (4294967295) is NOT a valid array index: it stays an ordinary
// string-keyed property and does not change `length`.
const a: any = [0, 1, 2];
a[4294967295] = "x";
show("2^32-1 length", () => a.length);
show("2^32-1 value", () => a[4294967295]);
show("2^32-1 own", () => Object.prototype.hasOwnProperty.call(a, "4294967295"));

// 2^32-2 (4294967294) is the maximum valid array index: writing it sets
// `length` to 2^32-1 with the element stored sparsely.
const b: any = [0, 1, 2];
b[4294967294] = "y";
show("2^32-2 length", () => b.length);
show("2^32-2 value", () => b[4294967294]);
show("2^32-2 own", () => Object.prototype.hasOwnProperty.call(b, "4294967294"));
show("2^32-2 keys", () => Object.keys(b).join("|"));

// 2^32 (4294967296) is past the index range: ordinary string property.
const c: any = [0, 1, 2];
c[4294967296] = "z";
show("2^32 length", () => c.length);
show("2^32 value", () => c[4294967296]);
show("2^32 own", () => Object.prototype.hasOwnProperty.call(c, "4294967296"));

// An explicit numeric-string key behaves like the string property above.
const d: any = [0, 1, 2];
d["4294967295"] = "s";
show("string 2^32-1 length", () => d.length);
show("string 2^32-1 value", () => d["4294967295"]);

// A non-integer numeric key is the string property "1.5"; it must not be
// truncated onto element 1.
const e: any = [0, 1, 2];
e[1.5] = "half";
show("1.5 length", () => e.length);
show("1.5 value", () => e[1.5]);
show("1.5 element-one", () => e[1]);

// A negative numeric key is the string property "-1".
const f: any = [0, 1, 2];
f[-1] = "neg";
show("-1 length", () => f.length);
show("-1 value", () => f[-1]);
