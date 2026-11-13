// Gap test for #12666: a local that holds a Number at try entry, is set to
// `undefined` inside the try and then overwritten with a Number after a
// conditional throw. The catch and the finally run after the `undefined`
// write, so `v + 1` there is NaN. The Number-at-read flow entered the catch
// with the try's entry state, so `v + 1` was lowered as a plain double add on
// the `undefined` word and printed `undefined`.
function f(k: number) {
  let v: number | undefined = undefined;
  v = 1;
  try {
    v = undefined;
    if (k) throw 0;
    v = 2;
  } catch (e) {
    let out = "";
    out += (v === undefined) + "," + (v == null) + "," + (v ?? 5) + "," + String(v) + "," + typeof v + ",";
    out += ((v as any) | 0) + "," + ((v as any) + 1) + "," + `${v}` + "," + [v].join("|") + "," + (v! > 0);
    return out;
  }
  return String(v);
}
function g(k: number) {
  let v: number | undefined;
  v = 1;
  try { v = undefined; if (k) throw 0; v = 2; } finally { console.log("fin", v === undefined, String(v), (v as any) + 1); }
  return v;
}
console.log(f(1)); console.log(f(0));
try { g(1) } catch {} ; g(0);
