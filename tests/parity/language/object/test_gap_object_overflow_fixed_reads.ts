// Fixed (layout) keys of objects that also have an overflow: reads through a
// site that saw plain objects of the same layout first, after a delete, after
// an accessor redefinition, and through dynamic keys and hasOwn.

class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

function readX(p: any): any {
  return p.x;
}

function readKey(p: any, k: string): any {
  return p[k];
}

const plain: any[] = [];
for (let i = 0; i < 8; i++) plain.push(new Point(i, i * 2));
let sum = 0;
for (let r = 0; r < 50; r++) for (const p of plain) sum += readX(p);
console.log("plain", sum);

// Same layout, plus keys added through computed names (an overflow).
const extended: any[] = [];
for (let i = 0; i < 8; i++) {
  const p: any = new Point(i + 100, i);
  const k = "extra" + (i % 3);
  p[k] = i;
  extended.push(p);
}
sum = 0;
for (let r = 0; r < 50; r++) for (const p of extended) sum += readX(p);
console.log("extended", sum, Object.keys(extended[4]).join(","));

// Delete a fixed key: the read must see undefined, not a stale slot.
const d: any = extended[1];
delete d.x;
console.log("deleted", readX(d), "x" in d, Object.prototype.hasOwnProperty.call(d, "x"), Object.keys(d).join(","));
d.x = 7;
console.log("readded", readX(d), Object.keys(d).join(","));

// Redefine a fixed key as an accessor on an object with an overflow.
const a: any = extended[2];
let calls = 0;
Object.defineProperty(a, "x", {
  get() {
    calls++;
    return "getter";
  },
  configurable: true,
  enumerable: true,
});
console.log("accessor", readX(a), readX(a), calls, readKey(a, "x"), Object.keys(a).join(","));
a.x = 5; // no setter: ignored in sloppy mode
console.log("accessor-after-write", readX(a), calls);
Object.defineProperty(a, "x", { value: 9, writable: true, configurable: true, enumerable: true });
console.log("data-again", readX(a), readKey(a, "x"));
a.x = 11;
console.log("data-write", readX(a), JSON.stringify(a));

// An inherited key shadowed by an overflow entry.
const proto: any = { greet: "proto" };
const child: any = Object.create(proto);
console.log("inherited", child.greet);
const name = "gr" + "eet";
child[name] = "own";
console.log("shadowed", child.greet, readKey(child, "greet"), Object.prototype.hasOwnProperty.call(child, "greet"));
delete child[name];
console.log("unshadowed", child.greet, Object.prototype.hasOwnProperty.call(child, "greet"));

// Dynamic keys on a table-from-birth JSON object.
const j: any = JSON.parse('{"b":1,"a":2,"2":3,"1":4}');
const keys = ["a", "b", "1", "2", "c"];
console.log("json", keys.map((k) => readKey(j, k)).join(","), keys.map((k) => Object.prototype.hasOwnProperty.call(j, k)).join(","), Object.keys(j).join(","));
