// Reading an array element through a *string* key — `a["1"]`, `a[k]` where k
// is a numeric string, `a[String(i)]` — must resolve the same element as the
// numeric `a[1]`, per ToPropertyKey / OrdinaryGet (a canonical numeric-string
// index addresses the element store). Previously the codegen array fast path
// and the typed-feedback boxed fallback `fptosi`'d the key, collapsing a
// NaN-boxed string to index 0, so every string-keyed read returned element 0
// (or undefined). Negative / non-index string keys are ordinary expando
// properties; `length` and Array.prototype methods resolve through the key
// as well.

function show(label: string, value: any) {
  console.log(label + " = " + value);
}

// number[] — string-literal and string-variable index.
const a = [10, 20, 30];
show('a["0"]', a["0"]);
show('a["1"]', a["1"]);
show('a["2"]', a["2"]);
show('a["3"] oob', a["3"]);
const k = "1";
show("a[k]", a[k]);
show("a[String(2)]", a[String(2)]);

// Dynamic numeric-string keys from a forEach (issue #637 sibling).
["0", "1", "2"].forEach((key) => show("byKey " + key, a[key as any]));

// string[] — element type isn't a pointer-free number, exercises the
// non-numeric-layout codegen path.
const s = ["x", "y", "z"];
show('s["1"]', s["1"]);
show("s[k]", s[k]);

// object[] — string index then property read.
const o = [{ v: 10 }, { v: 20 }];
show('o["1"].v', o["1"].v);

// 2D int matrix (#50 flat-const) — string-keyed double index.
const m = [[1, 2], [3, 4]];
show('m["1"]["0"]', m["1"]["0"]);
show('m[1]["0"]', m[1]["0"]);
show('m["1"][0]', m["1"][0]);

// Negative / non-index string keys are ordinary own (expando) properties.
const b: any = [1, 2, 3];
b[-1] = 99;
b["meta"] = "hi";
show("b[-1]", b[-1]);
show("b.meta", b["meta"]);
show("b[-1] missing", ([1, 2] as any)[-1]);

// `length` and prototype methods resolve through a string key too.
show('a["length"]', a["length"]);
show('typeof a["push"]', typeof a["push"]);

// Regression: plain numeric indexing and loops are unchanged.
show("a[0] num", a[0]);
show("a[2] num", a[2]);
let total = 0;
for (let i = 0; i < a.length; i++) total += a[i];
show("loop sum", total);
