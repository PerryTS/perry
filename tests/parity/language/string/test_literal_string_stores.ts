// Literal strings: a field or a local whose every String is a string
// literal's value needs no string demote when it is stored or aliased (the
// whole-program facts prove it). Every value below must stay what
// JavaScript says it is, in particular after an in-place append on a
// string that was stored or aliased just before.
const many = process.argv.length > 100;

// (a) The proven shape: a two-valued field flipped by a function that
// returns literals, a parameter every caller passes literals, an alias.
type Color = "white" | "black";
function opposite(c: Color): Color {
  return c === "white" ? "black" : "white";
}
function sign(c: Color): number {
  return c === "white" ? 1 : -1;
}
class Game {
  side: Color = "white";
  moves = 0;
  score = 0;
  make(): void {
    this.side = opposite(this.side);
    this.moves++;
    this.score += sign(this.side) * this.moves;
  }
  unmake(): void {
    this.side = opposite(this.side);
    this.moves--;
  }
  load(s: string): void {
    this.side = s === "w" ? "white" : "black";
  }
  copy(o: Game): void {
    this.side = o.side;
  }
  attacked(by: Color): number {
    const color = sign(by);
    const who = by;
    return color * (who === "black" ? 2 : 3);
  }
}
const g = new Game();
let acc = 0;
for (let i = 0; i < 9; i++) {
  g.make();
  acc += g.attacked(g.side) + g.attacked(opposite(g.side));
  if (i % 3 === 0) g.unmake();
}
g.load(many ? "w" : "b");
const h = new Game();
h.copy(g);
h.make();
console.log("side", g.side, h.side, g.moves, g.score, acc);

// (b) Negative control: a field that also receives an appended string. The
// accumulator is stored, then appended to in place: the stored value must
// not change.
class Log {
  last = "none";
  first = "none";
  keep(s: string): void {
    this.last = s;
  }
  once(s: string): void {
    if (this.first === "none") this.first = s;
  }
}
const log = new Log();
let line = "a";
for (let i = 0; i < 6; i++) {
  line += String(i);
  if (i === 2) {
    log.keep(line);
    log.once(line);
  }
}
line += "-tail";
console.log("log", log.last, log.first, line);

// (c) Negative control: a parameter fed from JSON and from input, aliased
// and then appended to in place by the caller.
function tag(by: string): string {
  let who = by;
  const before = who;
  who += "!";
  return before + "/" + who;
}
const parsed: { name: string } = JSON.parse('{"name":"json-name-that-is-long-enough"}');
let grown = "x";
for (let i = 0; i < 5; i++) grown += "y";
const t1 = tag(parsed.name);
const t2 = tag(grown);
grown += "z";
const t3 = tag(process.argv.length > 100 ? "never" : grown);
console.log("tag", t1, t2, t3, tag("lit"), grown);

// (d) Negative control: a value that escapes to a generic writer. The field
// holds literals in its own class's code, but an untyped store through a
// computed key writes an appended string into it.
class Cell {
  kind = "empty";
  set(k: string): void {
    this.kind = k;
  }
}
function poke(o: any, key: string, v: any): void {
  o[key] = v;
}
const cell = new Cell();
cell.set("full");
let built = "k";
for (let i = 0; i < 4; i++) built += "+";
poke(cell, many ? "none" : "kind", built);
const held = cell.kind;
built += "?";
console.log("cell", cell.kind, held, built);

// (e) Aliases among locals: a literal-valued local copied around, and one
// that an append reaches through an assignment.
function shuffle(n: number): string {
  let a = n > 2 ? "hi" : "lo";
  let b = a;
  let c = b;
  if (n > 5) c = "mid";
  let d = "s";
  for (let i = 0; i < n; i++) d += "t";
  let e = d;
  d += "u";
  b = c;
  return a + b + c + ":" + e + ":" + d;
}
console.log("shuffle", shuffle(1), shuffle(4), shuffle(7));

// (f) A literal past the pool atom length and the empty literal are
// literals too; a mixed field holds a Number beside them.
class Wide {
  text: string | number = "";
  set(k: number): void {
    this.text =
      k === 0
        ? ""
        : k === 1
          ? "a literal that is longer than sixty-four bytes, so the pool does not atomise it at all"
          : k;
  }
}
const w = new Wide();
const seenWide: Array<string | number> = [];
for (let k = 0; k < 4; k++) {
  w.set(k);
  seenWide.push(w.text);
}
console.log("wide", seenWide.join("|"), typeof w.text);
