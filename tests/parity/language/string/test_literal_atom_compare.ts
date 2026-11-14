// `x === "lit"` for a value the whole-program facts prove holds, as Strings,
// only short program literals is one pointer compare: the string pool keeps
// one String per such spelling. Every case prints what node prints; the
// negative controls are the Strings with a literal's spelling that are not
// the pool's String (built at run time, read from data, a single character
// of another String), which must still compare equal.
const many = process.argv.length > 100;

type Color = "white" | "black";
function opposite(c: Color): Color {
  return c === "white" ? "black" : "white";
}
function sign(c: Color): number {
  return c === "white" ? 1 : -1;
}

class Game {
  private side: Color = "white";
  private mode: "open" | "closed" | undefined = undefined;
  private tag: string = "a";
  private n = 0;

  make(): void {
    if (this.side === "black") this.n++;
    this.n += sign(this.side) * 3;
    this.side = opposite(this.side);
  }
  king(c: Color): number {
    return c === "white" ? 4 : 116;
  }
  attacked(by: Color): boolean {
    return by !== "white" ? this.n > 2 : this.n < -2;
  }
  open(on: boolean): void {
    this.mode = on ? "open" : "closed";
  }
  // `tag` is stored a built String too.
  retag(s: string): void {
    this.tag = s;
  }
  state(): string {
    return (
      (this.side === "white" ? "w" : "b") +
      (this.mode === "open" ? "O" : this.mode === "closed" ? "C" : "-") +
      (this.mode !== "open" ? "n" : "N") +
      (this.tag === "a" ? "A" : this.tag === "ab" ? "B" : "?") +
      ":" + this.n + ":" + this.king(this.side) + ":" + this.attacked(opposite(this.side))
    );
  }
}

// (a) Proven: a field and parameters that hold only short literals.
const g = new Game();
let trace = "";
for (let i = 0; i < 9; i++) {
  g.make();
  if (i === 3) g.open(true);
  if (i === 6) g.open(false);
  trace += g.state() + " ";
}
console.log("game", trace);

// (b) A local selected among literals, a literal that is not in the set.
function pick(k: number): string {
  const c = k % 3 === 0 ? "red" : k % 3 === 1 ? "green" : "blue";
  let out = "";
  if (c === "red") out += "R";
  if (c === "green") out += "G";
  if (c !== "blue") out += "!";
  if ((c as string) === "white") out += "W";
  return out;
}
console.log("pick", pick(0), pick(1), pick(2));

// Negative controls.

// (n1) Strings with a literal's spelling that are not the pool's String.
function isWhite(c: string): string {
  return c === "white" ? "T" : "F";
}
const built = "wh" + (many ? "x" : "ite");
const sliced = "xwhitex".slice(1, 6);
const parsed = JSON.parse('{"c":"white"}').c as string;
const joined = ["wh", "ite"].join("");
const lowered = "WHITE".toLowerCase();
console.log("built", isWhite("white"), isWhite(built), isWhite(sliced), isWhite(parsed), isWhite(joined),
  isWhite(lowered), isWhite("black"), isWhite(String("white")), isWhite(`${"wh"}ite`));

// (n2) Short Strings: a character of another String, a char code.
function isA(c: string): string {
  return c === "a" ? "T" : "F";
}
console.log("short", isA("a"), isA("banana"[1] ?? ""), isA("banana".charAt(3)), isA(String.fromCharCode(97)),
  isA("b"), isA(""));

// (n3) A field stored a built String beside its literals.
const h = new Game();
const s1 = h.state();
h.retag("a" + (many ? "x" : "b"));
const s2 = h.state();
h.retag(["a"].join(""));
console.log("fields", s1, s2, h.state());

// (n4) Values that are no String at all, through `any`.
function isOpen(c: any): string {
  return c === "open" ? "T" : "F";
}
const odd: any[] = ["open", "closed", undefined, null, 0, {}, new String("open"), ["open"], true];
let res = "";
for (const v of odd) res += isOpen(v);
console.log("any", res);

// (n5) The empty literal and a long literal are not pool atoms.
function isEmpty(c: string): string {
  return c === "" ? "T" : "F";
}
function isLong(c: string): string {
  return c === "0123456789012345678901234567890123456789012345678901234567890123456789" ? "T" : "F";
}
const longBuilt = "01234567890123456789012345678901234567890123456789012345678901234" + "56789";
console.log("edge", isEmpty(""), isEmpty("x"), isEmpty("abc".slice(3)),
  isLong("0123456789012345678901234567890123456789012345678901234567890123456789"), isLong(longBuilt), isLong("x"));
