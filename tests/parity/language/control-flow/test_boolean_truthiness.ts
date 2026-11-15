// The truthiness of a value the whole-program facts prove a Boolean (a
// parameter every caller passes `true` or `false`, a field every store
// writes a Boolean into) is one compare with `true`. Every case prints
// what node prints; the negative controls are the values an annotation
// calls `boolean` that are something else at run time.
const many = process.argv.length > 100;

interface Undo {
  from: number;
  tracked: boolean;
}

class Game {
  private side = 0;
  private keys: string[] = [];
  private done = false;
  private loose: boolean = false;
  private maybe: boolean | undefined = undefined;

  make(move: number, tracked: boolean): Undo {
    const undo: Undo = { from: this.side, tracked };
    this.side = (this.side + move) & 1023;
    if (tracked) this.keys.push("k" + this.side);
    return undo;
  }
  unmake(undo: Undo): void {
    this.side = undo.from;
    if (undo.tracked) this.keys.pop();
  }
  finish(): void {
    this.done = this.keys.length > 2 || this.side === 7;
  }
  // `loose` is declared boolean and stored a Number through `any`.
  setLoose(v: boolean): void {
    this.loose = v;
  }
  setMaybe(v: boolean | undefined): void {
    this.maybe = v;
  }
  state(): string {
    return (
      (this.done ? "D" : "d") +
      (this.loose ? "L" : "l") +
      (this.maybe ? "M" : "m") +
      (!this.done ? "n" : "N") +
      (this.done && this.side > 0 ? "A" : "a") +
      (this.done || this.side > 500 ? "O" : "o") +
      ":" + this.side + ":" + this.keys.length
    );
  }
}

// (a) Proven Booleans: parameters and fields.
const g = new Game();
let acc = 0;
const undos: Undo[] = [];
for (let i = 0; i < 40; i++) {
  undos.push(g.make(i * 7 + 1, i % 3 === 0));
  if (i % 5 === 4) {
    const u = undos.pop();
    if (u !== undefined) g.unmake(u);
  }
  g.finish();
  if (i % 9 === 0) acc += g.state().length;
}
console.log("game", g.state(), undos.length, acc);

// (b) Locals: a comparison result, a negation, a loop flag.
function scan(xs: number[], limit: number): string {
  let found = false;
  let all = true;
  let out = "";
  for (let i = 0; i < xs.length; i++) {
    const big = (xs[i] ?? 0) > limit;
    if (big) found = true;
    else all = false;
    const odd = ((xs[i] ?? 0) & 1) === 1;
    out += odd && !big ? "o" : big ? "B" : "-";
  }
  return out + (found ? "F" : "f") + (all ? "A" : "a") + (found && !all ? "X" : "x");
}
console.log("scan", scan([1, 5, 9, 2], 4), scan([], 0), scan([8, 9], 3));

// Negative controls: declared `boolean`, but the value is not one.

// (n1) A Number, a String, null, undefined, an object through `any`.
function pick(flag: boolean): string {
  return flag ? "T" : "F";
}
const odd: any[] = [0, 1, "", "x", null, undefined, {}, NaN, -0, 0n, 1n, [], true, false];
let picks = "";
for (const v of odd) picks += pick(v);
console.log("pick", picks);

// (n2) An optional parameter and a missing argument.
function opt(flag?: boolean): string {
  return flag ? "T" : "F";
}
console.log("opt", opt(), opt(true), opt(false), (opt as any)(1));

// (n3) A field stored a non-Boolean, a field that starts undefined.
const h = new Game();
h.setLoose(1 as any);
const s1 = h.state();
h.setLoose("" as any);
h.setMaybe(true);
const s2 = h.state();
h.setLoose(false);
h.setMaybe(undefined);
console.log("fields", s1, s2, h.state());

// (n4) An uninitialized local, a local assigned in one branch only.
function late(n: number): string {
  let flag: boolean;
  let other!: boolean;
  if (n > 2) flag = true;
  // @ts-ignore: read before assignment on purpose
  const a = flag ? "T" : "F";
  const b = other ? "T" : "F";
  return a + b;
}
console.log("late", late(1), late(5));

// (n5) A record field that may be absent, and a wider object.
function tracked(u: { tracked?: boolean }): string {
  return u.tracked ? "T" : "F";
}
console.log("absent", tracked({}), tracked({ tracked: true }), tracked({ tracked: false }));

// (n6) A Boolean object is truthy even when it wraps `false`.
function boxed(flag: boolean): string {
  return flag ? "T" : "F";
}
console.log("boxed", boxed(new Boolean(false) as any), boxed(false), boxed(Boolean(0)));

// (n7) A flag written by a closure and by a callee the facts cannot see.
function closure(n: number): string {
  let hit = false;
  const mark = (v: any) => {
    hit = v;
  };
  mark(n > 1);
  const a = hit ? "T" : "F";
  mark(many ? true : n);
  return a + (hit ? "T" : "F");
}
console.log("closure", closure(0), closure(3));
