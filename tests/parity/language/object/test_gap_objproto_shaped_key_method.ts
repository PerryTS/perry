// A computed key built from literal parts that reaches Object.prototype reads
// each method its shape can spell: `t["__define" + s]` is `__defineGetter__`,
// and the getter it installs runs.
class Cell {
  witness = 1;
}
let cell: any = new Cell();
const t: any = {};
const s: string = JSON.parse('"Getter__"');
t["__define" + s]("y", function () {
  cell = { other: 2 };
  return 7;
});
console.log(t.y, cell.witness, cell.other);
const u: any = {};
const z: string = JSON.parse('"Setter__"');
let seen: any = null;
u["__define" + z]("w", function (v: any) {
  seen = v;
});
u.w = 5;
console.log(seen, u.w);
// A shape that spells no Object.prototype method: the call throws.
try {
  t["zz" + s]("q", () => 1);
} catch (e) {
  console.log((e as Error).constructor.name);
}
console.log(new Cell().witness);
