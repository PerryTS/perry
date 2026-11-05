// A computed key built from literal parts names only members matching
// them; a key whose shape admits `constructor` may really be it.
class D {
  yy = 2;
}
const d = new D();
const kinds = ["mesh", "node"];
const json: any = { meshs: [d], nodes: [d, d] };
for (const type of kinds) {
  json[type + "s"].map((x: any) => console.log(type, x.yy));
  console.log(`${type}s`, json[`${type}s`].length);
}
const reg: any = {};
for (let i = 0; i < 2; i++) reg["mesh:" + i] = [d];
for (let i = 0; i < 2; i++) reg["mesh" + ":" + i].forEach((x: any) => console.log("reg", i, x.yy));
console.log(`${"mesh"}:${3}` in reg, reg[`mesh:${0}`].length);

const o: any = { name: d };
for (const prefix of ["a", "b"]) o[prefix + "_x"] = prefix.length;
console.log(o.name.yy, o.a_x, o.b_x, Object.keys(o).join(","));

// A shape that admits `constructor`, and a key that is `constructor`.
const rest = ["structor", "x"];
for (const r of rest) {
  const c = ({} as any)["con" + r];
  console.log(r, c === Object, typeof c);
  const f = (function () {} as any)["con" + r];
  console.log(r, f === Function, typeof f);
}
const tail = "uctor";
console.log((d as any)[`constr${tail}`] === D, ([] as any)["constr" + tail] === Array);

// ToPrimitive of the non-literal part: valueOf, null, undefined, BigInt.
const keyed: any = { "1n_k": "big", "null_k": "nul", "undefined_k": "und", "7_k": "seven" };
const v = { valueOf() { return 7; }, toString() { return "s"; } };
console.log(keyed[1n + "_k"], keyed[null + "_k"], keyed[undefined + "_k"], keyed[(v as any) + "_k"]);
// A numeric-looking shape may be an index.
const arr: any = ["zero", "one"];
for (const s of ["", "0"]) console.log(arr["1" + s]);
// A store whose shape admits a later read's name reaches it.
const p: any = { name: 1 };
for (const s of ["me", "x"]) p["na" + s] = s;
console.log(p.name, p.nax);
