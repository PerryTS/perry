function show(label: string, s: string): void {
  console.log(label + " len=" + s.length + " cp=" + [...s].length + " cp0=" + s.codePointAt(0) + " eq=");
}
const e = "a👋b";
const hi = e[1], lo = e[2];
let y = ""; y += hi; y += lo;
show("pluseq", y);
console.log("pluseq-eq", y === "👋", (hi + lo) === y);
let out = "";
for (let i = 0; i < e.length; i++) out += e[i];
console.log("rebuild", out === e, [...out].length);
const t = "x😀y👋z🎉w";
let r = "";
for (let i = 0; i < t.length; i++) r += t[i];
console.log("multi", r === t, [...r].length, r.codePointAt(1));
let lone = ""; lone += hi; lone += "Z";
console.log("lone", [...lone].length, lone.codePointAt(0), lone.length);
let ascii = "";
for (let i = 0; i < 500; i++) ascii += "aé";
console.log("bulk", ascii.length, [...ascii].length);
let split = ""; split += hi; split += "-"; split += lo;
console.log("split", [...split].length, split.codePointAt(0));
