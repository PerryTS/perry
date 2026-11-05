// JSON.parse records are born with the layout of their key sequence. These
// cases pin the semantics that birth must keep: duplicate keys, `__proto__`
// as data, integer-like key order, revivers that add or delete keys, records
// written after birth, and objects above the width bound.

// Duplicate keys: the later value wins, the key keeps its first position.
const dup: any = JSON.parse('{"a":1,"b":2,"a":3,"c":4,"b":5}');
console.log("dup", Object.keys(dup).join(","), dup.a, dup.b, dup.c, JSON.stringify(dup));
const dupRecords: any[] = JSON.parse(
  '[{"x":1,"y":2},{"x":3,"y":4,"x":5},{"y":6,"x":7},{"x":8,"y":9,"y":10,"z":11}]',
);
for (const r of dupRecords) console.log("dupRec", Object.keys(r).join(","), r.x, r.y, r.z, JSON.stringify(r));

// `__proto__` is an own data property of a parsed object, not a prototype write.
const proto: any = JSON.parse('{"a":1,"__proto__":{"polluted":true},"b":2}');
console.log("proto", Object.keys(proto).join(","), proto.polluted, Object.getPrototypeOf(proto) === Object.prototype);
console.log("protoOwn", Object.prototype.hasOwnProperty.call(proto, "__proto__"), JSON.stringify(proto));
console.log("protoClean", ({} as any).polluted);
const protoDup: any = JSON.parse('{"__proto__":1,"__proto__":2}');
console.log("protoDup", Object.keys(protoDup).join(","), JSON.stringify(protoDup));

// Integer-like keys enumerate first in ascending order; "01", "-1" and
// 2^32-1 are not array indices and keep insertion order.
const nums: any = JSON.parse('{"b":1,"10":2,"01":3,"1":4,"-1":5,"4294967295":6,"4294967294":7,"a":8}');
console.log("nums", Object.keys(nums).join(","), nums["1"], nums["01"], nums[10], JSON.stringify(nums));
const numRecords: any[] = JSON.parse('[{"2":"a","1":"b"},{"2":"c","1":"d"},{"1":"e","2":"f"}]');
for (const r of numRecords) console.log("numRec", Object.keys(r).join(","), r[1], r[2]);

// Revivers that add and delete keys.
const revived: any = JSON.parse('{"keep":1,"drop":2,"nested":{"x":1,"drop":3},"list":[{"a":1},{"a":2,"drop":4}]}', function (this: any, k, v) {
  if (k === "drop") return undefined;
  if (v && typeof v === "object" && !Array.isArray(v)) {
    v.added = Object.keys(v).length;
  }
  return v;
});
console.log("revived", JSON.stringify(revived), Object.keys(revived.nested).join(","));
const holderEdit: any = JSON.parse('{"a":1,"b":2,"c":3}', function (this: any, k, v) {
  if (k === "a") { delete this.b; this.late = 9; }
  return v;
});
console.log("holderEdit", JSON.stringify(holderEdit), Object.keys(holderEdit).join(","));

// Records stay ordinary objects after birth: writes, dynamic writes that name
// a born key, deletes and re-adds.
const recs: any[] = JSON.parse('[{"id":1,"name":"a","score":1.5},{"id":2,"name":"b","score":2.5},{"id":3,"name":"c","score":3.5}]');
const k = ["sc", "ore"].join("");
recs[0][k] = 10;
recs[1].extra = true;
delete recs[2].name;
recs[2].name = "z";
for (const r of recs) console.log("rec", Object.keys(r).join(","), r.score, r[k], JSON.stringify(r));
let total = 0;
for (const r of recs) total += r.score;
console.log("total", total, "missing", recs[0].nope, "in", "score" in recs[0], "nope" in recs[0]);
const copy = { ...recs[1] };
console.log("spread", JSON.stringify(copy), JSON.stringify(Object.assign({}, recs[2])));

// Above the width bound (more than 1024 keys) and just below it.
for (const n of [1024, 1025, 3000]) {
  let text = "{";
  for (let i = 0; i < n; i++) text += (i ? "," : "") + '"k' + ((i * 7919) % n) + '":' + i;
  text += ',"k0":"last"}';
  const wide: any = JSON.parse(text);
  const keys = Object.keys(wide);
  wide["k1"] = "dyn";
  wide.fresh = 1;
  delete wide.k2;
  const back = JSON.parse(JSON.stringify(wide));
  console.log("wide", n, keys.length, keys[0], keys[1], keys[n - 1], wide.k0, wide.k1, wide.k2, wide.fresh,
    Object.keys(wide).length, Object.keys(back).length, back.k0, Object.keys(back)[n - 1]);
}

// Many distinct small key sequences in one document.
const mixed: any[] = JSON.parse(JSON.stringify(Array.from({ length: 40 }, (_, i) => {
  const o: any = {};
  for (let j = 0; j < (i % 9); j++) o["f" + ((i + j) % 11)] = j;
  return o;
})));
console.log("mixed", mixed.map((o) => Object.keys(o).join("")).join("|"));
